//! Pure domain model for DarkBox.
//!
//! Everything here is independent of HTTP, SQL and external tooling. Storage
//! rows map onto these structs, and adapters receive identifiers instead of
//! domain objects.

pub mod endpoint;
pub mod entities;
pub mod findings;
pub mod flow;

pub use endpoint::{EndpointKey, RouteGroup, UrlError};
pub use entities::*;

/// Conservative normalization used for URL candidates. Never merges trailing
/// slashes, encoded segments, repeated parameters or fragment state.
pub fn normalize_url(raw: &str) -> Result<EndpointKey, UrlError> {
    EndpointKey::parse(raw, "GET")
}

pub fn percent_encode_path_segment(value: &str) -> String {
    const SAFE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if SAFE.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoding_is_not_identity() {
        let a = normalize_url("http://127.0.0.1:8080/a%2Fb").unwrap();
        let b = normalize_url("http://127.0.0.1:8080/a/b").unwrap();
        assert_ne!(a.path, b.path);
    }
}
