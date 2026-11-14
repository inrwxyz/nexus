use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::settings::{save_settings, AuraSettings};

fn get_allowed_origin(request: &str) -> Option<&'static str> {
    for line in request.lines() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("origin:") {
            let val = line[7..].trim();
            if val.starts_with("http://127.0.0.1:41780")
                || val.starts_with("http://localhost:41780")
                || val.starts_with("tauri://")
                || val.starts_with("https://tauri.localhost")
            {
                return Some("http://127.0.0.1:41780");
            }
            return None;
        }
    }
    Some("http://127.0.0.1:41780")
}

pub fn start_server(settings_state: Arc<Mutex<AuraSettings>>) {
    thread::spawn(move || {
        let listener = match TcpListener::bind("127.0.0.1:41780") {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Nexus Settings Server bind error: {}", e);
                return;
            }
        };

        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let state_clone = settings_state.clone();
                thread::spawn(move || {
                    let mut buffer = [0u8; 4096];
                    let bytes_read = match stream.read(&mut buffer) {
                        Ok(n) => n,
                        Err(_) => return,
                    };
                    let request = String::from_utf8_lossy(&buffer[..bytes_read]);

                    let allowed_origin = match get_allowed_origin(&request) {
                        Some(o) => o,
                        None => {
                            let _ = stream.write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 17\r\n\r\nOrigin not allowed");
                            return;
                        }
                    };

                    if request.starts_with("GET /api/settings") {
                        let current = state_clone.lock().unwrap().clone();
                        let json = serde_json::to_string(&current).unwrap_or_default();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {}\r\nContent-Length: {}\r\n\r\n{}",
                            allowed_origin,
                            json.len(),
                            json
                        );
                        let _ = stream.write_all(response.as_bytes());
                    } else if request.starts_with("POST /api/settings") {
                        if let Some(pos) = request.find("\r\n\r\n") {
                            let body = &request[pos + 4..];
                            if let Ok(mut new_settings) = serde_json::from_str::<AuraSettings>(body) {
                                save_settings(&mut new_settings);
                                *state_clone.lock().unwrap() = new_settings.clone();
                                crate::voice::set_listening_allowed(new_settings.module_should_run_now("voice"));
                                crate::modules::circle_search::sync_with_settings(&new_settings, true);
                                let resp = format!(
                                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: {}\r\nContent-Length: 15\r\n\r\n{{\"status\":\"ok\"}}",
                                    allowed_origin
                                );
                                let _ = stream.write_all(resp.as_bytes());
                            }
                        }
                    } else if request.starts_with("GET /lens-bridge") {
                        let bridge_path = std::env::temp_dir().join("nexus_lens_bridge.html");
                        if let Ok(content) = std::fs::read(&bridge_path) {
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\n\r\n",
                                content.len()
                            );
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.write_all(&content);
                        } else {
                            let _ = stream
                                .write_all(b"HTTP/1.1 404 NOT FOUND\r\n\r\nLens Bridge Missing");
                        }
                    } else {
                        // Serve static files
                        let (file_name, content_type) = if request.starts_with("GET /settings.css")
                        {
                            ("settings.css", "text/css")
                        } else if request.starts_with("GET /settings.js") {
                            ("settings.js", "application/javascript")
                        } else {
                            ("settings.html", "text/html")
                        };

                        let mut path = PathBuf::from("ui").join(file_name);
                        if !path.exists() {
                            if let Ok(mut exe) = std::env::current_exe() {
                                exe.pop();
                                path = exe.join("ui").join(file_name);
                            }
                        }

                        if let Ok(content) = std::fs::read(&path) {
                            let response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\n\r\n",
                                content_type,
                                content.len()
                            );
                            let _ = stream.write_all(response.as_bytes());
                            let _ = stream.write_all(&content);
                        } else {
                            let _ =
                                stream.write_all(b"HTTP/1.1 404 NOT FOUND\r\n\r\nFile Not Found");
                        }
                    }
                });
            }
        }
    });
}
