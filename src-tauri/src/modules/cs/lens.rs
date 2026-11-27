//! Visual search (Google Lens) & Google Translate Images clipboard bridge helpers.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use tauri::AppHandle;

use super::{
    browser, capture, crop_or_full, hide_and_destroy_overlay, url_encode_query, VisualSearchResult,
};

pub(crate) fn build_lens_bridge_html(png_bytes: &[u8]) -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>Nexus — Google Lens</title>
  <style>
    * {{ box-sizing: border-box; margin: 0; padding: 0; }}
    html, body {{
      width: 100%; height: 100%; background: #0b0e14; color: #e2e8f0;
      font-family: 'Segoe UI Variable', 'Segoe UI', system-ui, sans-serif;
      display: grid; place-items: center;
    }}
    .loader {{ display: flex; flex-direction: column; align-items: center; gap: 14px; }}
    .ring {{
      width: 36px; height: 36px; border-radius: 50%;
      border: 3px solid rgba(6, 182, 212, 0.22);
      border-top-color: #22d3ee;
      animation: spin .85s linear infinite;
    }}
    .msg {{ font-size: 13.5px; font-weight: 500; color: #94a3b8; letter-spacing: .01em; }}
    @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
    form {{ display: none; }}
  </style>
</head>
<body>
  <div class="loader">
    <div class="ring"></div>
    <div class="msg">Opening Google Lens…</div>
  </div>
  <form id="lensForm" action="https://lens.google.com/v3/upload?hl=en&ep=ccm&re=df&s=4&st={ts}" method="POST" enctype="multipart/form-data">
    <input type="file" name="encoded_image" id="fileInput" />
  </form>
  <script>
    (function () {{
      var b64 = "{b64}";
      var bin = atob(b64);
      var bytes = new Uint8Array(bin.length);
      for (var i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
      var blob = new Blob([bytes], {{ type: 'image/png' }});
      var file = new File([blob], 'capture.png', {{ type: 'image/png' }});
      var dt = new DataTransfer();
      dt.items.add(file);
      document.getElementById('fileInput').files = dt.files;
      document.getElementById('lensForm').submit();
    }})();
  </script>
</body>
</html>"#
    )
}

/// Sends Ctrl+V to paste the clipboard image into Lens / Translate Web.
fn send_ctrl_v() {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    };
    let key = |vk: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let mut inputs = [key(0x11, false), key(0x56, false), key(0x56, true), key(0x11, true)];
    unsafe {
        SendInput(4, inputs.as_mut_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}

fn open_browser_and_paste_clipboard(
    url: &str,
    clip_seq: Option<u32>,
    temp_file: Option<std::path::PathBuf>,
) -> Result<(), String> {
    browser::open_url(url)?;
    std::thread::spawn(move || {
        if let Some(seq) = clip_seq {
            std::thread::sleep(Duration::from_millis(3200));
            send_ctrl_v();
            std::thread::sleep(Duration::from_millis(1200));
            capture::clear_clipboard_if_sequence(seq);
        } else if temp_file.is_some() {
            std::thread::sleep(Duration::from_millis(10000));
        }
        if let Some(path) = temp_file {
            let _ = std::fs::remove_file(path);
        }
    });
    Ok(())
}

pub(crate) async fn run_lens_search_impl(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<VisualSearchResult, String> {
    let (_, _, cw, ch, bgra) = crop_or_full(x, y, width, height)?;

    let (clip_seq, bridge_path) = tauri::async_runtime::spawn_blocking(move || {
        let clip_seq = capture::copy_bgra_to_clipboard(cw, ch, &bgra);
        let bridge_path = if clip_seq.is_none() {
            let png = capture::encode_png(cw, ch, &bgra)?;
            let html = build_lens_bridge_html(&png);
            let path = std::env::temp_dir().join("nexus_lens_bridge.html");
            std::fs::write(&path, html.as_bytes())
                .map_err(|e| format!("Failed to write nexus_lens_bridge.html: {e}"))?;
            Some(path)
        } else {
            None
        };
        Ok::<(Option<u32>, Option<std::path::PathBuf>), String>((clip_seq, bridge_path))
    })
    .await
    .map_err(|e| format!("Bridge build worker failed: {e}"))??;

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    hide_and_destroy_overlay(&app);

    let clip_ok = clip_seq.is_some();
    let msg = if clip_ok {
        open_browser_and_paste_clipboard("https://lens.google.com/", clip_seq, bridge_path.clone())?;
        "Opened Google Lens; pasting captured image".to_string()
    } else {
        open_browser_and_paste_clipboard(
            &format!("http://127.0.0.1:41780/lens-bridge?t={ts}"),
            None,
            bridge_path.clone(),
        )?;
        "Opened Google Lens bridge in default browser".to_string()
    };

    Ok(VisualSearchResult {
        ok: true,
        opened_direct: true,
        clipboard_ready: clip_ok,
        url: bridge_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        message: msg,
    })
}

pub(crate) async fn google_translate_image_impl(
    app: AppHandle,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    target_lang: String,
) -> Result<VisualSearchResult, String> {
    let tl = if target_lang.trim().is_empty() {
        "bn".to_string()
    } else {
        target_lang.trim().to_string()
    };
    let (_, _, cw, ch, bgra) = crop_or_full(x, y, width, height)?;

    let (clip_seq, png_path) = tauri::async_runtime::spawn_blocking(move || {
        let clip_seq = capture::copy_bgra_to_clipboard(cw, ch, &bgra);
        let path = if clip_seq.is_none() {
            let png = capture::encode_png(cw, ch, &bgra)?;
            let p = std::env::temp_dir().join("nexus_translate_image.png");
            std::fs::write(&p, &png)
                .map_err(|e| format!("Failed to write nexus_translate_image.png: {e}"))?;
            Some(p)
        } else {
            None
        };
        Ok::<(Option<u32>, Option<std::path::PathBuf>), String>((clip_seq, path))
    })
    .await
    .map_err(|e| format!("Image prepare worker failed: {e}"))??;

    let clip_ok = clip_seq.is_some();
    let url = format!(
        "https://translate.google.com/?sl=auto&tl={}&op=images",
        url_encode_query(&tl)
    );
    hide_and_destroy_overlay(&app);
    open_browser_and_paste_clipboard(&url, clip_seq, png_path.clone())?;

    Ok(VisualSearchResult {
        ok: true,
        opened_direct: true,
        clipboard_ready: clip_ok,
        url: png_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        message: if clip_ok {
            "Opened Google Translate Images; pasting & clearing clipboard".to_string()
        } else {
            "Opened Google Translate Images; upload saved capture".to_string()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_self_submitting_lens_html_bridge() {
        let html = build_lens_bridge_html(&[0x89, b'P', b'N', b'G']);
        assert!(html.contains("https://lens.google.com/v3/upload?hl=en"));
        assert!(html.contains(r#"enctype="multipart/form-data""#));
        assert!(html.contains(r#"name="encoded_image""#));
        assert!(html.contains("new DataTransfer()"));
        assert!(html.contains("document.getElementById('lensForm').submit()"));
        assert!(html.contains("iVBORw=="));
    }
}
