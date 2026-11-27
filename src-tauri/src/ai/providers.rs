//! Unified HTTP LLM client across Cloud API Providers (Gemini, Groq, OpenRouter, OpenAI, Claude)
//! and optional Local Ollama (`127.0.0.1:11434`), with automatic 429 rate-limit failover.

use super::vault::{self, DecryptedCandidate};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ChatTurn {
    pub role: &'static str, // "user" | "assistant"
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct ProviderReply {
    pub text: String,
    pub used_label: String,
    pub used_provider: String,
    pub used_model: String,
}

fn agent_with_timeout(read_secs: u64) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(read_secs))
        .build()
}

fn call_gemini(
    cand: &DecryptedCandidate,
    system_prompt: &str,
    turns: &[ChatTurn],
) -> Result<String, (u16, String)> {
    // Pass API key exclusively via HTTP header so URLs/Transport errors can never leak credentials.
    let url = format!(
        "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
        cand.model
    );
    let contents: Vec<Value> = turns
        .iter()
        .map(|t| {
            json!({
                "role": if t.role == "assistant" { "model" } else { "user" },
                "parts": [{ "text": t.text }]
            })
        })
        .collect();
    let payload = json!({
        "systemInstruction": { "parts": [{ "text": system_prompt }] },
        "contents": contents,
        "generationConfig": { "temperature": 0.6, "maxOutputTokens": 900 }
    });

    let resp = agent_with_timeout(15)
        .post(&url)
        .set("Content-Type", "application/json")
        .set("x-goog-api-key", &cand.api_key)
        .send_string(&payload.to_string())
        .map_err(map_ureq_err)?;

    let v: Value = serde_json::from_str(&resp.into_string().map_err(|e| (0, e.to_string()))?)
        .map_err(|e| (0, e.to_string()))?;
    v.pointer("/candidates/0/content/parts/0/text")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| (0, "Empty Gemini response".to_string()))
}

pub fn parse_openai_response_text(v: &Value) -> Result<String, (u16, String)> {
    if let Some(err_msg) = v
        .pointer("/error/message")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let code = v.pointer("/error/code").and_then(|x| x.as_u64()).unwrap_or(502) as u16;
        return Err((code, format!("HTTP {code}: {}", err_msg.chars().take(180).collect::<String>())));
    }

    let msg_obj = v.pointer("/choices/0/message");
    if let Some(m) = msg_obj {
        // Handle both standard string content and array content blocks
        if let Some(s) = m.get("content").and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(s.to_string());
        }
        if let Some(arr) = m.get("content").and_then(|x| x.as_array()) {
            let joined: String = arr
                .iter()
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if !joined.is_empty() {
                return Ok(joined);
            }
        }
        if let Some(s) = m
            .get("reasoning")
            .and_then(|x| x.as_str())
            .or_else(|| m.get("reasoning_content").and_then(|x| x.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            return Ok(s.to_string());
        }
    }
    Err((0, "Empty AI response (no content returned by model)".to_string()))
}

fn call_openai_compatible(
    endpoint: &str,
    cand: &DecryptedCandidate,
    system_prompt: &str,
    turns: &[ChatTurn],
) -> Result<String, (u16, String)> {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
    for t in turns {
        messages.push(json!({ "role": t.role, "content": t.text }));
    }
    let payload = json!({
        "model": cand.model,
        "messages": messages,
        "temperature": 0.6,
        "max_tokens": 1600
    });

    let mut req = agent_with_timeout(15)
        .post(endpoint)
        .set("Content-Type", "application/json")
        .set("Authorization", &format!("Bearer {}", cand.api_key));
    if cand.provider == "openrouter" {
        req = req.set("HTTP-Referer", "https://github.com/inrwxyz/nexus").set("X-Title", "Nexus");
    }

    let resp = req
        .send_string(&payload.to_string())
        .map_err(map_ureq_err)?;

    let v: Value = serde_json::from_str(&resp.into_string().map_err(|e| (0, e.to_string()))?)
        .map_err(|e| (0, e.to_string()))?;
    parse_openai_response_text(&v)
}

fn call_claude(
    cand: &DecryptedCandidate,
    system_prompt: &str,
    turns: &[ChatTurn],
) -> Result<String, (u16, String)> {
    let messages: Vec<Value> = turns
        .iter()
        .map(|t| json!({ "role": t.role, "content": t.text }))
        .collect();
    let payload = json!({
        "model": cand.model,
        "max_tokens": 450,
        "system": system_prompt,
        "messages": messages
    });

    let resp = agent_with_timeout(15)
        .post("https://api.anthropic.com/v1/messages")
        .set("Content-Type", "application/json")
        .set("x-api-key", &cand.api_key)
        .set("anthropic-version", "2023-06-01")
        .send_string(&payload.to_string())
        .map_err(map_ureq_err)?;

    let v: Value = serde_json::from_str(&resp.into_string().map_err(|e| (0, e.to_string()))?)
        .map_err(|e| (0, e.to_string()))?;
    v.pointer("/content/0/text")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| (0, "Empty Claude response".to_string()))
}

pub fn call_ollama(model: &str, system_prompt: &str, turns: &[ChatTurn]) -> Result<String, String> {
    let mut messages = vec![json!({ "role": "system", "content": system_prompt })];
    for t in turns {
        messages.push(json!({ "role": t.role, "content": t.text }));
    }
    let payload = json!({
        "model": model,
        "messages": messages,
        "stream": false
    });

    let resp = agent_with_timeout(45)
        .post("http://127.0.0.1:11434/api/chat")
        .set("Content-Type", "application/json")
        .send_string(&payload.to_string())
        .map_err(|e| format!("Local LLM error: {e}"))?;

    let v: Value = serde_json::from_str(&resp.into_string().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    v.pointer("/message/content")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Empty Ollama response".to_string())
}

fn call_candidate(
    cand: &DecryptedCandidate,
    system_prompt: &str,
    turns: &[ChatTurn],
) -> Result<String, (u16, String)> {
    match cand.provider.as_str() {
        "gemini" => call_gemini(cand, system_prompt, turns),
        "groq" => call_openai_compatible("https://api.groq.com/openai/v1/chat/completions", cand, system_prompt, turns),
        "openrouter" => call_openai_compatible("https://openrouter.ai/api/v1/chat/completions", cand, system_prompt, turns),
        "openai" => call_openai_compatible("https://api.openai.com/v1/chat/completions", cand, system_prompt, turns),
        "claude" => call_claude(cand, system_prompt, turns),
        _ => Err((400, format!("Unsupported provider {}", cand.provider))),
    }
}

pub fn extract_clean_api_error(body: &str) -> String {
    let trimmed = body.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(msg) = v
            .pointer("/error/message")
            .and_then(|x| x.as_str())
            .or_else(|| v.pointer("/message").and_then(|x| x.as_str()))
        {
            return msg.trim().chars().take(180).collect();
        }
    }
    trimmed.chars().take(180).collect()
}

fn sanitize_transport_message(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("10060") || lower.contains("timed out") {
        return "Connection timed out — API server did not respond in time".to_string();
    }
    if lower.contains("10061") || lower.contains("connection refused") {
        return "Connection refused by host".to_string();
    }
    if lower.contains("dns") || lower.contains("failed to lookup") || lower.contains("no such host") {
        return "DNS lookup failed — check your internet connection".to_string();
    }
    if lower.contains("invalid header") {
        return "Invalid API key characters — please check what you pasted".to_string();
    }
    // Strip URLs so secrets or query params can never surface in error banners
    let cleaned: String = raw
        .split_whitespace()
        .map(|tok| if tok.contains("://") { "[endpoint]" } else { tok })
        .collect::<Vec<_>>()
        .join(" ");
    cleaned.chars().take(180).collect()
}

fn map_ureq_err(err: ureq::Error) -> (u16, String) {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = extract_clean_api_error(&body);
            (code, format!("HTTP {code}: {msg}"))
        }
        ureq::Error::Transport(t) => (0, sanitize_transport_message(&t.to_string())),
    }
}

/// Tests a single stored API key by sending a tiny ping prompt without exposing the secret.
pub fn test_single_key(id: &str) -> Result<String, String> {
    let cand = vault::decrypt_single(id)?;
    let ping = [ChatTurn {
        role: "user",
        text: "Reply with just: OK".to_string(),
    }];
    match call_candidate(&cand, "Respond concisely.", &ping) {
        Ok(reply) => {
            vault::clear_cooldown(&cand.id);
            Ok(format!("Connected ({}) · {}", cand.model, reply))
        }
        Err((code, msg)) => {
            if code == 429 || code == 503 {
                vault::mark_rate_limited(&cand.id, 90);
            }
            Err(msg)
        }
    }
}

/// Calls enabled Cloud API keys (with automatic 429 cooldown failover) and/or Local Ollama.
pub fn complete(
    routing_mode: &str,
    local_llm_enabled: bool,
    local_model: &str,
    system_prompt: &str,
    turns: &[ChatTurn],
) -> Result<ProviderReply, String> {
    let mut errors: Vec<String> = Vec::new();
    let try_local = || -> Result<ProviderReply, String> {
        call_ollama(local_model, system_prompt, turns).map(|text| ProviderReply {
            text,
            used_label: "Local LLM".to_string(),
            used_provider: "ollama".to_string(),
            used_model: local_model.to_string(),
        })
    };

    if routing_mode == "local_only" {
        if !local_llm_enabled {
            return Err("Local LLM is turned off".to_string());
        }
        return try_local();
    }

    if routing_mode == "local_first" && local_llm_enabled {
        match try_local() {
            Ok(r) => return Ok(r),
            Err(e) => errors.push(e),
        }
    }

    let cands = vault::candidates_for_call();
    for cand in &cands {
        match call_candidate(cand, system_prompt, turns) {
            Ok(text) => {
                vault::clear_cooldown(&cand.id);
                return Ok(ProviderReply {
                    text,
                    used_label: cand.label.clone(),
                    used_provider: cand.provider.clone(),
                    used_model: cand.model.clone(),
                });
            }
            Err((code, msg)) => {
                if code == 429 || code == 503 || code == 402 {
                    vault::mark_rate_limited(&cand.id, 90);
                }
                errors.push(format!("{}: {}", cand.label, msg));
            }
        }
    }

    if routing_mode == "hybrid" && local_llm_enabled {
        match try_local() {
            Ok(r) => return Ok(r),
            Err(e) => errors.push(e),
        }
    }

    if errors.is_empty() {
        Err("No active API key or Local LLM configured in Voice -> AI & Keys".to_string())
    } else {
        Err(errors.join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_formatter_extracts_json_message_and_scrubs_urls() {
        let json_body = r#"{"error":{"code":503,"message":"This model is currently experiencing high demand.","status":"UNAVAILABLE"}}"#;
        assert_eq!(
            extract_clean_api_error(json_body),
            "This model is currently experiencing high demand."
        );
        let scrubbed = sanitize_transport_message("https://generativelanguage.googleapis.com/v1beta?key=SECRET123: Connection failed");
        assert!(!scrubbed.contains("SECRET123"));
        assert!(sanitize_transport_message("os error 10060").contains("timed out"));

        let err200: Value = serde_json::from_str(r#"{"error":{"code":429,"message":"Provider returned error"}}"#).unwrap();
        assert_eq!(parse_openai_response_text(&err200).unwrap_err().0, 429);

        let reasoned: Value = serde_json::from_str(r#"{"choices":[{"message":{"content":"","reasoning":"{\"reply\":\"Sab badhiya hai!\",\"pc_command\":null}"}}]}"#).unwrap();
        assert!(parse_openai_response_text(&reasoned).unwrap().contains("Sab badhiya hai!"));
    }
}
