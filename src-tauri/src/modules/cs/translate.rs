// cs/translate.rs — Google Translate API client with fallback endpoints and newline-batching.

use std::sync::OnceLock;
use std::time::Duration;

const TRANSLATE_USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match *b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char);
            }
            b' ' => out.push('+'),
            other => {
                out.push('%');
                out.push_str(&format!("{other:02X}"));
            }
        }
    }
    out
}

pub fn translate_http_client() -> Result<&'static reqwest::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(TRANSLATE_USER_AGENT)
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(12))
                .pool_max_idle_per_host(6)
                .build()
                .map_err(|e| format!("HTTP client error: {e}"))
        })
        .as_ref()
        .map_err(|e| e.clone())
}

pub fn build_translate_urls(text: &str, tl: &str) -> [String; 3] {
    let tl_enc = url_encode(tl);
    let q_enc = url_encode(text);
    [
        format!(
            "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={tl_enc}&dt=t&q={q_enc}"
        ),
        format!(
            "https://translate.google.com/translate_a/single?client=gtx&sl=auto&tl={tl_enc}&dt=t&q={q_enc}"
        ),
        format!(
            "https://clients5.google.com/translate_a/t?client=dict-chrome-ex&sl=auto&tl={tl_enc}&q={q_enc}"
        ),
    ]
}

/// Parses both `translate_a/single` (`client=gtx`) and `clients5.google.com/translate_a/t`
/// (`client=dict-chrome-ex`) JSON payloads into `(translated_text, detected_lang)`.
pub fn parse_google_translate_json(val: &serde_json::Value) -> Result<(String, String), String> {
    let outer = val
        .as_array()
        .ok_or_else(|| "Unexpected translation response format".to_string())?;

    let mut translated = String::new();
    let mut detected = String::from("auto");

    if outer
        .first()
        .is_some_and(|v| v.as_array().is_some_and(|arr| arr.is_empty() || arr[0].is_array()))
    {
        if let Some(segs) = outer.first().and_then(|v| v.as_array()) {
            for seg in segs {
                if let Some(piece) = seg.get(0).and_then(|p| p.as_str()) {
                    translated.push_str(piece);
                }
            }
        }
        if let Some(lang) = outer.get(2).and_then(|v| v.as_str()) {
            if !lang.trim().is_empty() {
                detected = lang.trim().to_string();
            }
        }
    } else {
        for item in outer {
            if let Some(pair) = item.as_array() {
                if let Some(piece) = pair.first().and_then(|p| p.as_str()) {
                    translated.push_str(piece);
                }
                if detected == "auto" {
                    if let Some(lang) = pair.get(1).and_then(|v| v.as_str()) {
                        if !lang.trim().is_empty() {
                            detected = lang.trim().to_string();
                        }
                    }
                }
            } else if let Some(piece) = item.as_str() {
                translated.push_str(piece);
            }
        }
    }

    let trimmed = translated.trim().to_string();
    if trimmed.is_empty() {
        return Err("Empty translation response".to_string());
    }
    Ok((trimmed, detected))
}

pub fn needs_lowercase_retry(text: &str, translated: &str) -> bool {
    text.len() > 2
        && text.chars().any(|c| c.is_ascii_alphabetic())
        && text.chars().all(|c| !c.is_lowercase())
        && translated.eq_ignore_ascii_case(text)
}

pub async fn fetch_translate_once(
    client: &reqwest::Client,
    url: &str,
) -> Result<(String, String), String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Translation network error: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("Translation HTTP {status}"));
    }
    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Translation parse error: {e}"))?;
    parse_google_translate_json(&val)
}

pub async fn fetch_with_fallback(
    client: &reqwest::Client,
    text: &str,
    tl: &str,
) -> Result<(String, String), String> {
    let mut last_err = String::from("Translation request failed");
    for url in build_translate_urls(text, tl) {
        match fetch_translate_once(client, &url).await {
            Ok(pair) => return Ok(pair),
            Err(e) => last_err = e,
        }
    }
    Err(last_err)
}

pub async fn translate_single_resilient(
    client: &reqwest::Client,
    text: &str,
    tl: &str,
) -> Result<(String, String), String> {
    let (mut translated, mut detected) = fetch_with_fallback(client, text, tl).await?;
    if needs_lowercase_retry(text, &translated) {
        if let Ok((tr2, det2)) = fetch_with_fallback(client, &text.to_lowercase(), tl).await {
            if !tr2.is_empty() && !tr2.eq_ignore_ascii_case(text) {
                translated = tr2;
                detected = det2;
            }
        }
    }
    Ok((translated, detected))
}

pub async fn translate_chunk(
    client: &reqwest::Client,
    indices: &[usize],
    texts: &[String],
    tl: &str,
) -> (Vec<(usize, Option<String>)>, String, Option<String>) {
    if indices.len() >= 2 {
        let joined = texts.join("\n");
        if let Ok((tr_joined, det)) = fetch_with_fallback(client, &joined, tl).await {
            let parts: Vec<&str> = tr_joined.split('\n').map(|s| s.trim()).collect();
            if parts.len() == texts.len() && parts.iter().all(|p| !p.is_empty()) {
                let mut out = Vec::with_capacity(indices.len());
                for (pos, &u_idx) in indices.iter().enumerate() {
                    let mut line_tr = parts[pos].to_string();
                    if needs_lowercase_retry(&texts[pos], &line_tr) {
                        if let Ok((tr2, _)) =
                            fetch_with_fallback(client, &texts[pos].to_lowercase(), tl).await
                        {
                            if !tr2.is_empty() && !tr2.eq_ignore_ascii_case(&texts[pos]) {
                                line_tr = tr2;
                            }
                        }
                    }
                    out.push((u_idx, Some(line_tr)));
                }
                return (out, det, None);
            }
        }
    }

    // Fallback to controlled per-line translation (max 4 concurrent) if batching split lines or failed.
    let mut out = Vec::with_capacity(indices.len());
    let mut detected = String::from("auto");
    let mut first_err: Option<String> = None;

    for sub in indices.iter().zip(texts.iter()).collect::<Vec<_>>().chunks(4) {
        let mut handles = Vec::with_capacity(sub.len());
        for &(&u_idx, txt) in sub {
            let c = client.clone();
            let s = txt.clone();
            let lang = tl.to_string();
            handles.push(tauri::async_runtime::spawn(async move {
                (u_idx, translate_single_resilient(&c, &s, &lang).await)
            }));
        }
        for h in handles {
            if let Ok((u_idx, res)) = h.await {
                match res {
                    Ok((tr, det)) => {
                        if detected == "auto" && !det.is_empty() && det != "auto" {
                            detected = det;
                        }
                        out.push((u_idx, Some(tr)));
                    }
                    Err(e) => {
                        if first_err.is_none() {
                            first_err = Some(e);
                        }
                        out.push((u_idx, None));
                    }
                }
            }
        }
    }

    (out, detected, first_err)
}

/// Translates deduplicated lines by batching newline-separated chunks (cutting HTTP calls ~10x)
/// and automatically falling back to small single-line concurrent calls if a batch splits/merges lines.
pub async fn translate_unique_lines(
    client: &reqwest::Client,
    unique_texts: &[String],
    tl: &str,
) -> (Vec<Option<String>>, String, Option<String>) {
    const MAX_CHUNK_LINES: usize = 16;
    const MAX_CHUNK_BYTES: usize = 900;

    let mut chunks: Vec<Vec<usize>> = Vec::new();
    let mut cur = Vec::new();
    let mut cur_bytes = 0usize;

    for (idx, txt) in unique_texts.iter().enumerate() {
        let add_bytes = txt.len() + 1;
        if !cur.is_empty() && (cur.len() >= MAX_CHUNK_LINES || cur_bytes + add_bytes > MAX_CHUNK_BYTES) {
            chunks.push(cur);
            cur = Vec::new();
            cur_bytes = 0;
        }
        cur.push(idx);
        cur_bytes += add_bytes;
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }

    let mut results: Vec<Option<String>> = vec![None; unique_texts.len()];
    let mut detected_lang = String::from("auto");
    let mut first_err: Option<String> = None;

    for super_chunk in chunks.chunks(3) {
        let mut handles = Vec::with_capacity(super_chunk.len());
        for chunk_indices in super_chunk {
            let c = client.clone();
            let indices = chunk_indices.clone();
            let batch_texts: Vec<String> =
                indices.iter().map(|&i| unique_texts[i].clone()).collect();
            let lang = tl.to_string();

            handles.push(tauri::async_runtime::spawn(async move {
                translate_chunk(&c, &indices, &batch_texts, &lang).await
            }));
        }

        for h in handles {
            if let Ok((pairs, det, err)) = h.await {
                if detected_lang == "auto" && !det.is_empty() && det != "auto" {
                    detected_lang = det;
                }
                if first_err.is_none() {
                    first_err = err;
                }
                for (u_idx, tr_opt) in pairs {
                    results[u_idx] = tr_opt;
                }
            }
        }
    }

    (results, detected_lang, first_err)
}

pub fn is_ocr_noise_line(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || !trimmed.chars().any(|c| c.is_alphanumeric()) {
        return true;
    }
    trimmed.chars().count() == 1 && trimmed.chars().next().is_some_and(|c| c.is_ascii())
}
