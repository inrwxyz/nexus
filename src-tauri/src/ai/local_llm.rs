//! On-demand Local LLM setup manager (zero bundled model weights inside Nexus).
//! Checks local Ollama installation, lists models, installs Ollama or pulls a lightweight model strictly when requested.

use serde::Serialize;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct LocalLlmStatus {
    pub ollama_installed: bool,
    pub running: bool,
    pub models: Vec<String>,
}

fn ollama_exe_path() -> Option<PathBuf> {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let p = PathBuf::from(local).join("Programs").join("Ollama").join("ollama.exe");
        if p.exists() {
            return Some(p);
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(out) = std::process::Command::new("where")
            .arg("ollama")
            .creation_flags(0x0800_0000)
            .output()
        {
            if out.status.success() {
                let first = String::from_utf8_lossy(&out.stdout).lines().next()?.trim().to_string();
                if !first.is_empty() {
                    return Some(PathBuf::from(first));
                }
            }
        }
    }
    None
}

pub fn get_status() -> LocalLlmStatus {
    let exe = ollama_exe_path();
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build();
    if let Ok(resp) = agent.get("http://127.0.0.1:11434/api/tags").call() {
        let mut models = Vec::new();
        if let Ok(txt) = resp.into_string() {
            if let Ok(v) = serde_json::from_str::<Value>(&txt) {
                if let Some(arr) = v.get("models").and_then(|m| m.as_array()) {
                    for item in arr {
                        if let Some(name) = item.get("name").and_then(|n| n.as_str()) {
                            models.push(name.to_string());
                        }
                    }
                }
            }
        }
        return LocalLlmStatus {
            ollama_installed: true,
            running: true,
            models,
        };
    }

    LocalLlmStatus {
        ollama_installed: exe.is_some(),
        running: false,
        models: Vec::new(),
    }
}

pub fn ensure_ollama_running() -> Result<(), String> {
    if get_status().running {
        return Ok(());
    }
    let exe = ollama_exe_path().ok_or_else(|| "Ollama is not installed yet".to_string())?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new(exe)
            .arg("serve")
            .creation_flags(0x0800_0000)
            .spawn()
            .map_err(|e| format!("Failed to start Ollama: {e}"))?;
    }
    for _ in 0..16 {
        std::thread::sleep(Duration::from_millis(400));
        if get_status().running {
            return Ok(());
        }
    }
    Err("Ollama started but did not respond on port 11434 yet".to_string())
}

pub fn install_ollama(app: &AppHandle) -> Result<String, String> {
    if ollama_exe_path().is_some() {
        ensure_ollama_running()?;
        return Ok("Ollama is already installed and ready".to_string());
    }

    let _ = app.emit(
        "ai-local-progress",
        json!({ "stage": "install", "message": "Installing Ollama via Windows Package Manager..." }),
    );

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("winget")
            .args([
                "install",
                "-e",
                "--id",
                "Ollama.Ollama",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ])
            .creation_flags(0x0800_0000)
            .output()
            .map_err(|e| format!("winget install failed: {e}"))?;

        if !out.status.success() && ollama_exe_path().is_none() {
            let err_txt = String::from_utf8_lossy(&out.stderr);
            return Err(format!("Ollama install did not complete: {}", err_txt.trim()));
        }
    }

    let _ = ensure_ollama_running();
    let _ = app.emit(
        "ai-local-progress",
        json!({ "stage": "done", "message": "Ollama installed cleanly" }),
    );
    Ok("Ollama installed and started".to_string())
}

pub fn pull_model(app: &AppHandle, model: &str) -> Result<String, String> {
    let clean = model.trim();
    if clean.is_empty() {
        return Err("Model name is empty".to_string());
    }
    ensure_ollama_running()?;

    let _ = app.emit(
        "ai-local-progress",
        json!({ "stage": "pull", "model": clean, "message": format!("Downloading {clean}...") }),
    );

    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(600)).build();
    let resp = agent
        .post("http://127.0.0.1:11434/api/pull")
        .set("Content-Type", "application/json")
        .send_string(&json!({ "name": clean, "stream": true }).to_string())
        .map_err(|e| format!("Failed pulling {clean}: {e}"))?;

    let reader = BufReader::new(resp.into_reader());
    for line in reader.lines().map_while(Result::ok) {
        if let Ok(v) = serde_json::from_str::<Value>(&line) {
            if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
                return Err(err.to_string());
            }
            let status = v.get("status").and_then(|x| x.as_str()).unwrap_or("Downloading");
            let total = v.get("total").and_then(|x| x.as_u64()).unwrap_or(0);
            let completed = v.get("completed").and_then(|x| x.as_u64()).unwrap_or(0);
            let pct = completed.saturating_mul(100).checked_div(total).unwrap_or(0).min(100);
            let _ = app.emit(
                "ai-local-progress",
                json!({
                    "stage": "pull",
                    "model": clean,
                    "pct": pct,
                    "message": if pct > 0 { format!("{status} ({pct}%)") } else { status.to_string() }
                }),
            );
        }
    }

    let _ = app.emit(
        "ai-local-progress",
        json!({ "stage": "done", "model": clean, "pct": 100, "message": format!("{clean} ready!") }),
    );
    Ok(format!("Model {clean} is ready"))
}
