use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use darkbox_integrations::BurpAdapter;
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::sync::broadcast;

#[derive(Clone)]
struct Mock {
    tx: broadcast::Sender<String>,
}

async fn sse(
    State(state): State<Mock>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.tx.subscribe();
    let messages = futures::stream::unfold(receiver, |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(message) => {
                    return Some((
                        Ok::<_, Infallible>(Event::default().event("message").data(message)),
                        receiver,
                    ));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    let endpoint = futures::stream::once(async {
        Ok::<_, Infallible>(
            Event::default()
                .event("endpoint")
                .data("/message?sessionId=test"),
        )
    });
    Sse::new(endpoint.chain(messages))
}

async fn message(State(state): State<Mock>, Json(payload): Json<Value>) {
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    let method = payload
        .get("method")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": { "name": "burp-sse-mock", "version": "2026.1" },
            "capabilities": { "tools": {} }
        }),
        "tools/list" => json!({
            "tools": [{
                "name": "burp_get_proxy_history",
                "description": "proxy history",
                "inputSchema": {}
            }]
        }),
        "tools/call" => json!({
            "content": [{
                "type": "text",
                "text": "{\"data\":[{\"request\":{\"method\":\"GET\",\"url\":\"https://example.test/a\",\"headers\":{\"Host\":\"example.test\"},\"body\":null},\"response\":{\"status\":200,\"headers\":[],\"body\":\"ok\"}}]}"
            }]
        }),
        _ => json!({}),
    };
    if method == "notifications/initialized" {
        return;
    }
    let _ = state
        .tx
        .send(json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string());
}

#[tokio::test]
async fn imports_over_legacy_sse_transport() {
    let (tx, _) = broadcast::channel(32);
    let state = Mock { tx };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route(
            "/",
            post(|| async { axum::http::StatusCode::METHOD_NOT_ALLOWED }),
        )
        .route("/sse", get(sse))
        .route("/message", post(message))
        .with_state(state);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // SAFETY: this test binary owns the variables and runs one test.
    unsafe {
        std::env::set_var("DARKBOX_BURP_MCP_URL", format!("http://{address}"));
        std::env::set_var("DARKBOX_BURP_MCP_TOKEN", "test-token");
    }
    let adapter = BurpAdapter::from_env().unwrap();
    let status = adapter.probe().await.unwrap();
    assert!(status.available, "{status:?}");
    assert_eq!(status.server, "burp-sse-mock");
    let exchanges = adapter.import(5).await.unwrap();
    assert_eq!(exchanges.len(), 1);
    assert_eq!(exchanges[0].url, "https://example.test/a");
    unsafe {
        std::env::remove_var("DARKBOX_BURP_MCP_URL");
        std::env::remove_var("DARKBOX_BURP_MCP_TOKEN");
    }
}
