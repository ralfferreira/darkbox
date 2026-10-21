use serde::{Deserialize, Serialize};

/// The exact identity of an endpoint. A route group is a separate, versioned
/// interpretation and never replaces this record.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EndpointKey {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub method: String,
    pub path: String,
    /// Parameter names in the order they appear. Values are never stored here.
    pub query_names: Vec<String>,
    /// Raw query when it is needed to reproduce a request. Sensitive values are
    /// stripped by the evidence layer before persistence.
    pub raw_query: Option<String>,
    /// Fragment states belong to the page, not to a new HTTP path.
    pub fragment: Option<String>,
}

impl EndpointKey {
    pub fn parse(raw: &str, method: &str) -> Result<Self, UrlError> {
        let url =
            url::Url::parse(raw.trim()).map_err(|error| UrlError::Invalid(error.to_string()))?;
        Self::from_url(&url, method)
    }

    pub fn from_url(url: &url::Url, method: &str) -> Result<Self, UrlError> {
        let scheme = url.scheme().to_ascii_lowercase();
        if !matches!(scheme.as_str(), "http" | "https") {
            return Err(UrlError::UnsupportedScheme(scheme));
        }
        let host = url
            .host_str()
            .ok_or_else(|| UrlError::Invalid("missing host".into()))?
            .to_ascii_lowercase();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| UrlError::Invalid("missing effective port".into()))?;
        let method = method.trim().to_ascii_uppercase();
        if method.is_empty() {
            return Err(UrlError::Invalid("empty method".into()));
        }
        let path = if url.path().is_empty() {
            "/".to_string()
        } else {
            url.path().to_string()
        };
        let query_names = url
            .query_pairs()
            .map(|(name, _)| name.into_owned())
            .collect();
        let raw_query = url.query().map(|q| q.to_string());
        let fragment = url.fragment().map(|f| f.to_string());
        Ok(Self {
            scheme,
            host,
            port,
            method,
            path,
            query_names,
            raw_query,
            fragment,
        })
    }

    pub fn origin(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.port)
    }

    pub fn url_without_fragment(&self) -> String {
        let mut out = format!("{}{}", self.origin(), self.path);
        if let Some(query) = &self.raw_query {
            out.push('?');
            out.push_str(query);
        }
        out
    }

    /// Stable exact-record key. Query parameter values are deliberately absent
    /// so two values of the same parameter do not create separate records, but
    /// parameter order and repetition are preserved.
    pub fn record_key(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.method,
            self.origin(),
            self.path,
            self.query_names.join("&")
        )
    }

    pub fn route_group(&self) -> RouteGroup {
        RouteGroup {
            pattern: format!("{} {}", self.method, parameterize_path(&self.path)),
            version: 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteGroup {
    pub pattern: String,
    pub version: i32,
}

/// Replaces only segments whose whole value looks generated. Differences such
/// as a trailing slash or an encoded separator survive.
pub fn parameterize_path(path: &str) -> String {
    let trailing = path.ends_with('/') && path != "/";
    let mut segments: Vec<String> = path
        .split('/')
        .map(|segment| {
            if is_generated_segment(segment) {
                "{id}".to_string()
            } else {
                segment.to_string()
            }
        })
        .collect();
    let mut joined = segments.join("/");
    if trailing && !joined.ends_with('/') {
        joined.push('/');
    }
    segments.clear();
    joined
}

fn is_generated_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    if segment.len() >= 16 && segment.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    if segment.contains('-')
        && segment.len() >= 20
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        && segment.chars().filter(|c| c.is_ascii_digit()).count() >= 4
    {
        return true;
    }
    segment.len() >= 2
        && segment.chars().all(|c| c.is_ascii_digit())
        && !(segment.len() == 4 && segment.starts_with("20"))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UrlError {
    #[error("invalid URL: {0}")]
    Invalid(String),
    #[error("unsupported scheme: {0}")]
    UnsupportedScheme(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trailing_slash_is_preserved() {
        let a = EndpointKey::parse("http://example.test/a", "GET").unwrap();
        let b = EndpointKey::parse("http://example.test/a/", "GET").unwrap();
        assert_ne!(a.record_key(), b.record_key());
        assert_eq!(a.route_group().pattern, "GET /a");
        assert_eq!(b.route_group().pattern, "GET /a/");
    }

    #[test]
    fn repeated_parameters_keep_order() {
        let key = EndpointKey::parse("http://example.test/x?b=2&a=1&b=3", "get").unwrap();
        assert_eq!(key.method, "GET");
        assert_eq!(key.query_names, vec!["b", "a", "b"]);
    }

    #[test]
    fn default_ports_are_explicit() {
        let key = EndpointKey::parse("https://example.test/", "GET").unwrap();
        assert_eq!(key.port, 443);
        assert_eq!(key.origin(), "https://example.test:443");
    }

    #[test]
    fn generated_segments_group_but_static_ones_do_not() {
        let a = EndpointKey::parse("http://t.test/items/42/edit", "GET").unwrap();
        let b = EndpointKey::parse("http://t.test/items/43/edit", "GET").unwrap();
        assert_eq!(a.route_group().pattern, b.route_group().pattern);
        let c = EndpointKey::parse("http://t.test/items/new/edit", "GET").unwrap();
        assert_ne!(a.route_group().pattern, c.route_group().pattern);
    }

    #[test]
    fn fragments_never_create_paths() {
        let key = EndpointKey::parse("http://t.test/app#/settings", "GET").unwrap();
        assert_eq!(key.path, "/app");
        assert_eq!(key.fragment.as_deref(), Some("/settings"));
    }
}
