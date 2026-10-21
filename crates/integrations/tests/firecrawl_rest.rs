use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use darkbox_integrations::FirecrawlAdapter;
use serde_json::{Value, json};

#[derive(Clone)]
struct Mock;

async fn map(State(_): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(body["url"], "https://example.test/");
    Json(json!({
        "success": true,
        "links": [
            "https://example.test/a",
            {"url": "https://example.test/b"},
            "/c"
        ]
    }))
}

async fn scrape(State(_): State<Mock>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(body["url"], "https://example.test/a");
    assert!(
        body["formats"]
            .as_array()
            .unwrap()
            .contains(&json!("markdown"))
    );
    Json(json!({
        "success": true,
        "data": {
            "markdown": "# Rendered\n\nThis came from the managed browser.",
            "links": ["https://example.test/b"],
            "metadata": {
                "title": "Rendered",
                "sourceURL": "https://example.test/a",
                "statusCode": 200
            }
        }
    }))
}

#[tokio::test]
async fn maps_and_scrapes_through_rest() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .route("/map", post(map))
        .route("/scrape", post(scrape))
        .with_state(Mock);
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // SAFETY: this test binary owns these variables and runs one test.
    unsafe {
        std::env::set_var("DARKBOX_FIRECRAWL_API_KEY", "test-key");
        std::env::set_var("DARKBOX_FIRECRAWL_BASE_URL", format!("http://{address}"));
        std::env::remove_var("DARKBOX_FIRECRAWL_MCP_URL");
        std::env::remove_var("DARKBOX_FIRECRAWL_MCP_TOKEN");
    }
    let adapter = FirecrawlAdapter::from_env().unwrap();
    assert!(!adapter.is_mcp());
    let links = adapter
        .map("https://example.test/", 10, None)
        .await
        .unwrap();
    assert_eq!(links.len(), 3);
    assert!(links.contains(&"https://example.test/c".to_string()));
    let page = adapter
        .scrape("https://example.test/a", "Verify the managed browser.")
        .await
        .unwrap();
    assert_eq!(page.title.as_deref(), Some("Rendered"));
    assert!(page.markdown.contains("managed browser"));
    assert_eq!(page.status_code, Some(200));
    unsafe {
        std::env::remove_var("DARKBOX_FIRECRAWL_API_KEY");
        std::env::remove_var("DARKBOX_FIRECRAWL_BASE_URL");
    }
}
