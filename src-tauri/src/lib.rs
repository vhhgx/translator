//! Hy-MT2 跨端翻译工具 — Tauri backend.
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
fn save_settings(state: State<'_, AppState>, mut s: Settings) -> CmdResult<()> {
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
    *state.settings.lock().unwrap() = s;
    Ok(())
}

// ---------- 托盘 / 全局热键 / 开机自启 ----------

/// 热键换绑。注册失败时返回错误文本，界面必须显示出来——Windows 上
/// RegisterHotKey 冲突是静默失败的，不提示用户会以为功能没生效。
#[tauri::command]
fn set_hotkey(app: tauri::AppHandle, state: State<'_, AppState>, accel: String) -> CmdResult<String> {
    let accel = accel.trim().to_string();
    if accel.is_empty() {
        // 空字符串表示不要全局热键
        let _ = app.global_shortcut().unregister_all();
        let mut s = state.settings.lock().unwrap();
        s.hotkey_toggle = String::new();
        s.save()?;
        return Ok(String::new());
    }
    match tray::rebind_hotkey(&app, &accel) {
        Ok(()) => {
            let mut s = state.settings.lock().unwrap();
            s.hotkey_toggle = accel.clone();
            s.save()?;
            Ok(accel)
        }
        Err(e) => Err(AppError::Io(e)),
    }
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
pub struct HotkeyStatus {
    pub accel: String,
    pub registered: bool,
}

#[tauri::command]
fn hotkey_status(app: tauri::AppHandle, state: State<'_, AppState>) -> HotkeyStatus {
    let accel = state.settings.lock().unwrap().hotkey_toggle.clone();
    let registered = if accel.trim().is_empty() {
        false
    } else {
        accel
            .parse::<tauri_plugin_global_shortcut::Shortcut>()
            .map(|sc| app.global_shortcut().is_registered(sc))
            .unwrap_or(false)
    };
    HotkeyStatus { accel, registered }
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
        let _ = with_history(|h| {
            h.add(&history::HistoryItem {
                id: uuid::Uuid::new_v4().to_string(),
                created_at: now_millis(),
                source: req.text.clone(),
                translation: resp.text.clone(),
                source_lang: "auto".into(),
                target_lang: resp.target_lang.clone(),
                backend_id: backend.id.clone(),
                mode: mode_key(req.mode).to_string(),
            })
        });
    }
    Ok(resp)
}

/// Streaming translation.
///
/// Returns immediately; tokens are pushed to the webview as events:
///   - `hymt2:delta`  { job_id, text }   one or more times
///   - `hymt2:done`   { job_id, text, target_lang, ms, used_terms }
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
                let _ = app.emit(
                    "hymt2:done",
                    serde_json::json!({
                        "job_id": job_id,
                        "text": text,
                        "target_lang": target_lang,
                        "used_terms": used_terms,
                        "ms": started.elapsed().as_millis(),
                    }),
                );
                // Persist to history once the stream finishes.
                let hist = HISTORY.lock().unwrap();
                if let Some(h) = hist.as_ref() {
                    let _ = h.add(&history::HistoryItem {
                        id: uuid::Uuid::new_v4().to_string(),
                        created_at: now_millis(),
                        source: req.text.clone(),
                        translation: text,
                        source_lang: "auto".into(),
                        target_lang,
                        backend_id: backend.id.clone(),
                        mode: mode_key(req.mode).to_string(),
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
    pub terms: Vec<[String; 2]>,
}

#[tauri::command]
fn glossary_get(state: State<'_, AppState>) -> GlossaryView {
    let g = state.glossary.lock().unwrap();
    GlossaryView {
        terms: g.terms(&g.active),
        names: g.names(),
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
    limit: Option<i64>,
) -> CmdResult<Vec<history::HistoryItem>> {
    with_history(|h| h.list(search.as_deref(), limit.unwrap_or(100)))
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
        // 关闭事件挂在 Builder 上。状态在 setup 里才 manage，所以运行时再取，
        // 取不到就按「隐藏到托盘」处理，避免关窗直接杀掉进程导致热键失效。
        .on_window_event(|win, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
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
            app.manage(AppState {
                settings: Mutex::new(settings),
                glossary: Mutex::new(glossary),
            });

            let handle = app.handle().clone();
            let hotkey = {
                let st = handle.state::<AppState>();
                let guard = st.settings.lock().unwrap();
                // 显式绑定 guard：块尾表达式里的临时值会活到块末，比 st 还晚，
                // 那样会让借用活得比它依赖的 State 更久，编译不过。
                let hk = guard.hotkey_toggle.clone();
                hk
            };

            tray::build_tray(&handle)?;
            match tray::rebind_hotkey(&handle, &hotkey) {
                Ok(()) => tray::notify_hotkey(&handle, true, format!("全局热键已注册：{hotkey}")),
                Err(e) => tray::notify_hotkey(&handle, false, e),
            }

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
            set_active_backend,
            set_api_key,
            test_backend,
            translate,
            translate_stream,
            preview_prompt,
            glossary_get,
            glossary_set_terms,
            glossary_activate,
            glossary_create,
            glossary_rename,
            glossary_delete,
            glossary_export,
            glossary_import,
            history_list,
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