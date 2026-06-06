use std::fs;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use codex_core::code_bridge::BridgeControlAction;
use codex_core::code_bridge::BridgeControlRequest;
use codex_core::code_bridge::META_FILE;
use codex_core::code_bridge::discover_bridge_targets;
use codex_core::code_bridge::load_subscription;
use codex_core::code_bridge::request_control;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn fake_bridge_round_trip_covers_subscription_and_control_result() -> Result<()> {
    let fake = FakeBridgeServer::start(FakeBridgeControlResponse::Success).await?;
    let workspace = TempDir::new()?;
    let code_dir = workspace.path().join(".code");
    fs::create_dir(&code_dir)?;
    fs::write(
        code_dir.join(META_FILE),
        serde_json::to_string(&json!({
            "url": fake.url,
            "secret": fake.secret,
            "workspacePath": workspace.path(),
            "heartbeatAt": chrono::Utc::now(),
        }))?,
    )?;
    fs::write(
        code_dir.join(codex_core::code_bridge::SUBSCRIPTION_FILE),
        r#"{
            "levels": ["info", "errors", "INFO"],
            "capabilities": ["control", "screenshot", "control"],
            "llmFilter": "off"
        }"#,
    )?;

    let mut targets = discover_bridge_targets(workspace.path()).await?;
    assert_eq!(targets.len(), 1);
    let target = targets.remove(0);
    assert!(!target.stale);

    let subscription = load_subscription(workspace.path()).await?;
    let outcome = request_control(
        &target,
        &subscription,
        BridgeControlRequest {
            id: "control-1".to_string(),
            action: BridgeControlAction::Ping,
            code: None,
            timeout_ms: Some(1_000),
            expect_result: Some(true),
        },
        Duration::from_secs(2),
    )
    .await?;

    let observed = fake.wait().await?;
    assert_eq!(outcome.delivered, 1);
    assert!(outcome.ok);
    assert_eq!(outcome.result, Some(json!({ "pong": true })));

    assert_eq!(observed.auth["type"], "auth");
    assert_eq!(observed.auth["role"], "consumer");
    assert_eq!(observed.auth["secret"], "bridge-secret");
    assert_eq!(observed.subscribe["type"], "subscribe");
    assert_eq!(observed.subscribe["levels"], json!(["errors", "info"]));
    assert_eq!(
        observed.subscribe["capabilities"],
        json!(["control", "screenshot"])
    );
    assert_eq!(observed.subscribe["llmFilter"], "off");
    assert!(observed.subscribe.get("llm_filter").is_none());
    assert_eq!(observed.control["type"], "control_request");
    assert_eq!(observed.control["id"], "control-1");
    assert_eq!(observed.control["action"], "ping");
    assert!(observed.control.get("method").is_none());
    Ok(())
}

#[tokio::test]
async fn fake_bridge_failed_control_result_is_returned_as_error() -> Result<()> {
    let fake = FakeBridgeServer::start(FakeBridgeControlResponse::Failure).await?;
    let workspace = TempDir::new()?;
    let code_dir = workspace.path().join(".code");
    fs::create_dir(&code_dir)?;
    fs::write(
        code_dir.join(META_FILE),
        serde_json::to_string(&json!({
            "url": fake.url,
            "secret": fake.secret,
            "workspacePath": workspace.path(),
            "heartbeatAt": chrono::Utc::now(),
        }))?,
    )?;

    let mut targets = discover_bridge_targets(workspace.path()).await?;
    let target = targets.remove(0);
    let subscription = load_subscription(workspace.path()).await?;
    let err = request_control(
        &target,
        &subscription,
        BridgeControlRequest {
            id: "control-1".to_string(),
            action: BridgeControlAction::Ping,
            code: None,
            timeout_ms: Some(1_000),
            expect_result: Some(true),
        },
        Duration::from_secs(2),
    )
    .await
    .expect_err("failed control result should surface as an error");

    let observed = fake.wait().await?;
    assert_eq!(observed.control["type"], "control_request");
    assert!(err.to_string().contains("bridge refused ping"));
    Ok(())
}

struct FakeBridgeServer {
    url: String,
    secret: String,
    observed_rx: oneshot::Receiver<Result<ObservedBridgeMessages>>,
    task: JoinHandle<()>,
}

impl FakeBridgeServer {
    async fn start(response: FakeBridgeControlResponse) -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .context("bind fake bridge server")?;
        let url = format!("ws://{}", listener.local_addr()?);
        let (observed_tx, observed_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            let observed = accept_one(listener, response).await;
            let _ = observed_tx.send(observed);
        });

        Ok(Self {
            url,
            secret: "bridge-secret".to_string(),
            observed_rx,
            task,
        })
    }

    async fn wait(self) -> Result<ObservedBridgeMessages> {
        let observed = self
            .observed_rx
            .await
            .context("fake bridge server should send observed messages")??;
        self.task.await.context("fake bridge server task should join")?;
        Ok(observed)
    }
}

struct ObservedBridgeMessages {
    auth: Value,
    subscribe: Value,
    control: Value,
}

#[derive(Clone, Copy)]
enum FakeBridgeControlResponse {
    Success,
    Failure,
}

async fn accept_one(
    listener: TcpListener,
    response: FakeBridgeControlResponse,
) -> Result<ObservedBridgeMessages> {
    let (stream, _) = listener.accept().await.context("accept fake bridge client")?;
    let mut websocket = accept_async(stream)
        .await
        .context("accept fake bridge websocket")?;

    let auth = read_text_json(&mut websocket).await?;
    websocket
        .send(Message::Text(json!({ "type": "auth_success" }).to_string().into()))
        .await
        .context("send auth success")?;

    let subscribe = read_text_json(&mut websocket).await?;
    websocket
        .send(Message::Text(json!({ "type": "subscribe_ack" }).to_string().into()))
        .await
        .context("send subscribe ack")?;

    let control = read_text_json(&mut websocket).await?;
    let id = control
        .get("id")
        .and_then(Value::as_str)
        .context("control request should include id")?;
    websocket
        .send(Message::Text(
            json!({ "type": "control_forwarded", "id": id, "delivered": 1 })
                .to_string()
                .into(),
        ))
        .await
        .context("send control forwarded")?;
    let result = match response {
        FakeBridgeControlResponse::Success => {
            json!({ "type": "control_result", "id": id, "ok": true, "result": { "pong": true } })
        }
        FakeBridgeControlResponse::Failure => {
            json!({ "type": "control_result", "id": id, "ok": false, "error": { "message": "bridge refused ping" } })
        }
    };
    websocket
        .send(Message::Text(result.to_string().into()))
        .await
        .context("send control result")?;

    Ok(ObservedBridgeMessages {
        auth,
        subscribe,
        control,
    })
}

async fn read_text_json<S>(websocket: &mut S) -> Result<Value>
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    while let Some(message) = websocket.next().await {
        match message? {
            Message::Text(text) => return Ok(serde_json::from_str(&text)?),
            Message::Close(_) => anyhow::bail!("fake bridge websocket closed early"),
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
    anyhow::bail!("fake bridge websocket ended before text message")
}
