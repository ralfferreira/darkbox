use std::sync::LazyLock;

use regex::Regex;

/// Patterns that catch common secret shapes inside free text. They are a
/// best-effort net, not a guarantee: an arbitrary secret rendered as plain text
/// with no label is not detectable here.
static SECRET_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
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

/// Exact key names (normalized to lowercase with `-` replaced by `_`) that hold
/// a secret. Token matching below also catches compounds such as
/// `access_token` or `client_secret`.
const SENSITIVE_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "secret",
    "client_secret",
    "secret_key",
    "secretkey",
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "auth_token",
    "api_token",
    "api_key",
    "apikey",
    "private_key",
    "authorization",
    "cookie",
    "cookies",
    "set_cookie",
    "session_id",
    "sessionid",
    "sid",
    "csrf",
    "xsrf",
    "csrf_token",
    "xsrf_token",
    "credential",
    "credentials",
    "otp",
];

/// Tokens that, when they appear as a word inside a key, mark the value as
/// sensitive. `api_key` matches through `api` + exact key, `access_token`
/// matches through `token`.
const SENSITIVE_TOKENS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "passphrase",
    "secret",
    "token",
    "csrf",
    "xsrf",
    "credential",
    "cookie",
    "authorization",
    "bearer",
    "otp",
];

/// Keys whose string value names a header, paired with a sibling secret value.
const HEADER_NAME_KEYS: &[&str] = &["name", "header", "header_name", "key"];
const HEADER_VALUE_KEYS: &[&str] = &["value", "values", "content", "data"];

fn normalize_key(key: &str) -> String {
    key.trim().to_ascii_lowercase().replace('-', "_")
}

/// True when the key itself identifies a secret field.
pub fn is_sensitive_key(key: &str) -> bool {
    let normalized = normalize_key(key);
    if SENSITIVE_KEYS.contains(&normalized.as_str()) {
        return true;
    }
    normalized
        .split('_')
        .any(|token| SENSITIVE_TOKENS.contains(&token))
}

fn is_sensitive_header(name: &str) -> bool {
    SENSITIVE_HEADERS.contains(&name.trim().to_ascii_lowercase().as_str())
}

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
        if let Some((name, _)) = line.split_once(':')
            && is_sensitive_header(name)
        {
            lines.push(format!("{}: [redacted]", name.trim()));
            redactions += 1;
            continue;
        }
        let mut value = if looks_like_url(line) {
            redact_url_query(line)
        } else {
            line.to_string()
        };
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

fn looks_like_url(value: &str) -> bool {
    value.contains("://") || value.starts_with("//") || value.starts_with('/')
}

/// Redacts credentials in a URL: userinfo and sensitive query parameters.
/// The path, host and non-sensitive parameters are preserved so navigation
/// still works.
pub fn redact_url_query(input: &str) -> String {
    let Some((base, query)) = input.split_once('?') else {
        return redact_userinfo(input);
    };
    let (query, fragment) = match query.split_once('#') {
        Some((query, fragment)) => (query, Some(fragment)),
        None => (query, None),
    };
    let redacted: Vec<String> = query
        .split('&')
        .map(|pair| {
            let (name, value) = match pair.split_once('=') {
                Some((name, value)) => (name, value),
                None => (pair, ""),
            };
            if is_sensitive_key(name) {
                if value.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}=[redacted]")
                }
            } else {
                pair.to_string()
            }
        })
        .collect();
    let mut output = format!("{}?{}", redact_userinfo(base), redacted.join("&"));
    if let Some(fragment) = fragment {
        output.push('#');
        output.push_str(fragment);
    }
    output
}

fn redact_userinfo(input: &str) -> String {
    let Some(scheme_end) = input.find("://") else {
        return input.to_string();
    };
    let authority_start = scheme_end + 3;
    let authority_end = input[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(input.len());
    let authority = &input[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return input.to_string();
    };
    let host = &authority[at + 1..];
    format!("{}[redacted]@{}", &input[..authority_start], host)
}

/// Removes a specific known secret. Used by acceptance tests to prove that a
/// seeded token cannot reach an export.
pub fn contains_known_secret(input: &str, secret: &str) -> bool {
    !secret.is_empty() && input.contains(secret)
}

/// Redacts exact occurrences of secrets the caller already knows. The adapter
/// uses the same idea before sending an observation to the core; this helper
/// lets the core defend again if it learns a secret by another route.
pub fn redact_known_secrets(value: &serde_json::Value, secrets: &[String]) -> serde_json::Value {
    let usable: Vec<&str> = secrets
        .iter()
        .map(String::as_str)
        .filter(|secret| secret.len() >= 4)
        .collect();
    if usable.is_empty() {
        return value.clone();
    }
    match value {
        serde_json::Value::String(text) => {
            let mut redacted = text.clone();
            for secret in &usable {
                redacted = redacted.replace(*secret, "[redacted]");
            }
            serde_json::Value::String(redacted)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| redact_known_secrets(item, secrets))
                .collect(),
        ),
        serde_json::Value::Object(object) => serde_json::Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.clone(), redact_known_secrets(value, secrets)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Sanitizes structured browser data before it reaches storage, events,
/// transcript, memory or the model. The JSON shape (including element refs) is
/// preserved. Values under sensitive keys, header pairs, sensitive form fields
/// and URL query parameters are redacted even when the value carries no
/// recognizable prefix.
pub fn sanitize_json_value(value: &serde_json::Value, max_bytes: usize) -> serde_json::Value {
    sanitize_json_value_with_secrets(value, max_bytes, &[])
}

pub fn sanitize_json_value_with_secrets(
    value: &serde_json::Value,
    max_bytes: usize,
    secrets: &[String],
) -> serde_json::Value {
    let value = redact_known_secrets(value, secrets);
    match &value {
        serde_json::Value::String(text) => {
            serde_json::Value::String(sanitize_text(text, max_bytes).text)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| sanitize_json_value_with_secrets(item, max_bytes, &[]))
                .collect(),
        ),
        serde_json::Value::Object(object) => {
            let sensitive_object = object
                .get("sensitive")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                || object
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(|value| value.eq_ignore_ascii_case("password"))
                    .unwrap_or(false);
            let header_pair = HEADER_NAME_KEYS.iter().any(|name_key| {
                object
                    .get(*name_key)
                    .and_then(serde_json::Value::as_str)
                    .map(is_sensitive_header)
                    .unwrap_or(false)
            });
            let mut sanitized = serde_json::Map::new();
            for (key, item) in object {
                let redact_value = is_sensitive_key(key)
                    || (header_pair && HEADER_VALUE_KEYS.contains(&key.as_str()))
                    || (sensitive_object && key == "value");
                if redact_value {
                    sanitized.insert(key.clone(), serde_json::Value::String("[redacted]".into()));
                } else {
                    sanitized.insert(
                        key.clone(),
                        sanitize_json_value_with_secrets(item, max_bytes, &[]),
                    );
                }
            }
            serde_json::Value::Object(sanitized)
        }
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_and_tokens_are_removed() {
        let raw = "GET /x HTTP/1.1\nHost: t\nCookie: sid=abcdef123456\nAuthorization: Bearer abcdefghijklmnop\n\napi_key=abcdef1234";
        let out = sanitize_text(raw, 10_000);
        assert!(!out.text.contains("abcdef123456"));
        assert!(!out.text.contains("abcdefghijklmnop"));
        assert!(!out.text.contains("abcdef1234"));
        assert!(out.redactions >= 3);
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

#[cfg(test)]
mod json_tests {
    use super::*;

    #[test]
    fn sensitive_keys_are_redacted_without_losing_refs() {
        let value = serde_json::json!({
            "url": "http://lab/app?token=9f2b7c1a&period=2026-08",
            "interactive": [{
                "ref": "e5",
                "name": "Password",
                "value": "hunter22",
                "sensitive": true
            }]
        });
        let out = sanitize_json_value(&value, 10_000);
        assert_eq!(out["interactive"][0]["ref"], "e5");
        assert_eq!(out["interactive"][0]["value"], "[redacted]");
        assert_eq!(out["interactive"][0]["name"], "Password");
        assert!(out["url"].as_str().unwrap().contains("token=[redacted]"));
        assert!(out["url"].as_str().unwrap().contains("period=2026-08"));
    }

    #[test]
    fn bare_sensitive_key_is_redacted() {
        let value = serde_json::json!({"password": "hunter22", "note": "keep me"});
        let out = sanitize_json_value(&value, 10_000);
        assert_eq!(out["password"], "[redacted]");
        assert_eq!(out["note"], "keep me");
        assert!(!out.to_string().contains("hunter22"));
    }

    #[test]
    fn sensitive_header_pairs_are_redacted() {
        let value = serde_json::json!({
            "headers": [
                {"name": "Cookie", "value": "sid=abc123456"},
                {"name": "Accept", "value": "text/html"}
            ]
        });
        let out = sanitize_json_value(&value, 10_000);
        assert_eq!(out["headers"][0]["value"], "[redacted]");
        assert_eq!(out["headers"][1]["value"], "text/html");
    }

    #[test]
    fn arbitrary_sentinel_in_sensitive_field_is_redacted() {
        let sentinel = "q7Zx2Lm9Vt4P";
        let value = serde_json::json!({
            "interactive": [{"ref": "e9", "type": "text", "field_name": "token", "sensitive": true, "value": sentinel}],
            "url": format!("http://lab/app?access_token={sentinel}&period=all")
        });
        let out = sanitize_json_value(&value, 10_000);
        assert!(!out.to_string().contains(sentinel));
        assert_eq!(out["interactive"][0]["ref"], "e9");
    }

    #[test]
    fn previous_value_only_projection_leaked_a_bare_key_secret() {
        // Regression: the earlier projection sanitized each string value in
        // isolation, losing the key, so `{"password":"hunter22"}` survived.
        let value = serde_json::json!({"password": "hunter22"});
        let leaked = match &value {
            serde_json::Value::Object(object) => serde_json::Value::Object(
                object
                    .iter()
                    .map(|(key, item)| {
                        let item = match item {
                            serde_json::Value::String(text) => {
                                serde_json::Value::String(sanitize_text(text, 10_000).text)
                            }
                            other => other.clone(),
                        };
                        (key.clone(), item)
                    })
                    .collect(),
            ),
            other => other.clone(),
        };
        assert!(
            leaked.to_string().contains("hunter22"),
            "the value-only projection was expected to leak the bare-key secret"
        );
        let fixed = sanitize_json_value(&value, 10_000);
        assert!(!fixed.to_string().contains("hunter22"));
    }

    #[test]
    fn known_secret_is_redacted_by_value() {
        let sentinel = "k3N8pQ1sW7dR".to_string();
        let value = serde_json::json!({"text": format!("value {sentinel} here"), "keep": "other"});
        let out = sanitize_json_value_with_secrets(&value, 10_000, std::slice::from_ref(&sentinel));
        assert!(!out.to_string().contains(&sentinel));
        assert_eq!(out["keep"], "other");
    }
}
