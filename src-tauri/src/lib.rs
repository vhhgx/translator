//! Hy-MT2 跨端翻译工具 — Tauri backend.
//!
//! Rust owns config, credentials, HTTP and document IO; the Vue layer is
//! presentation only.

mod client;
mod config;
mod document;
mod error;
mod glossary;
mod history;
mod prompt;

use std::sync::Mutex;

use serde::Serialize;
use tauri::{Manager, State};

use client::{TranslateRequest, TranslateResponse};
use config::{Backend, Settings};
use error::{AppError, AppResult};
use glossary::Glossary;
use history::History;
use prompt::Mode;

pub struct AppState {
    settings: Mutex<Settings>,
    glossary: Mutex<Glossary>,
    history: Mutex<Option<History>>,
}

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
        let hist = state.history.lock().unwrap();
        if let Some(h) = hist.as_ref() {
            let _ = h.add(&history::HistoryItem {
                id: uuid::Uuid::new_v4().to_string(),
                created_at: now_millis(),
                source: req.text.clone(),
                translation: resp.text.clone(),
                source_lang: "auto".into(),
                target_lang: resp.target_lang.clone(),
                backend_id: backend.id.clone(),
                mode: mode_key(req.mode).to_string(),
            });
        }
    }
    Ok(resp)
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

fn with_history<T>(
    state: &State<'_, AppState>,
    f: impl FnOnce(&History) -> AppResult<T>,
) -> CmdResult<T> {
    let mut guard = state.history.lock().unwrap();
    if guard.is_none() {
        *guard = Some(History::open()?);
    }
    f(guard.as_ref().unwrap())
}

#[tauri::command]
fn history_list(
    state: State<'_, AppState>,
    search: Option<String>,
    limit: Option<i64>,
) -> CmdResult<Vec<history::HistoryItem>> {
    with_history(&state, |h| h.list(search.as_deref(), limit.unwrap_or(100)))
}

#[tauri::command]
fn history_delete(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_history(&state, |h| h.delete(&id))
}

#[tauri::command]
fn history_clear(state: State<'_, AppState>) -> CmdResult<()> {
    with_history(&state, |h| h.clear())
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
fn doc_translate(
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
    let mut errors: Vec<(usize, String)> = Vec::new();
    let total = doc.segments.iter().filter(|s| s.translatable).count();
    let queue = std::sync::Arc::new(tokio::sync::Mutex::new(
        doc.segments
            .iter()
            .filter(|s| s.translatable)
            .map(|s| s.index)
            .collect::<Vec<_>>(),
    ));
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut tasks = Vec::new();

    for _ in 0..concurrency {
        let queue = queue.clone();
        let sem = semaphore.clone();
        let backend = backend.clone();
        let key = key.clone();
        let terminology = terminology.clone();
        let lang = lang.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                let Some(index) = {
                    let mut q = queue.lock().await;
                    if q.is_empty() {
                        None
                    } else {
                        Some(q.remove(0))
                    }
                } else {
                    break;
                };
                let _permit = sem.acquire().await.ok();
                let text = doc
                    .segments
                    .iter()
                    .find(|s| s.index == index)
                    .map(|s| s.text.clone())
                    .unwrap_or_default();
                let p = prompt::build_prompt(
                    &text,
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
                match client::translate(&backend, key.as_deref(), temperature, timeout, &req).await {
                    Ok(r) => (index, Ok(r.text)),
                    Err(e) => (index, Err(e.to_string())),
                }
            }
        }));
    }

    for t in tasks {
        match t.await {
            Ok((index, Ok(text))) => results.push((index, text)),
            Ok((index, Err(e))) => errors.push((index, e)),
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

    let mut items: Vec<(usize, serde_json::Value)> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let Some(text) = v.get(&field).and_then(|x| x.as_str()).map(|s| s.to_string()) else {
            continue;
        };
        if !text.trim().is_empty() {
            items.push((i, v));
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
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let mut tasks = Vec::new();
    let mut results: Vec<(usize, serde_json::Value)> = Vec::new();
    let mut failed = 0usize;

    for (idx, value) in items {
        let Some(text) = value.get(&field).and_then(|x| x.as_str()).map(|s| s.to_string()) else {
            continue;
        };
        let backend = backend.clone();
        let key = key.clone();
        let lang = lang.clone();
        let sem = sem.clone();
        tasks.push(tokio::spawn(async move {
            let _permit = sem.acquire().await.ok();
            let p = prompt::build_prompt(&text, &lang, Mode::Basic, "", "", "", "");
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
            let translated =
                client::translate(&backend, key.as_deref(), temperature, timeout, &req).await;
            (idx, translated)
        }));
    }

    for t in tasks {
        match t.await {
            Ok((idx, Ok(r))) => {
                let (_, mut v) = items
                    .iter()
                    .find(|(i, _)| *i == idx)
                    .cloned()
                    .map(|(i, v)| (i, v))
                    .unwrap_or((0, serde_json::json!({})));
                v["translation"] = serde_json::Value::String(r.text);
                results.push((idx, v));
            }
            _ => failed += 1,
        }
    }

    results.sort_by_key(|(i, _)| *i);
    let mut out = String::new();
    for v in done {
        out.push_str(&v.to_string());
        out.push('\n');
    }
    for (_, v) in &results {
        out.push_str(&v.to_string());
        out.push('\n');
    }
    std::fs::write(&output, out).map_err(|e| AppError::Io(format!("写入失败: {e}")))?;

    Ok(BatchProgress {
        total,
        done: results.len(),
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
    let history = History::open().ok();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            app.manage(AppState {
                settings: Mutex::new(settings.clone()),
                glossary: Mutex::new(glossary.clone()),
                history: Mutex::new(history.clone()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_meta,
            get_settings,
            save_settings,
            set_active_backend,
            set_api_key,
            test_backend,
            translate,
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