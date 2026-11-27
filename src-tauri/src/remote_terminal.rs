//! Opt-in Remote PowerShell terminal endpoint (`/api/companion/terminal`).
//! Disabled by default (`companion.remote_terminal = false`) and strictly gated by token + setting.

use std::io::Write;
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

use crate::settings::AuraSettings;

const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const CMD_TIMEOUT: Duration = Duration::from_secs(15);

fn respond_json(stream: &mut TcpStream, code: u16, text: &str, body: serde_json::Value) {
    let b = body.to_string();
    let resp = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        code,
        text,
        b.len(),
        b
    );
    let _ = stream.write_all(resp.as_bytes());
}

fn header_val<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    let h = head.split("\r\n\r\n").next().unwrap_or(head);
    for line in h.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim());
            }
        }
    }
    None
}

fn bearer(head: &str) -> String {
    let Some(v) = header_val(head, "authorization") else {
        return String::new();
    };
    if v.get(..7).map(|p| p.eq_ignore_ascii_case("bearer ")).unwrap_or(false) {
        v[7..].trim().to_string()
    } else {
        String::new()
    }
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n...[output truncated]", &s[..end])
}

pub fn run_powershell(cmd: &str) -> (i32, String, bool) {
    let mut command = Command::new("powershell");
    command
        .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    #[cfg(target_os = "windows")]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let Ok(mut child) = command.spawn() else {
        return (-1, "Failed to start PowerShell".into(), false);
    };

    let start = Instant::now();
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if start.elapsed() >= CMD_TIMEOUT {
                    timed_out = true;
                    let _ = child.kill();
                    break;
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            Err(_) => break,
        }
    }

    match child.wait_with_output() {
        Ok(out) => {
            let code = out.status.code().unwrap_or(if timed_out { -1 } else { 0 });
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let mut combined = String::new();
            if !stdout.trim().is_empty() {
                combined.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(stderr.trim_end());
            }
            if timed_out {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str("[Timed out after 15s]");
            }
            (code, truncate_utf8(&combined, MAX_OUTPUT_BYTES), timed_out)
        }
        Err(e) => (-1, format!("Command execution failed: {e}"), timed_out),
    }
}

pub fn handle_http(
    head: &str,
    body: &[u8],
    stream: &mut TcpStream,
    state: &Arc<Mutex<AuraSettings>>,
) {
    let tok = bearer(head);
    let (token_ok, companion_on, term_on) = {
        let s = state.lock().unwrap_or_else(|e| e.into_inner());
        (
            !tok.is_empty() && s.companion.auth_token.as_deref() == Some(tok.as_str()),
            s.module_enabled("companion"),
            s.companion.remote_terminal,
        )
    };

    if !token_ok {
        return respond_json(stream, 401, "Unauthorized", serde_json::json!({ "status": "error", "message": "Unauthorized" }));
    }
    if !companion_on || !term_on {
        return respond_json(
            stream,
            403,
            "Forbidden",
            serde_json::json!({ "status": "error", "message": "Remote Terminal is disabled in Nexus PC settings" }),
        );
    }

    #[derive(serde::Deserialize)]
    struct Req {
        command: Option<String>,
    }

    let req: Req = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(_) => return respond_json(stream, 400, "Bad Request", serde_json::json!({ "status": "error", "message": "Malformed terminal payload" })),
    };

    let cmd = req.command.unwrap_or_default().trim().to_string();
    if cmd.is_empty() {
        return respond_json(stream, 400, "Bad Request", serde_json::json!({ "status": "error", "message": "Empty command" }));
    }

    println!("💻 Companion Remote Terminal: {}", cmd);
    let (exit_code, output, timed_out) = run_powershell(&cmd);
    respond_json(
        stream,
        200,
        "OK",
        serde_json::json!({
            "status": "ok",
            "exit_code": exit_code,
            "output": output,
            "timed_out": timed_out,
        }),
    );
}
