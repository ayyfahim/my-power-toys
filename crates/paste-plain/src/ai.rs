//! Optional AI transformation through an OpenAI-compatible
//! `/chat/completions` endpoint (OpenAI, Azure-compatible proxies, Ollama,
//! LM Studio, llama.cpp server, ...).

use crate::config::PastePlainConfig;
use anyhow::{Context, Result};
use std::time::Duration;

/// Same system prompt as upstream `CustomActionTransformService`.
pub const SYSTEM_PROMPT: &str = "You are tasked with reformatting user's clipboard data. Use the user's instructions, and the content of their clipboard below to edit their clipboard content as they have requested it.\nDo not output anything else besides the reformatted clipboard content.";

/// Prompt for the built-in "Fix spelling and grammar" action.
pub const FIX_SPELLING_PROMPT: &str =
    "Fix all spelling and grammar errors in the clipboard text and return the corrected version.";

/// Build the chat completions URL from a base URL.
pub fn completions_url(endpoint: &str) -> String {
    let base = endpoint.trim().trim_end_matches('/');
    if base.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

/// The user message, formatted like upstream `SemanticKernelPasteProvider`.
pub fn user_message(prompt: &str, clipboard: &str) -> String {
    format!("User instructions:\n{prompt}\n\nClipboard Content:\n{clipboard}\n\nOutput:")
}

fn request_body(model: &str, prompt: &str, clipboard: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": user_message(prompt, clipboard)},
        ],
    })
}

/// Extract `choices[0].message.content` from a chat completions response.
pub fn parse_response(body: &serde_json::Value) -> Result<String> {
    if let Some(err) = body.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        anyhow::bail!("AI service error: {msg}");
    }
    body.pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
        .context("AI response did not contain choices[0].message.content")
}

/// Run `prompt` against `clipboard` text. Blocking, bounded by `ai_timeout_secs`.
pub fn transform(cfg: &PastePlainConfig, prompt: &str, clipboard: &str) -> Result<String> {
    if !cfg.ai_available() {
        anyhow::bail!("AI paste is disabled or no endpoint is configured");
    }
    let url = completions_url(&cfg.ai_endpoint);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(cfg.ai_timeout_secs.max(1)))
        .build();
    let mut req = agent.post(&url).set("Content-Type", "application/json");
    let key = std::env::var(cfg.ai_api_key_env.trim()).unwrap_or_default();
    if !key.trim().is_empty() {
        req = req.set("Authorization", &format!("Bearer {}", key.trim()));
    }
    let body = request_body(&cfg.ai_model, prompt, clipboard);
    let resp: serde_json::Value = match req.send_json(body) {
        Ok(r) => r.into_json().context("invalid JSON from AI endpoint")?,
        Err(ureq::Error::Status(code, r)) => {
            let text = r.into_string().unwrap_or_default();
            let parsed: Option<serde_json::Value> = serde_json::from_str(&text).ok();
            if let Some(v) = parsed
                && v.get("error").is_some()
            {
                return parse_response(&v).with_context(|| format!("HTTP {code}"));
            }
            anyhow::bail!("AI endpoint returned HTTP {code}");
        }
        Err(e) => return Err(anyhow::anyhow!("AI request to {url} failed: {e}")),
    };
    parse_response(&resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_normalisation() {
        assert_eq!(
            completions_url("https://api.openai.com/v1/"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            completions_url("http://h/v1/chat/completions"),
            "http://h/v1/chat/completions"
        );
    }

    #[test]
    fn parse_ok_and_error() {
        let ok = serde_json::json!({"choices":[{"message":{"content":" hi \n"}}]});
        assert_eq!(parse_response(&ok).unwrap(), "hi");
        let err = serde_json::json!({"error":{"message":"bad key"}});
        assert!(
            parse_response(&err)
                .unwrap_err()
                .to_string()
                .contains("bad key")
        );
    }

    #[test]
    fn body_contains_prompt_and_clipboard() {
        let b = request_body("m", "Translate", "Hello");
        assert_eq!(b["model"], "m");
        let user = b["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("Translate") && user.contains("Hello"));
    }

    #[test]
    fn disabled_config_refuses() {
        let cfg = PastePlainConfig::default();
        assert!(transform(&cfg, "x", "y").is_err());
    }
}
