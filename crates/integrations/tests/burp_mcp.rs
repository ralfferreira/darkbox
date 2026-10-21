use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::State;
use axum::response::IntoResponse;
use axum::{Json, Router, routing::post};
use darkbox_integrations::BurpAdapter;
use serde_json::{Value, json};
use tokio::sync::Mutex;

#[derive(Clone)]
struct Mock {
    initialized: Arc<AtomicBool>,
    calls: Arc<Mutex<Vec<Value>>>,
}

async fn mcp(State(state): State<Mock>, Json(payload): Json<Value>) -> impl IntoResponse {
    let method = payload
        .get("method")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let id = payload.get("id").cloned();
    if method == "notifications/initialized" {
        return (axum::http::StatusCode::ACCEPTED, Json(json!({})));
    }
    let result = match method {
        "initialize" => {
            state.initialized.store(true, Ordering::Relaxed);
            json!({
                "protocolVersion": "2025-03-26",
                "serverInfo": { "name": "burp-mcp-mock", "version": "2026.1" },
                "capabilities": { "tools": {} }
            })
        }
        "tools/list" => json!({
            "tools": [{
                "name": "burp_get_proxy_history",
                "description": "Return in-scope proxy history",
                "inputSchema": { "type": "object", "properties": { "limit": { "type": "number" } } }
            }]
        }),
        "tools/call" => {
            state.calls.lock().await.push(payload.clone());
            json!({
                "structuredContent": {
                    "data": [{
                        "time": "2026-10-21T12:00:00Z",
                        "request": {
                            "method": "GET",
                            "url": "https://example.test/reports",
                            "headers": {"Host": "example.test", "Cookie": "sid=secret"},
                            "body": null
                        },
                        "response": {
                            "status": 200,
                            "headers": [{"name": "Content-Type", "value": "text/html"}],
                            "body": "<html>reports</html>"
                        }
                    }]
                }
            })
        }
        _ => json!({}),
    };
    (
        axum::http::StatusCode::OK,
        Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
    )
}

#[tokio::test]
async fn probes_and_imports_burp_history_over_mcp() {
    let state = Mock {
        initialized: Arc::new(AtomicBool::new(false)),
        calls: Arc::new(Mutex::new(Vec::new())),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/mcp", post(mcp))
        .with_state(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // SAFETY: this integration test owns the process-wide variable and no other
    // test reads it. Rust 2024 requires the unsafe block for set_var.
    unsafe {
        std::env::set_var("DARKBOX_BURP_MCP_URL", format!("http://{address}/mcp"));
        std::env::set_var("DARKBOX_BURP_MCP_TOKEN", "test-token");
    }
    let adapter = BurpAdapter::from_env().unwrap();
    let status = adapter.probe().await.unwrap();
    assert!(status.available);
    assert_eq!(status.server, "burp-mcp-mock");
    assert_eq!(
        status.import_tool.as_deref(),
        Some("burp_get_proxy_history")
    );
    let exchanges = adapter.import(10).await.unwrap();
    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].method, "GET");
    assert_eq!(exchanges[0].status, Some(200));
    assert!(
        exchanges[0]
            .request_headers
            .iter()
            .any(|(name, _)| name == "Cookie")
    );
    assert!(state.initialized.load(Ordering::Relaxed));
    let calls = state.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["params"]["name"], "burp_get_proxy_history");
    unsafe {
        std::env::remove_var("DARKBOX_BURP_MCP_URL");
        std::env::remove_var("DARKBOX_BURP_MCP_TOKEN");
    }
}
