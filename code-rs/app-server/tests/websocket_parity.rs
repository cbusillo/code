use std::net::SocketAddr;
use std::time::Duration;

use code_app_server::AppServerTransport;
use code_app_server::run_main_with_transport;
use code_common::CliConfigOverrides;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sse_response(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("content-type", "text/event-stream")
        .set_body_string(body)
}

async fn connect_with_retry(
    url: &str,
) -> WebSocketStream<MaybeTlsStream<TcpStream>> {
    let mut attempts = 0;
    loop {
        match connect_async(url).await {
            Ok((stream, _)) => return stream,
            Err(err) => {
                attempts += 1;
                assert!(attempts < 40, "failed to connect to {url}: {err}");
                sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

async fn send_request(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    request: Value,
) {
    ws.send(Message::Text(request.to_string().into()))
        .await
        .expect("request should send");
}

async fn recv_response_for_id(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: i64,
) -> Value {
    loop {
        let message = ws
            .next()
            .await
            .expect("websocket should stay open")
            .expect("websocket frame should decode");
        let Message::Text(text) = message else {
            continue;
        };
        let json: Value = serde_json::from_str(text.as_ref()).expect("response must be JSON");
        let json_id = json.get("id").and_then(Value::as_i64);
        if json_id == Some(id) {
            return json;
        }
    }
}

async fn recv_error_for_id(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    id: i64,
) -> Value {
    loop {
        let message = ws
            .next()
            .await
            .expect("websocket should stay open")
            .expect("websocket frame should decode");
        let Message::Text(text) = message else {
            continue;
        };
        let json: Value = serde_json::from_str(text.as_ref()).expect("response must be JSON");
        let json_id = json.get("id").and_then(Value::as_i64);
        if json_id == Some(id) && json.get("error").is_some() {
            return json;
        }
    }
}

async fn recv_notification_for_method(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    method: &str,
) -> Value {
    loop {
        let message = ws
            .next()
            .await
            .expect("websocket should stay open")
            .expect("websocket frame should decode");
        let Message::Text(text) = message else {
            continue;
        };
        let json: Value = serde_json::from_str(text.as_ref()).expect("notification must be JSON");
        let json_method = json.get("method").and_then(Value::as_str);
        if json_method == Some(method) {
            return json;
        }
    }
}

async fn assert_no_message(
    ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    wait_for: Duration,
) {
    match tokio::time::timeout(wait_for, ws.next()).await {
        Ok(Some(Ok(frame))) => {
            panic!("unexpected frame while waiting for silence: {frame:?}");
        }
        Ok(Some(Err(err))) => {
            panic!("unexpected websocket read error while waiting for silence: {err}");
        }
        Ok(None) => {
            panic!("websocket closed unexpectedly while waiting for silence");
        }
        Err(_) => {}
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_user_agent_is_connection_scoped() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().expect("resolve bound address");
    drop(listener);

    let server_handle = tokio::spawn(async move {
        run_main_with_transport(
            None,
            CliConfigOverrides::default(),
            AppServerTransport::WebSocket { bind_address: addr },
        )
        .await
    });

    let url = format!("ws://{addr}");
    let mut client_a = connect_with_retry(&url).await;
    let mut client_b = connect_with_retry(&url).await;

    send_request(
        &mut client_a,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "client-a",
                    "version": "1.0.0"
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client_a, 1).await;

    send_request(
        &mut client_b,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "client-b",
                    "version": "2.0.0"
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client_b, 1).await;

    send_request(
        &mut client_a,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getUserAgent"
        }),
    )
    .await;
    let response_a = recv_response_for_id(&mut client_a, 2).await;

    send_request(
        &mut client_b,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getUserAgent"
        }),
    )
    .await;
    let response_b = recv_response_for_id(&mut client_b, 2).await;

    let user_agent_a = response_a
        .get("result")
        .and_then(|result| result.get("userAgent"))
        .and_then(Value::as_str)
        .expect("client a should receive user agent");
    let user_agent_b = response_b
        .get("result")
        .and_then(|result| result.get("userAgent"))
        .and_then(Value::as_str)
        .expect("client b should receive user agent");

    assert!(
        user_agent_a.contains("(client-a; 1.0.0)"),
        "client a user-agent should include its own suffix: {user_agent_a}"
    );
    assert!(
        user_agent_b.contains("(client-b; 2.0.0)"),
        "client b user-agent should include its own suffix: {user_agent_b}"
    );
    assert!(
        !user_agent_a.contains("client-b; 2.0.0"),
        "client a user-agent should not include client b suffix: {user_agent_a}"
    );
    assert!(
        !user_agent_b.contains("client-a; 1.0.0"),
        "client b user-agent should not include client a suffix: {user_agent_b}"
    );

    client_a.close(None).await.expect("client a should close");
    client_b.close(None).await.expect("client b should close");
    server_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_routes_handshake_and_same_id_requests_per_connection() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().expect("resolve bound address");
    drop(listener);

    let server_handle = tokio::spawn(async move {
        run_main_with_transport(
            None,
            CliConfigOverrides::default(),
            AppServerTransport::WebSocket { bind_address: addr },
        )
        .await
    });

    let url = format!("ws://{addr}");
    let mut client_a = connect_with_retry(&url).await;
    let mut client_b = connect_with_retry(&url).await;

    send_request(
        &mut client_a,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "client-a",
                    "version": "1.0.0"
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client_a, 1).await;

    // Initialize responses are request-scoped and should not leak to other clients.
    assert_no_message(&mut client_b, Duration::from_millis(200)).await;

    send_request(
        &mut client_b,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "getUserAgent"
        }),
    )
    .await;
    let pre_init_error = recv_error_for_id(&mut client_b, 2).await;
    let pre_init_message = pre_init_error
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .expect("error message should exist");
    assert!(
        pre_init_message.contains("Not initialized"),
        "unexpected pre-init error: {pre_init_message}"
    );

    send_request(
        &mut client_b,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "client-b",
                    "version": "2.0.0"
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client_b, 3).await;

    // Same request id on different connections should route independently.
    send_request(
        &mut client_a,
        json!({
            "jsonrpc": "2.0",
            "id": 77,
            "method": "getUserAgent"
        }),
    )
    .await;
    send_request(
        &mut client_b,
        json!({
            "jsonrpc": "2.0",
            "id": 77,
            "method": "getUserAgent"
        }),
    )
    .await;

    let response_a = recv_response_for_id(&mut client_a, 77).await;
    let response_b = recv_response_for_id(&mut client_b, 77).await;

    assert!(response_a.get("result").is_some(), "client a should get response");
    assert!(response_b.get("result").is_some(), "client b should get response");

    client_a.close(None).await.expect("client a should close");
    client_b.close(None).await.expect("client b should close");
    server_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_supports_thread_native_requests_and_listener_notifications() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().expect("resolve bound address");
    drop(listener);

    let server_handle = tokio::spawn(async move {
        run_main_with_transport(
            None,
            CliConfigOverrides::default(),
            AppServerTransport::WebSocket { bind_address: addr },
        )
        .await
    });

    let url = format!("ws://{addr}");
    let mut client = connect_with_retry(&url).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "shared-session-web-ui-parity",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client, 1).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "thread/list",
            "params": {}
        }),
    )
    .await;
    let thread_list = recv_response_for_id(&mut client, 2).await;
    assert!(
        thread_list
            .get("result")
            .and_then(|result| result.get("data"))
            .and_then(Value::as_array)
            .is_some(),
        "thread/list result missing data array: {thread_list}"
    );

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "thread/start",
            "params": {}
        }),
    )
    .await;
    let thread_start = recv_response_for_id(&mut client, 3).await;
    let thread = thread_start
        .get("result")
        .and_then(|result| result.get("thread"))
        .expect("thread/start result should include thread");
    let thread_id = thread
        .get("id")
        .and_then(Value::as_str)
        .expect("thread/start response missing thread id");
    let model = thread_start
        .get("result")
        .and_then(|result| result.get("model"))
        .and_then(Value::as_str)
        .expect("thread/start response missing model");
    assert!(!thread_id.is_empty(), "thread/start returned empty thread id");
    assert!(!model.is_empty(), "thread/start returned empty model");

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "thread/loaded/list",
            "params": {}
        }),
    )
    .await;
    let loaded_threads = recv_response_for_id(&mut client, 4).await;
    assert!(
        loaded_threads
            .get("result")
            .and_then(|result| result.get("data"))
            .and_then(Value::as_array)
            .is_some_and(|data| data.iter().any(|value| value.as_str() == Some(thread_id))),
        "thread/loaded/list should include started thread: {loaded_threads}"
    );

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "addConversationListener",
            "params": {
                "conversationId": thread_id,
                "experimentalRawEvents": false
            }
        }),
    )
    .await;
    let listener_response = recv_response_for_id(&mut client, 5).await;
    assert!(
        listener_response
            .get("result")
            .and_then(|result| result.get("subscriptionId"))
            .and_then(Value::as_str)
            .is_some(),
        "listener response missing subscription id: {listener_response}"
    );

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 6,
            "method": "turn/start",
            "params": {
                "threadId": thread_id,
                "input": [
                    {
                        "type": "text",
                        "text": "Say hello",
                        "textElements": []
                    }
                ]
            }
        }),
    )
    .await;
    let turn_start = recv_response_for_id(&mut client, 6).await;
    assert!(
        turn_start.get("result").is_some(),
        "turn/start response should succeed: {turn_start}"
    );

    let notification = recv_notification_for_method(&mut client, "turn/started").await;
    let params = notification
        .get("params")
        .expect("turn/started notification should include params");
    assert_eq!(
        params
            .get("threadId")
            .and_then(Value::as_str)
            .expect("turn/started thread id should be present"),
        thread_id
    );
    assert_eq!(
        params
            .get("turn")
            .and_then(|turn| turn.get("status"))
            .and_then(Value::as_str)
            .expect("turn/started turn status should be present"),
        "inProgress"
    );

    client.close(None).await.expect("client should close");
    server_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_emits_turn_completed_for_mocked_responses_provider() {
    let mock_provider = MockServer::start().await;
    let message_item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "id": "msg-1",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "done"}],
        }
    });
    let completed = json!({
        "type": "response.completed",
        "response": {
            "id": "resp-1",
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": 0
            }
        }
    });
    let body = format!(
        "event: response.output_item.done\ndata: {message_item}\n\n\
event: response.completed\ndata: {completed}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(sse_response(body))
        .up_to_n_times(1)
        .mount(&mock_provider)
        .await;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().expect("resolve bound address");
    drop(listener);

    let cli_config_overrides = CliConfigOverrides {
        raw_overrides: vec![
            "model_provider=test_openai".to_string(),
            format!("openai_base_url=\"{}/v1\"", mock_provider.uri()),
            "model_providers.test_openai.name=\"Test OpenAI\"".to_string(),
            format!("model_providers.test_openai.base_url=\"{}/v1\"", mock_provider.uri()),
            "model_providers.test_openai.wire_api=\"responses\"".to_string(),
            "model_providers.test_openai.requires_openai_auth=false".to_string(),
            "model=gpt-5.1-codex".to_string(),
        ],
    };

    let server_handle = tokio::spawn(async move {
        run_main_with_transport(
            None,
            cli_config_overrides,
            AppServerTransport::WebSocket { bind_address: addr },
        )
        .await
    });

    let url = format!("ws://{addr}");
    let mut client = connect_with_retry(&url).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "shared-session-web-ui-completion-parity",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client, 1).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "thread/start",
            "params": {}
        }),
    )
    .await;
    let thread_start = recv_response_for_id(&mut client, 2).await;
    let thread_id = thread_start
        .get("result")
        .and_then(|result| result.get("thread"))
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .expect("thread/start response missing thread id");

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "addConversationListener",
            "params": {
                "conversationId": thread_id,
                "experimentalRawEvents": false
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client, 3).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "turn/start",
            "params": {
                "threadId": thread_id,
                "input": [
                    {
                        "type": "text",
                        "text": "Say hello",
                        "textElements": []
                    }
                ]
            }
        }),
    )
    .await;
    let turn_start = recv_response_for_id(&mut client, 4).await;
    assert!(
        turn_start.get("result").is_some(),
        "turn/start response should succeed: {turn_start}"
    );

    let started = recv_notification_for_method(&mut client, "turn/started").await;
    let started_params = started
        .get("params")
        .expect("turn/started notification should include params");
    assert_eq!(
        started_params
            .get("threadId")
            .and_then(Value::as_str)
            .expect("turn/started thread id should be present"),
        thread_id
    );

    let completed = recv_notification_for_method(&mut client, "turn/completed").await;
    let completed_params = completed
        .get("params")
        .expect("turn/completed notification should include params");
    assert_eq!(
        completed_params
            .get("threadId")
            .and_then(Value::as_str)
            .expect("turn/completed thread id should be present"),
        thread_id
    );
    assert_eq!(
        completed_params
            .get("turn")
            .and_then(|turn| turn.get("status"))
            .and_then(Value::as_str)
            .expect("turn/completed turn status should be present"),
        "completed"
    );

    client.close(None).await.expect("client should close");
    server_handle.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn websocket_supports_turn_interrupt_for_active_thread() {
    let mock_provider = MockServer::start().await;
    let message_item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "message",
            "id": "msg-1",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "done"}],
        }
    });
    let completed = json!({
        "type": "response.completed",
        "response": {
            "id": "resp-1",
            "usage": {
                "input_tokens": 0,
                "input_tokens_details": null,
                "output_tokens": 0,
                "output_tokens_details": null,
                "total_tokens": 0
            }
        }
    });
    let body = format!(
        "event: response.output_item.done\ndata: {message_item}\n\n\
event: response.completed\ndata: {completed}\n\n",
    );
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(sse_response(body).set_delay(Duration::from_secs(2)))
        .up_to_n_times(1)
        .mount(&mock_provider)
        .await;

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().expect("resolve bound address");
    drop(listener);

    let cli_config_overrides = CliConfigOverrides {
        raw_overrides: vec![
            "model_provider=test_openai".to_string(),
            format!("openai_base_url=\"{}/v1\"", mock_provider.uri()),
            "model_providers.test_openai.name=\"Test OpenAI\"".to_string(),
            format!("model_providers.test_openai.base_url=\"{}/v1\"", mock_provider.uri()),
            "model_providers.test_openai.wire_api=\"responses\"".to_string(),
            "model_providers.test_openai.requires_openai_auth=false".to_string(),
            "model=gpt-5.1-codex".to_string(),
        ],
    };

    let server_handle = tokio::spawn(async move {
        run_main_with_transport(
            None,
            cli_config_overrides,
            AppServerTransport::WebSocket { bind_address: addr },
        )
        .await
    });

    let url = format!("ws://{addr}");
    let mut client = connect_with_retry(&url).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "shared-session-web-ui-interrupt-parity",
                    "version": "0.1.0"
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client, 1).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "thread/start",
            "params": {}
        }),
    )
    .await;
    let thread_start = recv_response_for_id(&mut client, 2).await;
    let thread_id = thread_start
        .get("result")
        .and_then(|result| result.get("thread"))
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .expect("thread/start response missing thread id");

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "addConversationListener",
            "params": {
                "conversationId": thread_id,
                "experimentalRawEvents": false
            }
        }),
    )
    .await;
    let _ = recv_response_for_id(&mut client, 3).await;

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "turn/start",
            "params": {
                "threadId": thread_id,
                "input": [
                    {
                        "type": "text",
                        "text": "Say hello",
                        "textElements": []
                    }
                ]
            }
        }),
    )
    .await;
    let turn_start = recv_response_for_id(&mut client, 4).await;
    let turn_id = turn_start
        .get("result")
        .and_then(|result| result.get("turn"))
        .and_then(|turn| turn.get("id"))
        .and_then(Value::as_str)
        .expect("turn/start response should include turn id");

    let started = recv_notification_for_method(&mut client, "turn/started").await;
    let started_params = started
        .get("params")
        .expect("turn/started notification should include params");
    assert_eq!(
        started_params
            .get("threadId")
            .and_then(Value::as_str)
            .expect("turn/started thread id should be present"),
        thread_id
    );

    send_request(
        &mut client,
        json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "turn/interrupt",
            "params": {
                "threadId": thread_id,
                "turnId": turn_id
            }
        }),
    )
    .await;
    let interrupt_response = recv_response_for_id(&mut client, 5).await;
    assert!(
        interrupt_response.get("result").is_some(),
        "turn/interrupt response should succeed: {interrupt_response}"
    );

    let completed = recv_notification_for_method(&mut client, "turn/completed").await;
    let completed_params = completed
        .get("params")
        .expect("turn/completed notification should include params");
    assert_eq!(
        completed_params
            .get("threadId")
            .and_then(Value::as_str)
            .expect("turn/completed thread id should be present"),
        thread_id
    );
    assert_eq!(
        completed_params
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .expect("turn/completed turn id should be present"),
        turn_id
    );
    assert_eq!(
        completed_params
            .get("turn")
            .and_then(|turn| turn.get("status"))
            .and_then(Value::as_str)
            .expect("turn/completed turn status should be present"),
        "interrupted"
    );

    client.close(None).await.expect("client should close");
    server_handle.abort();
}
