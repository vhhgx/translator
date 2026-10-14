//! Zephyr Translator — Tauri backend.
//!
//! Rust owns config, credentials, HTTP and document IO; the Vue layer is
//! presentation only.

pub mod client;
pub mod config;
pub mod document;
pub mod error;
pub mod glossary;
pub mod history;
pub mod prompt;
pub mod selection;
// Windows 取词走 UI Automation，见该模块说明。只有 Windows 参与编译。
#[cfg(windows)]
mod selection_win;
pub mod tray;

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{Manager, State};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use client::{TranslateRequest, TranslateResponse};
use config::{Backend, Settings};
use error::{AppError, AppResult};
use glossary::Glossary;
use history::History;
use prompt::Mode;

pub struct AppState {
    settings: Mutex<Settings>,
    glossary: Mutex<Glossary>,
}

/// History lives in a global because streaming jobs run detached from the
/// command's `State`, which cannot be moved into a `'static` task.
static HISTORY: Mutex<Option<History>> = Mutex::new(None);

type CmdResult<T> = Result<T, AppError>;

// ---------- metadata ----------

#[derive(Serialize)]
pub struct Meta {
    languages: Vec<(&'static str, &'static str)>,
    styles: Vec<&'static str>,
    format_types: Vec<&'static str>,
    modes: Vec<(&'static str, &'static str)>,
}

#[tauri::command]
fn get_meta() -> Meta {
    Meta {
        languages: prompt::SUPPORTED_LANGUAGES.to_vec(),
        styles: prompt::STYLES.to_vec(),
        format_types: prompt::FORMAT_TYPES.to_vec(),
        modes: prompt::Mode::ALL
            .iter()
            .map(|m| (mode_key(*m), m.label()))
            .collect(),
    }
}

fn mode_key(m: Mode) -> &'static str {
    match m {
        Mode::Basic => "basic",
        Mode::Terminology => "terminology",
        Mode::Style => "style",
        Mode::Context => "context",
        Mode::Delimiter => "delimiter",
        Mode::Structured => "structured",
    }
}

// ---------- settings / backends ----------

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mut s: Settings,
) -> CmdResult<()> {
    // has_api_key is derived, never trusted from the frontend.
    for b in &mut s.backends {
        b.has_api_key = config::load_secret(&b.id).is_some();
    }
    if !s.backends.iter().any(|b| b.id == s.active_id) {
        if let Some(f) = s.backends.first() {
            s.active_id = f.id.clone();
        }
    }
    s.save()?;
    let (sel_on, sel_mod) = (s.selection_enabled, s.selection_modifier.clone());
    *state.settings.lock().unwrap() = s;
    // 开关一变就同步给监听线程；打开时才真正拉起钩子。
    selection::configure(&app, sel_on, &sel_mod);
    Ok(())
}

// ---------- 托盘 / 全局热键 / 开机自启 ----------

/// 改某一个全局热键。`kind` 取 `tray::HOTKEY_TOGGLE` 或 `tray::HOTKEY_MINI`。
///
/// 注册结果不在这里返回，而是走 `hymt2:hotkey` 事件逐个回报：两个热键共用
/// 一次 `unregister_all`，改一个会把另一个一起重注册，两者的成败都得报。
#[tauri::command]
fn set_hotkey(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    kind: String,
    accel: String,
) -> CmdResult<String> {
    let accel = accel.trim().to_string();
    let snapshot = {
        let mut s = state.settings.lock().unwrap();
        match kind.as_str() {
            tray::HOTKEY_TOGGLE => s.hotkey_toggle = accel.clone(),
            tray::HOTKEY_MINI => s.hotkey_mini = accel.clone(),
            other => return Err(AppError::Io(format!("未知的热键类型：{other}"))),
        }
        s.save()?;
        s.clone()
    };
    tray::apply_hotkeys(&app, &snapshot);
    Ok(accel)
}

#[tauri::command]
fn get_hotkey(state: State<'_, AppState>) -> String {
    state.settings.lock().unwrap().hotkey_toggle.clone()
}

/// 查询热键当前的真实注册状态。
///
/// 不能只靠启动时推事件：setup() 早于前端挂载 listen()，那条事件必然
/// 被丢掉，界面会一直显示不出状态。所以让前端能主动查一次。
#[derive(Serialize)]
pub struct HotkeyEntry {
    pub accel: String,
    pub registered: bool,
}

/// 两个热键各自的真实注册状态。
#[derive(Serialize)]
pub struct HotkeyStatus {
    pub toggle: HotkeyEntry,
    pub mini: HotkeyEntry,
}

fn hotkey_entry(app: &tauri::AppHandle, accel: &str) -> HotkeyEntry {
    let registered = if accel.trim().is_empty() {
        false
    } else {
        accel
            .parse::<tauri_plugin_global_shortcut::Shortcut>()
            .map(|sc| app.global_shortcut().is_registered(sc))
            .unwrap_or(false)
    };
    HotkeyEntry {
        accel: accel.to_string(),
        registered,
    }
}

#[tauri::command]
fn hotkey_status(app: tauri::AppHandle, state: State<'_, AppState>) -> HotkeyStatus {
    let (toggle, mini) = {
        let s = state.settings.lock().unwrap();
        (s.hotkey_toggle.clone(), s.hotkey_mini.clone())
    };
    HotkeyStatus {
        toggle: hotkey_entry(&app, &toggle),
        mini: hotkey_entry(&app, &mini),
    }
}

/// 开机自启状态以系统实际注册情况为准，不信任前端缓存。
#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enable: bool) -> CmdResult<()> {
    use tauri_plugin_autostart::ManagerExt;
    let m = app.autolaunch();
    let r = if enable {
        m.enable()
    } else {
        m.disable()
    };
    r.map_err(|e| AppError::Io(e.to_string()))
}

/// 从托盘或热键唤出窗口，也给前端「显示窗口」用。
#[tauri::command]
fn show_window(app: tauri::AppHandle) {
    tray::show_window(&app);
}

/// 小窗上的关闭按钮 / Esc 走这里。只隐藏，窗口留着——真销毁了热键就再也
/// 唤不出来了。
#[tauri::command]
fn hide_mini(app: tauri::AppHandle) {
    // 收起即解除钉住，否则下次热键唤出时仍是钉住态，不会失焦隐藏。
    tray::set_mini_pinned(false);
    if let Some(w) = app.get_webview_window("mini") {
        let _ = w.hide();
    }
}

/// 小窗「钉住」：钉住后点击别处不再收起（与划词弹窗同一套交互）。
#[tauri::command]
fn set_mini_pinned(pinned: bool) {
    tray::set_mini_pinned(pinned);
}

// ---------- 划词翻译 ----------

/// 监听线程的真实状态。起不来（Wayland 等）时必须把原因显示给用户，
/// 否则开关打开了却毫无反应，无从排查——和热键那套是同一个思路。
#[tauri::command]
fn selection_status() -> selection::SelectionStatus {
    selection::status()
}

/// 划词弹窗上的关闭按钮。
#[tauri::command]
fn hide_selection_popup(app: tauri::AppHandle) {
    selection::hide(&app);
}

/// 弹窗挂载后主动取一次待处理文本，见 `selection::take_pending` 的说明。
#[tauri::command]
fn take_pending_selection() -> Option<String> {
    selection::take_pending()
}

/// 钉住/取消钉住划词弹窗。钉住后点击别处不再自动收起，直到用户手动关闭。
#[tauri::command]
fn set_popup_pinned(pinned: bool) {
    selection::set_pinned(pinned);
}

/// 供划词弹窗「复制译文」用。直接用 arboard，不再额外引入 clipboard 插件。
#[tauri::command]
fn copy_text(text: String) -> CmdResult<()> {
    let mut cb = arboard::Clipboard::new().map_err(|e| AppError::Io(e.to_string()))?;
    cb.set_text(text).map_err(|e| AppError::Io(e.to_string()))
}

/// 读剪贴板文本，给小窗的「粘贴」按钮用。读不到（空 / 非文本）就给空串，
/// 不当成错误——按钮点一下没反应比弹个错更合适。
#[tauri::command]
fn read_clipboard_text() -> String {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok())
        .unwrap_or_default()
}

#[tauri::command]
fn set_active_backend(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    {
        let mut s = state.settings.lock().unwrap();
        if !s.backends.iter().any(|b| b.id == id) {
            return Err(AppError::Io(format!("后端 {id} 不存在")));
        }
        s.active_id = id;
        s.save()?;
    }
    Ok(())
}

#[derive(Serialize)]
pub struct KeyStatus {
    pub backend_id: String,
    pub has_key: bool,
}

/// Store (or clear) an API key in the OS credential manager.
#[tauri::command]
fn set_api_key(state: State<'_, AppState>, backend_id: String, api_key: String) -> CmdResult<KeyStatus> {
    let key = api_key.trim().to_string();
    if key.is_empty() {
        config::delete_secret(&backend_id)?;
    } else {
        config::store_secret(&backend_id, &key)?;
    }
    let has = config::load_secret(&backend_id).is_some();
    {
        let mut s = state.settings.lock().unwrap();
        if let Some(b) = s.backends.iter_mut().find(|b| b.id == backend_id) {
            b.has_api_key = has;
        }
        s.save()?;
    }
    Ok(KeyStatus { backend_id, has_key: has })
}

/// Probe a backend and return the models it advertises.
#[tauri::command]
async fn test_backend(backend: Backend) -> Result<Vec<String>, String> {
    client::check_health(&backend).await
}

// ---------- translate ----------

#[tauri::command]
async fn translate(
    state: State<'_, AppState>,
    req: TranslateRequest,
    save_history: Option<bool>,
) -> CmdResult<TranslateResponse> {
    let (backend, temperature, timeout) = {
        let s = state.settings.lock().unwrap();
        let b = s
            .active()
            .cloned()
            .ok_or_else(|| AppError::Io("尚未配置任何后端".into()))?;
        (b, s.temperature, s.timeout_secs)
    };
    let key = config::load_secret(&backend.id);
    let resp = client::translate(&backend, key.as_deref(), temperature, timeout, &req).await?;

    if save_history.unwrap_or(true) {
        let id = uuid::Uuid::new_v4().to_string();
        let _ = with_history(|h| {
            h.add(&history::HistoryItem {
                id: id.clone(),
                created_at: now_millis(),
                source: req.text.clone(),
                translation: resp.text.clone(),
                source_lang: "auto".into(),
                target_lang: resp.target_lang.clone(),
                backend_id: backend.id.clone(),
                mode: mode_key(req.mode).to_string(),
                starred: false,
            })
        });
        return Ok(TranslateResponse {
            history_id: Some(id),
            ..resp
        });
    }
    Ok(resp)
}

/// 检测一段文本的语种。走模型自身的语种识别指令——选「检测语言」时
/// 让前端能把「检测语言」徽标替换成具体检测到的语言名。
///
/// 复用 OpenAI 兼容端点，prompt 极简、只要求输出语言名。
#[tauri::command]
async fn detect_language(
    state: State<'_, AppState>,
    text: String,
) -> CmdResult<String> {
    let (backend, timeout) = {
        let s = state.settings.lock().unwrap();
        let b = s
            .active()
            .cloned()
            .ok_or_else(|| AppError::Io("尚未配置任何后端".into()))?;
        (b, s.timeout_secs)
    };
    let key = config::load_secret(&backend.id);
    client::detect_language(&backend, key.as_deref(), timeout, &text).await
}

/// Streaming translation.
///
/// Returns immediately; tokens are pushed to the webview as events:
///   - `hymt2:delta`  { job_id, text }   one or more times
///   - `hymt2:done`   { job_id, text, target_lang, ms, used_terms, history_id }
///   - `hymt2:error`  { job_id, message }
#[tauri::command]
async fn translate_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: TranslateRequest,
    job_id: String,
) -> CmdResult<()> {
    let (backend, temperature, timeout) = {
        let s = state.settings.lock().unwrap();
        let b = s
            .active()
            .cloned()
            .ok_or_else(|| AppError::Io("尚未配置任何后端".into()))?;
        (b, s.temperature, s.timeout_secs)
    };
    let key = config::load_secret(&backend.id);
    let started = std::time::Instant::now();

    tauri::async_runtime::spawn(async move {
        use tauri::Emitter;

        let app2 = app.clone();
        let jid = job_id.clone();
        let result = client::translate_stream(&backend, key.as_deref(), temperature, timeout, &req, |delta| {
            let _ = app2.emit(
                "hymt2:delta",
                serde_json::json!({ "job_id": jid, "text": delta }),
            );
        })
        .await;

        match result {
            Ok(text) => {
                let (target_lang, used_terms) = match client::compose(&req) {
                    Ok((_, lang, used)) => (lang, used),
                    Err(_) => (String::new(), Vec::new()),
                };
                // 先生成 id 再入库，让前端拿到 history_id 就能立刻打星/操作这条记录。
                let history_id = uuid::Uuid::new_v4().to_string();
                let _ = app.emit(
                    "hymt2:done",
                    serde_json::json!({
                        "job_id": job_id,
                        "text": text,
                        "target_lang": target_lang,
                        "used_terms": used_terms,
                        "ms": started.elapsed().as_millis(),
                        "history_id": history_id,
                    }),
                );
                // Persist to history once the stream finishes.
                let hist = HISTORY.lock().unwrap();
                if let Some(h) = hist.as_ref() {
                    let _ = h.add(&history::HistoryItem {
                        id: history_id,
                        created_at: now_millis(),
                        source: req.text.clone(),
                        translation: text,
                        source_lang: "auto".into(),
                        target_lang,
                        backend_id: backend.id.clone(),
                        mode: mode_key(req.mode).to_string(),
                        starred: false,
                    });
                }
            }
            Err(e) => {
                let _ = app.emit(
                    "hymt2:error",
                    serde_json::json!({ "job_id": job_id, "message": e.to_string() }),
                );
            }
        }
    });

    Ok(())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Preview the exact prompt that would be sent — useful for debugging.
#[tauri::command]
fn preview_prompt(req: TranslateRequest) -> CmdResult<String> {
    let (p, _, _) = client::compose(&req)?;
    Ok(p)
}

// ---------- glossary ----------

#[derive(Serialize)]
pub struct GlossaryView {
    pub active: String,
    pub names: Vec<String>,
    /// 与 `names` 对齐的各词库词条数，用于页签与导航徽标。
    pub counts: Vec<usize>,
    pub terms: Vec<[String; 2]>,
}

#[tauri::command]
fn glossary_get(state: State<'_, AppState>) -> GlossaryView {
    let g = state.glossary.lock().unwrap();
    let names = g.names();
    let counts = names.iter().map(|n| g.terms(n).len()).collect();
    GlossaryView {
        terms: g.terms(&g.active),
        names,
        counts,
        active: g.active.clone(),
    }
}

#[tauri::command]
fn glossary_set_terms(
    state: State<'_, AppState>,
    name: String,
    terms: Vec<[String; 2]>,
) -> CmdResult<()> {
    let mut g = state.glossary.lock().unwrap();
    g.set_terms(&name, terms);
    g.save()
}

#[tauri::command]
fn glossary_activate(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    let mut g = state.glossary.lock().unwrap();
    g.set_active(&name);
    g.save()
}

#[tauri::command]
fn glossary_create(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    let mut g = state.glossary.lock().unwrap();
    g.create_set(&name)?;
    g.save()
}

#[tauri::command]
fn glossary_rename(state: State<'_, AppState>, old: String, new: String) -> CmdResult<()> {
    let mut g = state.glossary.lock().unwrap();
    g.rename_set(&old, &new)?;
    g.save()
}

#[tauri::command]
fn glossary_delete(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    let mut g = state.glossary.lock().unwrap();
    g.delete_set(&name)?;
    g.save()
}

#[tauri::command]
fn glossary_export(state: State<'_, AppState>, name: String) -> glossary::ExportPayload {
    let g = state.glossary.lock().unwrap();
    glossary::ExportPayload {
        terms: g.terms(&name),
        name,
    }
}

#[tauri::command]
fn glossary_import(
    state: State<'_, AppState>,
    name: String,
    terms: Vec<[String; 2]>,
    replace: bool,
) -> CmdResult<usize> {
    let mut g = state.glossary.lock().unwrap();
    let n = g.import(&name, terms, replace);
    g.save()?;
    Ok(n)
}

// ---------- history ----------

fn with_history<T>(f: impl FnOnce(&History) -> AppResult<T>) -> CmdResult<T> {
    let mut guard = HISTORY.lock().unwrap();
    if guard.is_none() {
        *guard = Some(History::open()?);
    }
    f(guard.as_ref().unwrap())
}

#[tauri::command]
fn history_list(
    search: Option<String>,
    starred: Option<bool>,
    limit: Option<i64>,
) -> CmdResult<Vec<history::HistoryItem>> {
    with_history(|h| h.list(search.as_deref(), starred, limit.unwrap_or(100)))
}

#[tauri::command]
fn history_count() -> CmdResult<i64> {
    with_history(|h| h.count())
}

#[tauri::command]
fn history_set_starred(id: String, starred: bool) -> CmdResult<()> {
    with_history(|h| h.set_starred(&id, starred))
}

#[tauri::command]
fn history_delete(id: String) -> CmdResult<()> {
    with_history(|h| h.delete(&id))
}

#[tauri::command]
fn history_clear() -> CmdResult<()> {
    with_history(|h| h.clear())
}

// ---------- documents ----------

#[tauri::command]
fn doc_load(path: String) -> CmdResult<document::LoadedDocument> {
    document::load(&path)
}

#[tauri::command]
fn doc_output_path(path: String, target_lang: String) -> String {
    document::output_path(&path, &target_lang)
}

#[tauri::command]
fn doc_save(path: String, content: String) -> CmdResult<()> {
    document::save(&path, &content)
}

#[tauri::command]
async fn doc_translate(
    state: State<'_, AppState>,
    path: String,
    target_lang: String,
    terms: Vec<[String; 2]>,
    concurrency: Option<usize>,
) -> CmdResult<DocProgress> {
    let doc = document::load(&path)?;
    let (backend, temperature, timeout) = {
        let s = state.settings.lock().unwrap();
        let b = s
            .active()
            .cloned()
            .ok_or_else(|| AppError::Io("尚未配置任何后端".into()))?;
        (b, s.temperature, s.timeout_secs)
    };
    let key = config::load_secret(&backend.id);
    let lang = prompt::resolve_language(&target_lang)?;
    let reverse = lang == "中文"
        && terms
            .first()
            .map(|p| !p[0].chars().any(|c| (c as u32) >= 0x3000))
            .unwrap_or(false);
    let terminology = prompt::terminology_string(&terms, reverse);
    let concurrency = concurrency.unwrap_or(3).clamp(1, 8);

let mut results: Vec<(usize, String)> = Vec::new();
    let mut errors: Vec<(i32, String)> = Vec::new();

    // Own the translatable texts so worker tasks don't borrow `doc`.
    let work: Arc<Vec<(usize, String)>> = Arc::new(
        doc.segments
            .iter()
            .filter(|s| s.translatable)
            .map(|s| (s.index, s.text.clone()))
            .collect(),
    );
    let total = work.len();
    let next = Arc::new(tokio::sync::Mutex::new(0usize));
    let mut tasks = Vec::new();

    for _ in 0..concurrency {
        let work = work.clone();
        let next = next.clone();
        let backend = backend.clone();
        let key = key.clone();
        let terminology = terminology.clone();
        let lang = lang.clone();
        tasks.push(tokio::spawn(async move {
            let mut out: Vec<(usize, Result<String, String>)> = Vec::new();
            loop {
                let i = {
                    let mut n = next.lock().await;
                    if *n >= work.len() {
                        None
                    } else {
                        let v = *n;
                        *n += 1;
                        Some(v)
                    }
                };
                let Some(i) = i else { break };

                let (index, text) = &work[i];
                let p = prompt::build_prompt(
                    text,
                    &lang,
                    Mode::Basic,
                    &terminology,
                    "",
                    "",
                    "",
                );
                let req = TranslateRequest {
                    text: p,
                    target_lang: lang.clone(),
                    mode: Mode::Basic,
                    terms: vec![],
                    auto_filter_terms: false,
                    style: String::new(),
                    format_type: String::new(),
                    context: String::new(),
                    ignore_glossary: true,
                };
                let r = match client::translate(&backend, key.as_deref(), temperature, timeout, &req).await
                {
                    Ok(r) => Ok(r.text),
                    Err(e) => Err(e.to_string()),
                };
                out.push((*index, r));
            }
            out
        }));
    }

    for t in tasks {
        match t.await {
            Ok(items) => {
                for (index, r) in items {
                    match r {
                        Ok(text) => results.push((index, text)),
                        Err(e) => errors.push((index as i32, e)),
                    }
                }
            }
            Err(e) => errors.push((-1, e.to_string())),
        }
    }

    let out_path = document::output_path(&path, &target_lang);
    let content = document::reassemble(&doc.segments, &results);
    document::save(&out_path, &content)?;

    Ok(DocProgress {
        out_path,
        done: results.len(),
        total,
        failed: errors.len(),
        errors,
    })
}

#[derive(Serialize)]
pub struct DocProgress {
    pub out_path: String,
    pub done: usize,
    pub total: usize,
    pub failed: usize,
    pub errors: Vec<(i32, String)>,
}

// ---------- JSONL batch ----------

#[tauri::command]
async fn batch_translate(
    state: State<'_, AppState>,
    input: String,
    output: String,
    target_lang: String,
    text_field: Option<String>,
    concurrency: Option<usize>,
    resume: bool,
) -> CmdResult<BatchProgress> {
    let raw = std::fs::read_to_string(&input).map_err(|e| AppError::Io(format!("读取失败: {e}")))?;
    let lang = prompt::resolve_language(&target_lang)?;
    let field = text_field.unwrap_or_else(|| "text".to_string());

    // Resume: keep records already present in the output file.
    let mut done: Vec<serde_json::Value> = Vec::new();
    if resume && std::path::Path::new(&output).exists() {
        if let Ok(t) = std::fs::read_to_string(&output) {
            for line in t.lines() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                    done.push(v);
                }
            }
        }
    }

    let mut items: Vec<(usize, String)> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(text) = v.get(&field).and_then(|x| x.as_str()) {
            if !text.trim().is_empty() {
                items.push((i, text.to_string()));
            }
        }
    }
    let total = items.len();

    let (backend, temperature, timeout) = {
        let s = state.settings.lock().unwrap();
        let b = s
            .active()
            .cloned()
            .ok_or_else(|| AppError::Io("尚未配置任何后端".into()))?;
        (b, s.temperature, s.timeout_secs)
    };
    let key = config::load_secret(&backend.id);
    let concurrency = concurrency.unwrap_or(3).clamp(1, 16);
    let work = Arc::new(items);
    let next = Arc::new(tokio::sync::Mutex::new(0usize));
    let mut tasks = Vec::new();
    let mut failed = 0usize;

    for _ in 0..concurrency {
        let work = work.clone();
        let next = next.clone();
        let backend = backend.clone();
        let key = key.clone();
        let lang = lang.clone();
        tasks.push(tokio::spawn(async move {
            let mut out: Vec<(usize, Result<String, String>)> = Vec::new();
            loop {
                let i = {
                    let mut n = next.lock().await;
                    if *n >= work.len() {
                        None
                    } else {
                        let v = *n;
                        *n += 1;
                        Some(v)
                    }
                };
                let Some(i) = i else { break };

                let (idx, text) = &work[i];
                let p = prompt::build_prompt(text, &lang, Mode::Basic, "", "", "", "");
                let req = TranslateRequest {
                    text: p,
                    target_lang: lang.clone(),
                    mode: Mode::Basic,
                    terms: vec![],
                    auto_filter_terms: false,
                    style: String::new(),
                    format_type: String::new(),
                    context: String::new(),
                    ignore_glossary: true,
                };
                let r = match client::translate(&backend, key.as_deref(), temperature, timeout, &req).await
                {
                    Ok(r) => Ok(r.text),
                    Err(e) => Err(e.to_string()),
                };
                out.push((*idx, r));
            }
            out
        }));
    }

    let mut translated: Vec<(usize, String)> = Vec::new();
    for t in tasks {
        match t.await {
            Ok(items) => {
                for (idx, r) in items {
                    match r {
                        Ok(text) => translated.push((idx, text)),
                        Err(_) => failed += 1,
                    }
                }
            }
            Err(_) => failed += concurrency,
        }
    }

    let mut out = String::new();
    for v in done {
        out.push_str(&v.to_string());
        out.push('\n');
    }
    // Re-read the original records so untouched fields survive.
    for (line_no, text) in translated.iter() {
        if let Some(orig) = raw.lines().nth(*line_no) {
            if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(orig.trim()) {
                v["translation"] = serde_json::Value::String(text.clone());
                out.push_str(&v.to_string());
                out.push('\n');
            }
        }
    }
    std::fs::write(&output, out).map_err(|e| AppError::Io(format!("写入失败: {e}")))?;

    Ok(BatchProgress {
        total,
        done: translated.len(),
        failed,
        output,
    })
}

#[derive(Serialize)]
pub struct BatchProgress {
    pub total: usize,
    pub done: usize,
    pub failed: usize,
    pub output: String,
}

#[tauri::command]
fn read_text_file(path: String) -> CmdResult<String> {
    let bytes = std::fs::read(&path).map_err(|e| AppError::Io(format!("读取失败: {e}")))?;
    let (text, _) = document::decode(&bytes);
    Ok(text)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let settings = Settings::load();
    let glossary = Glossary::load();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // 状态必须在 Builder 上挂，不能等 setup：config 里声明的窗口是在
        // setup **之前**就创建并开始加载的（tauri 的 setup() 先遍历
        // app.windows 建窗，之后才回调用户的 setup 闭包）。popup / mini 虽然
        // visible:false，webview 照样建、照样加载 index.html，于是它们的
        // onMounted 会和 app.manage 抢跑，报
        // 「state not managed for field 'state' on command 'get_settings'」。
        // Settings::load() / Glossary::load() 只是读文件、不需要 AppHandle，
        // 所以在这里就能把状态备好，彻底没有抢跑的余地。
        .manage(AppState {
            settings: Mutex::new(settings),
            glossary: Mutex::new(glossary),
        })
        // 关闭事件挂在 Builder 上。状态由 Builder::manage 提供，比 setup 更早，
        // 这里其实一定能取到；仍然用 try_state 并保留兜底，避免这条路径一旦
        // 出问题就直接把进程杀掉、导致全局热键和托盘一起失效。
        .on_window_event(|win, event| {
            // 小窗是「唤出即用、用完就走」：失焦就藏起来，符合直觉，
            // 也省得用户非要按一次 Esc。
            if let tauri::WindowEvent::Focused(false) = event {
                if win.label() == "mini" {
                    // 钉住时不收起——用户可能在读译文或复制，误点别处不该把窗弄没。
                    if !tray::mini_pinned() {
                        let _ = win.hide();
                    }
                }
                return;
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // popup / mini 都是常驻窗口，关闭只隐藏。真销毁了就再也弹不
                // 出来，除非重启应用。
                if win.label() == "popup" || win.label() == "mini" {
                    api.prevent_close();
                    let _ = win.hide();
                    return;
                }
                let close_to_tray = win
                    .app_handle()
                    .try_state::<AppState>()
                    .map(|st| st.settings.lock().unwrap().close_to_tray)
                    .unwrap_or(true);
                if close_to_tray {
                    api.prevent_close();
                    let _ = win.hide();
                }
            }
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let snapshot = {
                let st = handle.state::<AppState>();
                // 显式绑定 guard：块尾表达式里的临时值会活到块末，比 st 还晚，
                // 那样会让借用活得比它依赖的 State 更久，编译不过。
                let guard = st.settings.lock().unwrap();
                let s = guard.clone();
                s
            };

            tray::build_tray(&handle)?;
            // 两个热键一起注册（共用一次 unregister_all）。结果走事件推给前端，
            // 但 setup 早于前端挂载 listen，那条事件必然被丢——所以界面挂载后
            // 会主动查一次 hotkey_status。
            tray::apply_hotkeys(&handle, &snapshot);

            // 划词翻译：开着才拉全局鼠标监听
            let (sel_on, sel_mod) = {
                let st = handle.state::<AppState>();
                let guard = st.settings.lock().unwrap();
                (guard.selection_enabled, guard.selection_modifier.clone())
            };
            selection::configure(&handle, sel_on, &sel_mod);
            // 上个进程若死在捕获窗口里，哨兵会留在用户剪贴板上，启动时清掉。
            selection::repair_clipboard();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_meta,
            get_settings,
            save_settings,
            set_hotkey,
            get_hotkey,
            hotkey_status,
            get_autostart,
            set_autostart,
            show_window,
            hide_mini,
            set_mini_pinned,
            selection_status,
            hide_selection_popup,
            take_pending_selection,
            set_popup_pinned,
            copy_text,
            read_clipboard_text,
            set_active_backend,
            set_api_key,
            test_backend,
            translate,
            translate_stream,
            preview_prompt,
            detect_language,
            glossary_get,
            glossary_set_terms,
            glossary_activate,
            glossary_create,
            glossary_rename,
            glossary_delete,
            glossary_export,
            glossary_import,
            history_list,
            history_count,
            history_set_starred,
            history_delete,
            history_clear,
            doc_load,
            doc_output_path,
            doc_save,
            doc_translate,
            batch_translate,
            read_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running application");
}