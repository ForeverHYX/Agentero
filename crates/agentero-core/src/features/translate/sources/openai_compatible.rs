use crate::error::AppError;
use crate::http;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Serialize)]
struct OpenAiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct OpenAiRequest<'a> {
    model: &'a str,
    messages: [OpenAiMessage<'a>; 2],
    temperature: f32,
}

/// Instruction block for the OpenAI-compatible path. Mirrors the Agent prompt in
/// `src/lib/translate/prompt.ts`; keep both in sync.
const OPENAI_TRANSLATE_SYSTEM: &str = "You are a professional academic translator. You render research-paper prose into fluent, idiomatic target-language text and output only the translation.";

/// Numbered batch payload ([[1]] …, [[2]] …): ask the model to keep the
/// markers and paragraph count so the caller can split the result back.
/// Appended by the Host on both the default and the custom-prompt path.
const NUMBERED_BATCH_RULE: &str = "The text contains several paragraphs, each prefixed with a [[n]] marker. Translate every paragraph and keep the same [[n]] markers, in the same order, with the same number of paragraphs. Do not merge paragraphs.";

/// Literal word-for-word output at 0.0 reads badly for paper prose; a small
/// amount of sampling lets the model restructure sentences.
const OPENAI_TRANSLATE_TEMPERATURE: f32 = 0.2;

/// Map a language code to a prompt-facing display name. Mirrors
/// `targetLangDisplayName` in `src/lib/translate/lang.ts`; unknown values pass
/// through so future codes keep working.
fn target_display_name(target: &str) -> String {
    let t = target.trim();
    let lower = t.to_ascii_lowercase();
    if lower == "zh" || lower == "zh-cn" || lower.starts_with("zh-") || lower == "chinese" {
        "Chinese".into()
    } else if lower == "en" || lower.starts_with("en-") || lower == "english" {
        "English".into()
    } else {
        t.to_string()
    }
}

/// Substitute `{{targetLang}}` / `{{sourceLang}}` in a user-supplied prompt
/// template. `auto` keeps the wording the built-in prompt uses.
fn interpolate_custom_prompt(custom: &str, source: &str, target: &str) -> String {
    let from = if source == "auto" {
        "the source language"
    } else {
        source
    };
    custom
        .replace("{{targetLang}}", &target_display_name(target))
        .replace("{{sourceLang}}", from)
}

fn openai_translate_prompt(text: &str, source: &str, target: &str) -> String {
    let numbered_hint = if text.contains("[[1]]") {
        format!("\n- {NUMBERED_BATCH_RULE}")
    } else {
        String::new()
    };
    let from = if source == "auto" {
        "the source language"
    } else {
        source
    };
    format!(
        "Translate the text below from {from} to {target}.\n\nRules:\n\
         - The source is prose from a research paper, often extracted from a PDF text layer. Translate the meaning, not the word order: re-order clauses and split long sentences when that reads better.\n\
         - Keep mathematics, symbols, variable names, units, inline code, URLs, citation markers and figure/table/equation numbers exactly as they appear, including any ⟦n⟧ placeholders.\n\
         - Use the established target-language term for each concept and stay consistent.\n\
         - Do not add, drop, summarize or explain anything. No translator notes, no markdown fences.\n\
         - Output only the translation.{numbered_hint}\n\nText:\n{text}"
    )
}

/// Compose the (system, user) message pair. A non-empty `custom_prompt`
/// replaces the built-in system + rules block (after `{{targetLang}}` /
/// `{{sourceLang}}` interpolation); the numbered-batch rule and the text
/// payload stay Host-composed so `[[n]]` splitting keeps working.
fn openai_translate_messages(
    text: &str,
    source: &str,
    target: &str,
    custom_prompt: Option<&str>,
) -> (String, String) {
    let custom = custom_prompt.map(str::trim).filter(|s| !s.is_empty());
    match custom {
        None => (
            OPENAI_TRANSLATE_SYSTEM.to_string(),
            openai_translate_prompt(text, source, target),
        ),
        Some(template) => {
            let system = interpolate_custom_prompt(template, source, target);
            let mut user = String::new();
            if text.contains("[[1]]") {
                user.push_str(NUMBERED_BATCH_RULE);
                user.push_str("\n\n");
            }
            user.push_str("Text:\n");
            user.push_str(text);
            (system, user)
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub async fn translate_openai_compatible(
    text: &str,
    source: &str,
    target: &str,
    timeout: Duration,
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
    custom_prompt: Option<&str>,
) -> Result<String, AppError> {
    let key = super::required_api_key("OpenAI-compatible", api_key)?;
    let model = model
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            AppError::message("OpenAI-compatible requires model (Settings → Translate)")
        })?;
    let url = super::optional_endpoint(base_url, "https://api.openai.com/v1", "/chat/completions");
    let (system, user) = openai_translate_messages(text, source, target, custom_prompt);
    let client = http::client(timeout)?;
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", "application/json")
        .json(&OpenAiRequest {
            model,
            messages: [
                OpenAiMessage {
                    role: "system",
                    content: &system,
                },
                OpenAiMessage {
                    role: "user",
                    content: &user,
                },
            ],
            temperature: OPENAI_TRANSLATE_TEMPERATURE,
        })
        .send()
        .await
        .map_err(|e| AppError::message(format!("OpenAI-compatible request failed: {e}")))?;
    let (status, body) = super::read_body(resp).await?;
    if !status.is_success() {
        return Err(super::http_err(status, &body, "OpenAI-compatible"));
    }
    let v: Value = serde_json::from_str(&body)
        .map_err(|e| AppError::message(format!("OpenAI-compatible parse: {e}")))?;
    let choice = v
        .get("choices")
        .and_then(|x| x.get(0))
        .ok_or_else(|| AppError::message("Unexpected OpenAI-compatible response"))?;
    if let Some(reason) = choice.get("finish_reason").and_then(|x| x.as_str()) {
        if matches!(reason, "length" | "max_tokens" | "content_filter") {
            return Err(AppError::message(format!(
                "OpenAI-compatible translation incomplete (finish_reason={reason}); retry with a smaller chunk"
            )));
        }
    }
    choice
        .get("message")
        .and_then(|x| x.get("content"))
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| AppError::message("Unexpected OpenAI-compatible response"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_prompt_carries_academic_rules_and_marker_hint() {
        let (system, single) = openai_translate_messages("Hello world", "auto", "zh-CN", None);
        assert_eq!(system, OPENAI_TRANSLATE_SYSTEM);
        assert!(single.contains("the source language"));
        assert!(single.contains("zh-CN"));
        assert!(single.contains("Translate the meaning, not the word order"));
        assert!(single.contains("Output only the translation."));
        assert!(!single.contains("[[n]] marker"));

        let (_, batch) = openai_translate_messages("[[1]] a\n\n[[2]] b", "en", "zh-CN", None);
        assert!(batch.contains("from en to zh-CN"));
        assert!(batch.contains("[[n]] marker"));
    }

    #[test]
    fn custom_prompt_replaces_instructions_but_not_payload() {
        let (system, user) = openai_translate_messages(
            "Hello world",
            "auto",
            "zh-CN",
            Some("Translate into {{targetLang}} from {{sourceLang}}. Be terse."),
        );
        assert_eq!(
            system,
            "Translate into Chinese from the source language. Be terse."
        );
        assert!(user.starts_with("Text:\nHello world"));
        assert!(!user.contains("Rules:"));
        // Whitespace-only custom prompts fall back to the built-in prompt.
        let (system, _) = openai_translate_messages("Hello world", "auto", "zh-CN", Some("  "));
        assert_eq!(system, OPENAI_TRANSLATE_SYSTEM);
    }

    #[test]
    fn custom_prompt_keeps_numbered_batch_rule() {
        let (_, user) =
            openai_translate_messages("[[1]] a\n\n[[2]] b", "en", "zh-CN", Some("Custom."));
        assert!(user.contains("[[n]] marker"));
        assert!(user.contains("Do not merge paragraphs."));
        assert!(user.ends_with("Text:\n[[1]] a\n\n[[2]] b"));
    }
}
