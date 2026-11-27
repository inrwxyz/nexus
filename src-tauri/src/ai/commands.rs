//! Tauri UI commands for the Voice -> AI & Keys subsection.
//! Heavy HTTP / Win32 / Ollama work runs inside `spawn_blocking` so the UI never freezes.

use super::{local_llm, providers, terminal_guard, tts, vault};
use crate::app_launcher::scan_apps;
use crate::settings::AuraSettings;
use crate::vocab::{load_aliases, load_vocab};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

#[tauri::command]
pub fn ai_list_keys() -> Vec<vault::MaskedKeyInfo> {
    vault::list_keys_masked()
}

#[tauri::command]
pub fn ai_add_key(
    label: String,
    provider: String,
    model: String,
    api_key: String,
) -> Result<vault::MaskedKeyInfo, String> {
    vault::add_key(&label, &provider, &model, &api_key)
}

#[tauri::command]
pub fn ai_remove_key(id: String) -> Result<bool, String> {
    vault::remove_key(&id)
}

#[tauri::command]
pub fn ai_set_key_enabled(id: String, enabled: bool) -> Result<bool, String> {
    vault::set_key_enabled(&id, enabled)
}

#[tauri::command]
pub fn ai_update_key_model(id: String, model: String) -> Result<bool, String> {
    vault::update_key_model(&id, &model)
}

#[tauri::command]
pub async fn ai_test_key(id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || providers::test_single_key(&id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ai_local_status() -> Result<local_llm::LocalLlmStatus, String> {
    tauri::async_runtime::spawn_blocking(local_llm::get_status)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_install_local_llm(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || local_llm::install_ollama(&app))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ai_pull_local_model(app: AppHandle, model: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || local_llm::pull_model(&app, &model))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn ai_ask_text(
    prompt: String,
    state: tauri::State<'_, Arc<Mutex<AuraSettings>>>,
    app: AppHandle,
) -> Result<Value, String> {
    let clean = prompt.trim().to_string();
    if clean.is_empty() {
        return Err("Prompt is empty".to_string());
    }
    let (ai_cfg, island_enabled) = {
        let s = state.lock().unwrap();
        (s.ai.clone(), s.show_on_island)
    };
    if !ai_cfg.enabled {
        return Err("Turn on AI Assistant first to ask questions".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let vocab = load_vocab();
        let mut aliases = load_aliases();
        let mut apps = scan_apps();
        let out = super::ask_ai(
            &ai_cfg,
            &clean,
            false,
            &vocab,
            &mut apps,
            &mut aliases,
            None,
            island_enabled,
            false,
            &app,
        )?;
        Ok(json!({
            "reply": out.reply,
            "meta": out.meta,
            "executed_cmd": out.executed_cmd,
            "proposal": out.proposal
        }))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn ai_clear_chat() -> bool {
    super::clear_conversation();
    tts::stop();
    true
}

#[tauri::command]
pub fn ai_list_proposals() -> Vec<terminal_guard::PendingTerminalProposal> {
    terminal_guard::list_proposals()
}

#[tauri::command]
pub async fn ai_approve_proposal(
    id: String,
    state: tauri::State<'_, Arc<Mutex<AuraSettings>>>,
) -> Result<String, String> {
    let allowed = state.lock().unwrap().ai.allow_ai_terminal;
    tauri::async_runtime::spawn_blocking(move || terminal_guard::approve_and_run(&id, allowed))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn ai_dismiss_proposal(id: String) -> bool {
    terminal_guard::dismiss_proposal(&id);
    true
}
