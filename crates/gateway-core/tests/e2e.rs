use std::sync::Arc;
use oclaws_gateway_core::{GatewayServer, HttpServer};
use oclaws_config::settings::Gateway;
use oclaws_llm_core::providers::MockLlmProvider;

async fn start_server(mock: MockLlmProvider) -> String {
    let gateway = Gateway::default();
    let gs = Arc::new(GatewayServer::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = HttpServer::new(addr, Arc::new(gateway), gs)
        .with_llm_provider(Arc::new(mock));

    tokio::spawn(async move {
        axum::serve(listener, server.into_router()).await.unwrap();
    });

    format!("http://127.0.0.1:{}", addr.port())
}

#[tokio::test]
async fn test_e2e_health() {
    let base = start_server(MockLlmProvider::new()).await;
    let resp = reqwest::get(format!("{}/health", base)).await.unwrap();
    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn test_e2e_chat_completions() {
    let mock = MockLlmProvider::new();
    mock.queue_text("E2E works!");
    let base = start_server(mock).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1/chat/completions", base))
        .json(&serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hello"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let json: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(json["choices"][0]["message"]["content"], "E2E works!");
}

#[tokio::test]
async fn test_e2e_no_provider_returns_503() {
    let gateway = Gateway::default();
    let gs = Arc::new(GatewayServer::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = HttpServer::new(addr, Arc::new(gateway), gs);
    tokio::spawn(async move {
        axum::serve(listener, server.into_router()).await.unwrap();
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://127.0.0.1:{}/v1/chat/completions", addr.port()))
        .json(&serde_json::json!({
            "model": "test",
            "messages": [{"role": "user", "content": "hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 503);
}
