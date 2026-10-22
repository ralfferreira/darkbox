use std::sync::LazyLock;

use regex::Regex;

static SECRET_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (
            Regex::new(r#"(?i)\bLAB_SECRET_[A-Za-z0-9_\-]+"#).unwrap(),
            "LAB_SECRET_[redacted]",
        ),
        (
            Regex::new(r#"(?i)\bBearer\s+[A-Za-z0-9._~+/=\-]{8,}"#).unwrap(),
            "Bearer [redacted]",
        ),
        (
            Regex::new(r#"(?i)\b(api[_-]?key|access[_-]?token|refresh[_-]?token|secret|password)\b["']?\s*[:=]\s*["']?([^\s"'&,;]{4,})"#).unwrap(),
            "$1=[redacted]",
        ),
        (
            Regex::new(r#"(?i)\bsession[_-]?id\s*[:=]\s*[A-Za-z0-9._\-]{6,}"#).unwrap(),
            "session_id=[redacted]",
        ),
        (
            Regex::new(r#"(?i)\b(?:darkbox_)?sid=[A-Za-z0-9._\-]{6,}"#).unwrap(),
            "sid=[redacted]",
        ),
        (
            Regex::new(r#"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}"#).unwrap(),
            "[email redacted]",
        ),
    ]
});

const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-csrf-token",
    "x-xsrf-token",
];

#[derive(Debug, Clone)]
pub struct SanitizedText {
    pub text: String,
    pub truncated: bool,
    pub redactions: usize,
}

/// Sanitizes a capture before persistence. The result is a derived document,
/// not a byte-for-byte copy of the original.
pub fn sanitize_text(input: &str, max_bytes: usize) -> SanitizedText {
    let mut redactions = 0usize;
    let mut lines = Vec::new();
    for line in input.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some((name, _)) = line.split_once(':')
            && SENSITIVE_HEADERS.contains(&name.trim().to_ascii_lowercase().as_str())
        {
            lines.push(format!("{}: [redacted]", name.trim()));
            redactions += 1;
            continue;
        }
        let _ = lower;
        let mut value = line.to_string();
        for (regex, replacement) in SECRET_PATTERNS.iter() {
            let before = value.clone();
            value = regex.replace_all(&value, *replacement).into_owned();
            if value != before {
                redactions += 1;
            }
        }
        lines.push(value);
    }
    let mut text = lines.join("\n");
    let mut truncated = false;
    if text.len() > max_bytes {
        let mut boundary = max_bytes;
        while boundary > 0 && !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
        text.push_str("\n[truncated by evidence policy]");
        truncated = true;
    }
    SanitizedText {
        text,
        truncated,
        redactions,
    }
}

/// Removes a specific known secret. Used by acceptance tests to prove that the
/// seeded token cannot reach an export.
pub fn contains_known_secret(input: &str, secret: &str) -> bool {
    input.contains(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_tokens_are_removed() {
        let raw = "GET /x HTTP/1.1\nHost: t\nCookie: sid=abcdef123456\nAuthorization: Bearer abcdefghijklmnop\n\nLAB_SECRET_darkbox_7f3a and {\"password\":\"hunter22\"}";
        let out = sanitize_text(raw, 10_000);
        assert!(!out.text.contains("LAB_SECRET_darkbox_7f3a"));
        assert!(!out.text.contains("abcdef123456"));
        assert!(!out.text.contains("abcdefghijklmnop"));
        assert!(!out.text.contains("hunter22"));
        assert!(out.redactions >= 3);
    }

    #[test]
    fn lab_sentinel_is_redacted_in_json_text() {
        let value = serde_json::json!({"text": "DarkBox Lab\nSecret surface\nLAB_SECRET_darkbox_7f3a\nAPI token"});
        let out = sanitize_json_value(&value, 10_000);
        assert!(
            !out.to_string().contains("LAB_SECRET_darkbox_7f3a"),
            "{}",
            out
        );
    }

    #[test]
    fn truncation_is_reported() {
        let out = sanitize_text(&"x".repeat(1000), 100);
        assert!(out.truncated);
        assert!(out.text.contains("truncated"));
    }

    #[test]
    fn normal_content_survives() {
        let out = sanitize_text("GET /pricing HTTP/1.1\nHost: lab\n\nprice: 42", 1000);
        assert!(out.text.contains("GET /pricing"));
        assert!(out.text.contains("price: 42"));
        assert_eq!(out.redactions, 0);
    }
}

/// Sanitizes structured browser data before it reaches storage, events,
/// transcript, memory or the model. The JSON shape (including element refs) is
/// preserved unless sanitization makes it unparseable.
pub fn sanitize_json_value(value: &serde_json::Value, max_bytes: usize) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            serde_json::Value::String(sanitize_text(text, max_bytes).text)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| sanitize_json_value(item, max_bytes))
                .collect(),
        ),
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), sanitize_json_value(value, max_bytes)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn structured_values_and_urls_are_redacted_without_losing_refs() {
        let value = serde_json::json!({
            "url": "http://lab/app?token=LAB_SECRET_darkbox_7f3a",
            "interactive": [{
                "ref": "e5",
                "name": "Password",
                "value": "LAB_SECRET_darkbox_7f3a",
                "error": "api_key=abcdef1234"
            }]
        });
        let out = sanitize_json_value(&value, 10_000);
        assert_eq!(out["interactive"][0]["ref"], "e5");
        assert!(!out.to_string().contains("LAB_SECRET_darkbox_7f3a"));
        assert!(!out.to_string().contains("abcdef1234"));
    }
}
