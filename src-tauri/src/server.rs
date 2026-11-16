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
        let listener = loop {
            match TcpListener::bind("0.0.0.0:41780") {
                Ok(sock) => {
                    println!("✅ Nexus Server listening on 0.0.0.0:41780");
                    break sock;
                }
                Err(_e) => {
                    thread::sleep(std::time::Duration::from_millis(1000));
                }
            }
        };

        for stream in listener.incoming() {
            if let Ok(mut stream) = stream {
                let state_clone = settings_state.clone();
                thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .ok();

                    let mut raw_buf = Vec::with_capacity(4096);
                    let mut chunk = [0u8; 4096];
                    let mut content_length: Option<usize> = None;
                    let mut header_end: Option<usize> = None;

                    loop {
                        let n = match stream.read(&mut chunk) {
                            Ok(0) => break,
                            Ok(n) => n,
                            Err(_) => break,
                        };
                        raw_buf.extend_from_slice(&chunk[..n]);

                        if header_end.is_none() {
                            if let Some(pos) = raw_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                header_end = Some(pos + 4);
                                let head_str = String::from_utf8_lossy(&raw_buf[..pos]);
                                for line in head_str.lines() {
                                    let lower = line.to_ascii_lowercase();
                                    if let Some(val) = lower.strip_prefix("content-length:") {
                                        if let Ok(len) = val.trim().parse::<usize>() {
                                            content_length = Some(len);
                                        }
                                    }
                                }
                            }
                        }

                        if let Some(hend) = header_end {
                            let expected = hend + content_length.unwrap_or(0);
                            if raw_buf.len() >= expected {
                                break;
                            }
                        }

                        if raw_buf.len() > 65536 {
                            break;
                        }
                    }

                    let request = String::from_utf8_lossy(&raw_buf);

                    // Handle CORS Preflight for Companion / Web API
                    if request.starts_with("OPTIONS ") {
                        let resp = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nAccess-Control-Max-Age: 86400\r\nContent-Length: 0\r\n\r\n";
                        let _ = stream.write_all(resp.as_bytes());
                        return;
                    }

                    // ── Phone Link API: everything under /api/companion/* lives in companion.rs ───
                    let from_local = stream
                        .peer_addr()
                        .map(|a| a.ip().is_loopback())
                        .unwrap_or(false);
                    if crate::companion::handle(&request, &mut stream, &state_clone, from_local) {
                        return;
                    }

                    // Settings API / UI are for this PC only. A phone or any LAN device must not read or change them
                    // (the settings JSON contains the phone's auth token).
                    if !from_local {
                        let _ = stream.write_all(
                            b"HTTP/1.1 403 Forbidden\r\nContent-Length: 15\r\nConnection: close\r\n\r\nLocal only page",
                        );
                        return;
                    }

                    // ── Original Settings & Static Web API ───
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
