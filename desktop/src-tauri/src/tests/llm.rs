#[test]
fn llm_models_endpoint_appends_models_to_provider_base_path() {
    assert_eq!(
        llm_models_endpoint("https://api.openai.com/v1/", "openai_chat")
            .unwrap()
            .as_str(),
        "https://api.openai.com/v1/models"
    );
    assert_eq!(
        llm_models_endpoint(
            "https://gateway.example/v1/chat/completions?trace=1",
            "openai_chat",
        )
        .unwrap()
        .as_str(),
        "https://gateway.example/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("https://gateway.example/v1/responses", "openai_responses")
            .unwrap()
            .as_str(),
        "https://gateway.example/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("http://127.0.0.1:11434/v1/models", "openai_chat")
            .unwrap()
            .as_str(),
        "http://127.0.0.1:11434/v1/models"
    );
    assert_eq!(
        llm_models_endpoint("https://api.deepseek.com/anthropic", "anthropic_messages",)
            .unwrap()
            .as_str(),
        "https://api.deepseek.com/models"
    );
    assert!(!llm_models_uses_anthropic_auth(
        "https://api.deepseek.com/anthropic",
        "anthropic_messages",
    ));
    assert!(llm_models_uses_anthropic_auth(
        "https://gateway.example/anthropic",
        "anthropic_messages",
    ));
    assert!(llm_models_endpoint("http://models.example.test/v1", "openai_chat").is_err());
    assert!(llm_models_endpoint("https://user:secret@gateway.example/v1", "openai_chat",).is_err());
    assert!(llm_models_endpoint("file:///tmp/models", "openai_chat").is_err());
}

#[test]
fn llm_inference_endpoint_supports_protocol_switching_and_full_urls() {
    assert_eq!(
        llm_inference_endpoint(
            "https://gateway.example/v1/chat/completions",
            "openai_responses",
            false,
        )
        .unwrap()
        .as_str(),
        "https://gateway.example/v1/responses"
    );
    assert_eq!(
        llm_inference_endpoint(
            "https://api.deepseek.com/anthropic",
            "anthropic_messages",
            false,
        )
        .unwrap()
        .as_str(),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
    assert_eq!(
        llm_inference_endpoint(
            "https://gateway.example/custom/generate?tenant=1",
            "openai_chat",
            true,
        )
        .unwrap()
        .as_str(),
        "https://gateway.example/custom/generate?tenant=1"
    );
    assert!(llm_inference_endpoint(
        "https://gateway.example/custom/generate",
        "anthropic_messages",
        true,
    )
    .is_err());
    assert_eq!(
        llm_inference_endpoint(
            "https://gateway.example/custom/v1/messages",
            "anthropic_messages",
            true,
        )
        .unwrap()
        .as_str(),
        "https://gateway.example/custom/v1/messages"
    );
    assert!(
        llm_inference_endpoint("http://models.example.test/v1", "openai_chat", false,).is_err()
    );
}

#[test]
fn llm_models_uses_anthropic_key_for_anthropic_protocol() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock model catalog endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept model catalog request");
        let mut request = vec![0_u8; 16_384];
        let read = stream
            .read(&mut request)
            .expect("read model catalog request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        assert!(request.starts_with("get /v1/models "));
        assert!(request.contains("x-api-key: anthropic-key"));
        assert!(!request.contains("authorization: bearer anthropic-key"));

        let body = r#"{"data":[{"id":"deepseek-v4-flash"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
        .expect("write model catalog response");
    });

    let result = tauri::async_runtime::block_on(api_llm_models(json!({
        "provider": "anthropic-compatible",
        "base_url": format!("http://{address}/v1"),
        "api_key": "anthropic-key",
        "timeout_seconds": 5,
    })))
    .expect("model catalog request should succeed");

    assert_eq!(result["count"], 1);
    server
        .join()
        .expect("mock model catalog server should finish");
}

#[test]
fn llm_models_uses_bearer_auth_for_openai_protocol() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock OpenAI catalog endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept OpenAI catalog request");
        let mut request = vec![0_u8; 16_384];
        let read = stream
            .read(&mut request)
            .expect("read OpenAI catalog request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        assert!(request.starts_with("get /v1/models "));
        assert!(request.contains("authorization: bearer openai-key"));
        assert!(!request.contains("x-api-key: openai-key"));

        let body = r#"{"data":[{"id":"relay-chat"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
        .expect("write OpenAI catalog response");
    });

    let result = tauri::async_runtime::block_on(api_llm_models(json!({
        "provider": "openai-compatible",
        "api_format": "openai_chat",
        "base_url": format!("http://{address}/v1"),
        "api_key": "openai-key",
        "timeout_seconds": 5,
    })))
    .expect("OpenAI model catalog request should succeed");

    assert_eq!(result["count"], 1);
    server
        .join()
        .expect("mock OpenAI catalog server should finish");
}

#[test]
fn llm_test_calls_anthropic_v1_messages_with_anthropic_key() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock Anthropic endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept Anthropic request");
        let mut request = vec![0_u8; 16_384];
        let read = stream.read(&mut request).expect("read Anthropic request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        assert!(request.starts_with("post /anthropic/v1/messages "));
        assert!(request.contains("x-api-key: anthropic-key"));
        assert!(!request.contains("authorization: bearer anthropic-key"));

        let body = r#"{"content":[{"type":"text","text":"OK"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
        .expect("write Anthropic response");
    });

    let result = tauri::async_runtime::block_on(api_llm_test(json!({
        "base_url": format!("http://{address}/anthropic"),
        "api_key": "anthropic-key",
        "model": "deepseek-v4-flash",
        "api_format": "anthropic_messages",
        "endpoint_mode": "base_url",
        "timeout_seconds": 5,
    })))
    .expect("Anthropic connection test should succeed");

    assert_eq!(result["ok"], true);
    server.join().expect("mock Anthropic server should finish");
}

#[test]
fn llm_test_uses_anthropic_protocol_for_legacy_anthropic_provider() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind legacy Anthropic endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept legacy Anthropic request");
        let mut request = vec![0_u8; 16_384];
        let read = stream
            .read(&mut request)
            .expect("read legacy Anthropic request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        assert!(request.starts_with("post /anthropic/v1/messages "));
        assert!(request.contains("x-api-key: legacy-key"));
        assert!(!request.contains("authorization: bearer legacy-key"));

        let body = r#"{"content":[{"type":"text","text":"OK"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
        .expect("write legacy Anthropic response");
    });

    let result = tauri::async_runtime::block_on(api_llm_test(json!({
        "provider": "anthropic-compatible",
        "base_url": format!("http://{address}/anthropic"),
        "api_key": "legacy-key",
        "model": "legacy-model",
        "timeout_seconds": 5,
    })))
    .expect("legacy Anthropic connection test should succeed");

    assert_eq!(result["api_format"], "anthropic_messages");
    server
        .join()
        .expect("legacy Anthropic server should finish");
}

#[test]
fn llm_test_calls_openai_chat_with_bearer_auth() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock OpenAI chat endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept OpenAI chat request");
        let mut request = vec![0_u8; 16_384];
        let read = stream.read(&mut request).expect("read OpenAI chat request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        assert!(request.starts_with("post /v1/chat/completions "));
        assert!(request.contains("authorization: bearer openai-key"));
        assert!(!request.contains("x-api-key: openai-key"));

        let body = r#"{"choices":[{"message":{"content":"OK"}}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
        .expect("write OpenAI chat response");
    });

    let result = tauri::async_runtime::block_on(api_llm_test(json!({
        "base_url": format!("http://{address}/v1"),
        "api_key": "openai-key",
        "model": "relay-chat",
        "api_format": "openai_chat",
        "endpoint_mode": "base_url",
        "timeout_seconds": 5,
    })))
    .expect("OpenAI chat connection test should succeed");

    assert_eq!(result["ok"], true);
    server
        .join()
        .expect("mock OpenAI chat server should finish");
}

#[test]
fn llm_test_calls_openai_responses_with_bearer_auth() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock OpenAI responses endpoint");
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept OpenAI responses request");
        let mut request = vec![0_u8; 16_384];
        let read = stream
            .read(&mut request)
            .expect("read OpenAI responses request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        assert!(request.starts_with("post /v1/responses "));
        assert!(request.contains("authorization: bearer responses-key"));
        assert!(!request.contains("x-api-key: responses-key"));

        let body = r#"{"output_text":"OK"}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        )
        .expect("write OpenAI responses response");
    });

    let result = tauri::async_runtime::block_on(api_llm_test(json!({
        "base_url": format!("http://{address}/v1"),
        "api_key": "responses-key",
        "model": "relay-responses",
        "api_format": "openai_responses",
        "endpoint_mode": "base_url",
        "timeout_seconds": 5,
    })))
    .expect("OpenAI responses connection test should succeed");

    assert_eq!(result["ok"], true);
    server
        .join()
        .expect("mock OpenAI responses server should finish");
}

#[test]
fn parse_llm_model_options_supports_openai_and_ollama_shapes() {
    let openai = parse_llm_model_options(&json!({
        "data": [
            {"id": "gpt-5.4", "owned_by": "openai"},
            {"id": "gpt-5.4", "owned_by": "duplicate"},
            {"id": "gpt-5.4-mini", "display_name": "GPT 5.4 Mini"}
        ]
    }));
    assert_eq!(openai.len(), 2);
    assert_eq!(openai[0]["id"].as_str(), Some("gpt-5.4"));
    assert_eq!(openai[1]["name"].as_str(), Some("GPT 5.4 Mini"));

    let ollama = parse_llm_model_options(&json!({
        "models": [
            {"name": "qwen2.5:7b", "model": "qwen2.5:7b"},
            "deepseek-r1:8b"
        ]
    }));
    assert_eq!(ollama.len(), 2);
    assert_eq!(ollama[0]["id"].as_str(), Some("qwen2.5:7b"));
    assert_eq!(ollama[1]["id"].as_str(), Some("deepseek-r1:8b"));
}

#[test]
fn llm_models_http_error_explains_auth_and_version_failures() {
    assert!(llm_models_http_error(401, b"{}", "sk-test").contains("API 密钥"));
    assert!(llm_models_http_error(404, b"{}", "sk-test").contains("/v1"));
    assert!(
        llm_models_http_error(429, br#"{"error":{"message":"rate limited"}}"#, "sk-test")
            .contains("rate limited")
    );
    let redacted = llm_models_http_error(
        500,
        br#"{"error":{"message":"upstream echoed sk-secret-token"}}"#,
        "sk-secret-token",
    );
    assert!(!redacted.contains("sk-secret-token"));
    assert!(redacted.contains("[已隐藏密钥]"));
}
