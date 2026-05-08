use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use code_core::config::RemoteInboxConfig;
use code_core::protocol::CustomToolCallBeginEvent;
use code_core::protocol::CustomToolCallEndEvent;
use code_core::protocol::ExecCommandBeginEvent;
use code_core::protocol::ExecCommandEndEvent;
use code_core::protocol::ExecApprovalRequestEvent;
use code_core::protocol::FileChange;
use code_core::protocol::ImageGenerationBeginEvent;
use code_core::protocol::ImageGenerationEndEvent;
use code_core::protocol::McpToolCallBeginEvent;
use code_core::protocol::McpToolCallEndEvent;
use code_core::protocol::PatchApplyBeginEvent;
use code_core::protocol::PatchApplyEndEvent;
use code_core::protocol::TurnDiffEvent;
use code_protocol::request_user_input::RequestUserInputEvent;
use futures::SinkExt;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde_json::Value;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::header::AUTHORIZATION;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::app_event::AppEvent;
use crate::app_event::Redacted;
use crate::app_event_sender::AppEventSender;
use crate::remote_inbox::protocol::ClientMessage;
use crate::remote_inbox::protocol::CommandAck;
use crate::remote_inbox::protocol::CommandReject;
use crate::remote_inbox::protocol::RemoteApprovalDecisionAck;
use crate::remote_inbox::protocol::RemoteApprovalDecisionReject;
use crate::remote_inbox::protocol::RemoteApprovalRequest;
use crate::remote_inbox::protocol::RemoteCommandKind;
use crate::remote_inbox::protocol::RemoteRequestUserInput;
use crate::remote_inbox::protocol::RemoteUserMessage;
use crate::remote_inbox::protocol::ServerMessage;
use crate::remote_inbox::protocol::SessionHeartbeat;
use crate::remote_inbox::protocol::SessionHello;
use crate::remote_inbox::protocol::SessionOrigin;
use crate::remote_inbox::protocol::SessionStatusEvent;
use crate::remote_inbox::protocol::RemoteTurnStep;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RECONNECT_DELAY: Duration = Duration::from_secs(5);
const COMMAND_ACCEPT_TIMEOUT: Duration = Duration::from_secs(30);
const APPROVAL_DECISION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_PROCESSED_COMMAND_IDS: usize = 1024;

type PendingCommandFuture = Pin<Box<dyn Future<Output = PendingCommandResult> + Send>>;
type LatestStatusSnapshot = Arc<Mutex<Option<ClientMessage>>>;

#[derive(Debug, Clone)]
pub(crate) struct RemoteInboxSession {
    pub session_id: String,
    pub session_epoch: String,
    pub cwd: String,
    pub branch: Option<String>,
    pub pid: u32,
    pub origin: Option<SessionOrigin>,
}

pub(crate) struct RemoteInboxClientHandle {
    handles: Vec<tokio::task::JoinHandle<()>>,
    status_txs: Vec<tokio::sync::mpsc::UnboundedSender<ClientMessage>>,
    timeline_status_txs: Vec<tokio::sync::mpsc::UnboundedSender<ClientMessage>>,
    session_id: String,
    session_epoch: String,
    latest_status_snapshot: LatestStatusSnapshot,
    timeline_enabled: bool,
}

struct RemoteInboxClientSink {
    handle: tokio::task::JoinHandle<()>,
    status_tx: tokio::sync::mpsc::UnboundedSender<ClientMessage>,
    timeline_enabled: bool,
}

impl Drop for RemoteInboxClientHandle {
    fn drop(&mut self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

impl RemoteInboxClientHandle {
    fn from_sinks(
        session_id: String,
        session_epoch: String,
        latest_status_snapshot: LatestStatusSnapshot,
        sinks: Vec<RemoteInboxClientSink>,
    ) -> Self {
        let mut handles = Vec::with_capacity(sinks.len());
        let mut status_txs = Vec::with_capacity(sinks.len());
        let mut timeline_status_txs = Vec::new();
        let mut timeline_enabled = false;

        for sink in sinks {
            if sink.timeline_enabled {
                timeline_enabled = true;
                timeline_status_txs.push(sink.status_tx.clone());
            }
            handles.push(sink.handle);
            status_txs.push(sink.status_tx);
        }

        Self {
            handles,
            status_txs,
            timeline_status_txs,
            session_id,
            session_epoch,
            latest_status_snapshot,
            timeline_enabled,
        }
    }

    pub(crate) fn send_waiting_for_model(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            None,
            Some("Waiting for model".to_string()),
            None,
        )));
    }

    pub(crate) fn send_turn_started(&self, turn_id: &str) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            Some(turn_id),
            Some("Turn started".to_string()),
            None,
        )));
    }

    pub(crate) fn send_user_message(&self, message: &str) {
        if message.trim().is_empty() {
            return;
        }
        self.send_status(ClientMessage::UserMessage(RemoteUserMessage {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            turn_id: None,
            message: message.to_string(),
        }));
    }

    pub(crate) fn send_turn_complete(&self, turn_id: &str, assistant_message: Option<String>) {
        self.send_status(ClientMessage::TurnComplete(self.status_event(
            Some(turn_id),
            Some("Turn complete. Replies here will start the next turn.".to_string()),
            assistant_message,
        )));
    }

    pub(crate) fn send_exec_command_begin(&self, turn_id: &str, event: &ExecCommandBeginEvent) {
        if !self.timeline_enabled {
            return;
        }

        let command = shell_command_label(&event.command);
        self.send_status(ClientMessage::TurnStep(self.turn_step(
            turn_id,
            &event.call_id,
            "tool",
            "Shell command",
            &command,
            "running",
        )));
    }

    pub(crate) fn send_exec_command_end(
        &self,
        turn_id: &str,
        call_id: &str,
        command: &[String],
        event: &ExecCommandEndEvent,
    ) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step(
            turn_id,
            call_id,
            "tool",
            "Shell command",
            &exec_command_end_detail(command, event),
            if event.exit_code == 0 { "completed" } else { "error" },
        )));
    }

    pub(crate) fn send_mcp_tool_begin(&self, turn_id: &str, event: &McpToolCallBeginEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step(
            turn_id,
            &event.call_id,
            "tool",
            "MCP tool",
            &tool_begin_detail(&mcp_tool_label(event), event.invocation.arguments.as_ref()),
            "running",
        )));
    }

    pub(crate) fn send_mcp_tool_end(&self, turn_id: &str, event: &McpToolCallEndEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step(
            turn_id,
            &event.call_id,
            "tool",
            "MCP tool",
            &tool_end_detail(
                &format!("{}.{}", event.invocation.server, event.invocation.tool),
                event.duration,
                event.result.as_ref().err().map(String::as_str),
            ),
            if event.result.is_ok() { "completed" } else { "error" },
        )));
    }

    pub(crate) fn send_custom_tool_begin(&self, turn_id: &str, event: &CustomToolCallBeginEvent) {
        if !self.timeline_enabled || !should_mirror_custom_tool(&event.tool_name) {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step(
            turn_id,
            &event.call_id,
            "tool",
            &custom_tool_title(&event.tool_name),
            &tool_begin_detail(&event.tool_name, event.parameters.as_ref()),
            "running",
        )));
    }

    pub(crate) fn send_custom_tool_end(&self, turn_id: &str, event: &CustomToolCallEndEvent) {
        if !self.timeline_enabled || !should_mirror_custom_tool(&event.tool_name) {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step(
            turn_id,
            &event.call_id,
            "tool",
            &custom_tool_title(&event.tool_name),
            &tool_end_detail(
                &event.tool_name,
                event.duration,
                event.result.as_ref().err().map(String::as_str),
            ),
            if event.result.is_ok() { "completed" } else { "error" },
        )));
    }

    pub(crate) fn send_patch_apply_begin(&self, turn_id: &str, event: &PatchApplyBeginEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step_with_id(
            turn_id,
            &patch_step_id(turn_id, &event.call_id),
            "diff",
            "Patch apply",
            &patch_begin_detail(event),
            "running",
        )));
    }

    pub(crate) fn send_patch_apply_end(&self, turn_id: &str, event: &PatchApplyEndEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step_with_id(
            turn_id,
            &patch_step_id(turn_id, &event.call_id),
            "diff",
            if event.success { "Patch applied" } else { "Patch failed" },
            &patch_end_detail(event),
            if event.success { "completed" } else { "error" },
        )));
    }

    pub(crate) fn send_turn_diff(&self, turn_id: &str, event: &TurnDiffEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step_with_id(
            turn_id,
            &format!("{turn_id}:diff"),
            "diff",
            "Turn diff",
            &bounded_detail(&event.unified_diff, 4_000),
            "completed",
        )));
    }

    pub(crate) fn send_image_generation_begin(&self, turn_id: &str, event: &ImageGenerationBeginEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step_with_id(
            turn_id,
            &artifact_step_id(turn_id, &event.call_id),
            "artifact",
            "Image generation",
            "Generating image artifact.",
            "running",
        )));
    }

    pub(crate) fn send_image_generation_end(&self, turn_id: &str, event: &ImageGenerationEndEvent) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step_with_id(
            turn_id,
            &artifact_step_id(turn_id, &event.call_id),
            "artifact",
            if event.saved_path.is_some() { "Image artifact" } else { "Image generation" },
            &image_generation_detail(event),
            if event.saved_path.is_some() { "completed" } else { "error" },
        )));
    }

    pub(crate) fn send_turn_error(&self, turn_id: &str, message: &str) {
        if !self.timeline_enabled {
            return;
        }

        self.send_status(ClientMessage::TurnStep(self.turn_step_with_id(
            turn_id,
            &format!("{turn_id}:error"),
            "error",
            "Turn error",
            &bounded_detail(message, 1_000),
            "error",
        )));
    }

    pub(crate) fn send_error(&self, message: &str) {
        self.send_status(ClientMessage::Error(
            self.status_event(None, Some(message.to_string()), None),
        ));
    }

    pub(crate) fn send_turn_aborted(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            None,
            Some("Turn aborted".to_string()),
            None,
        )));
    }

    pub(crate) fn send_compaction_started(&self) {
        self.send_status(ClientMessage::StatusChanged(self.status_event(
            None,
            Some("Compacting context".to_string()),
            None,
        )));
    }

    pub(crate) fn send_exec_approval_request(&self, request: &ExecApprovalRequestEvent) {
        self.send_status(ClientMessage::ApprovalRequest(RemoteApprovalRequest {
            approval_id: request.effective_approval_id(),
            call_id: request.call_id.clone(),
            turn_id: request.turn_id.clone(),
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            command: request.command.clone(),
            cwd: request.cwd.display().to_string(),
            reason: request.reason.clone(),
        }));
    }

    pub(crate) fn send_request_user_input(&self, request: &RequestUserInputEvent) {
        self.send_status(ClientMessage::RequestUserInput(RemoteRequestUserInput {
            call_id: request.call_id.clone(),
            turn_id: request.turn_id.clone(),
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            questions: request.questions.clone(),
        }));
    }

    fn status_event(
        &self,
        turn_id: Option<&str>,
        message: Option<String>,
        assistant_message: Option<String>,
    ) -> SessionStatusEvent {
        SessionStatusEvent {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            turn_id: turn_id.map(str::to_string),
            message,
            assistant_message,
        }
    }

    fn turn_step(
        &self,
        turn_id: &str,
        call_id: &str,
        kind: &str,
        title: &str,
        detail: &str,
        state: &str,
    ) -> RemoteTurnStep {
        self.turn_step_with_id(
            turn_id,
            &format!("{turn_id}:tool:{call_id}"),
            kind,
            title,
            detail,
            state,
        )
    }

    fn turn_step_with_id(
        &self,
        turn_id: &str,
        step_id: &str,
        kind: &str,
        title: &str,
        detail: &str,
        state: &str,
    ) -> RemoteTurnStep {
        RemoteTurnStep {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            turn_id: turn_id.to_string(),
            step_id: step_id.to_string(),
            kind: kind.to_string(),
            title: title.to_string(),
            detail: detail.to_string(),
            state: state.to_string(),
        }
    }

    fn send_status(&self, message: ClientMessage) {
        if is_replayable_status_snapshot(&message) {
            match self.latest_status_snapshot.lock() {
                Ok(mut latest) => *latest = Some(message.clone()),
                Err(err) => tracing::warn!("failed to store remote inbox status snapshot: {err}"),
            }
        }

        let target_txs = if matches!(message, ClientMessage::TurnStep(_)) {
            &self.timeline_status_txs
        } else {
            &self.status_txs
        };

        for status_tx in target_txs {
            if let Err(err) = status_tx.send(message.clone()) {
                tracing::warn!("failed to queue remote inbox status event: {err}");
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn test_remote_inbox_client_handle(
) -> (
    RemoteInboxClientHandle,
    tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
) {
    let (status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
    let latest_status_snapshot = Arc::new(Mutex::new(None));
    (
        RemoteInboxClientHandle {
            handles: Vec::new(),
            status_txs: vec![status_tx.clone()],
            timeline_status_txs: vec![status_tx],
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot,
            timeline_enabled: true,
        },
        status_rx,
    )
}

pub(crate) fn spawn_remote_inbox_client(
    config: RemoteInboxConfig,
    session: RemoteInboxSession,
    app_event_tx: AppEventSender,
) -> Option<RemoteInboxClientHandle> {
    if !config.enabled {
        tracing::info!("remote inbox client spawn skipped because it is disabled");
        return None;
    }

    let session_id = session.session_id.clone();
    let session_epoch = session.session_epoch.clone();
    let latest_status_snapshot = Arc::new(Mutex::new(None));
    let mut sinks = Vec::new();

    if let Some(bridge_url) = config
        .bridge_url
        .clone()
        .filter(|url| !url.trim().is_empty())
    {
        let bridge_endpoint = redact_bridge_url_for_logs(&bridge_url);
        tracing::info!(
            session_id = %session_id,
            bridge_endpoint = %bridge_endpoint,
            "spawning websocket remote inbox client"
        );
        sinks.push(spawn_websocket_remote_inbox_client(
            bridge_url,
            config.clone(),
            session.clone(),
            app_event_tx.clone(),
            latest_status_snapshot.clone(),
        ));
    }

    if sinks.is_empty() {
        tracing::warn!("remote inbox is enabled but bridge_url is not configured");
        return None;
    }

    Some(RemoteInboxClientHandle::from_sinks(
        session_id,
        session_epoch,
        latest_status_snapshot,
        sinks,
    ))
}

fn spawn_websocket_remote_inbox_client(
    bridge_url: String,
    config: RemoteInboxConfig,
    session: RemoteInboxSession,
    app_event_tx: AppEventSender,
    latest_status_snapshot: LatestStatusSnapshot,
) -> RemoteInboxClientSink {
    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
    let client_latest_status_snapshot = latest_status_snapshot.clone();
    let handle = tokio::spawn(async move {
        let mut processed_command_ids = ProcessedCommandIds::default();
        loop {
            if let Err(err) = connect_once(
                &bridge_url,
                &config,
                &session,
                &app_event_tx,
                &mut processed_command_ids,
                &mut status_rx,
                &client_latest_status_snapshot,
            )
            .await
            {
                let bridge_endpoint = redact_bridge_url_for_logs(&bridge_url);
                tracing::warn!(
                    session_id = %session.session_id,
                    bridge_endpoint = %bridge_endpoint,
                    "remote inbox connection ended: {err}"
                );
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    });
    RemoteInboxClientSink {
        handle,
        status_tx,
        timeline_enabled: true,
    }
}

impl RemoteInboxSession {
    pub(crate) fn new(
        session_id: String,
        session_epoch: String,
        cwd: std::path::PathBuf,
    ) -> Self {
        let branch = current_branch(&cwd);
        Self {
            session_id,
            session_epoch,
            cwd: cwd.display().to_string(),
            branch,
            pid: std::process::id(),
            origin: session_origin_from_env(),
        }
    }
}

fn session_origin_from_env() -> Option<SessionOrigin> {
    let kind = env_value("EVERY_CODE_SESSION_ORIGIN")
        .or_else(|| env_value("EVERY_CODE_ORIGIN"))
        .or_else(|| env_value("LAUNCHPLANE_EVERY_CODE_ORIGIN"))
        .or_else(|| env_value("EVERY_CODE_REQUEST_ID").map(|_| "every_code".to_string()))?;

    Some(SessionOrigin {
        kind,
        request_id: env_value("EVERY_CODE_REQUEST_ID"),
        repository: env_value("EVERY_CODE_REPOSITORY"),
        issue_number: env_value("EVERY_CODE_ISSUE_NUMBER").and_then(|value| value.parse().ok()),
        issue_url: env_value("EVERY_CODE_ISSUE_URL"),
    })
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

impl RemoteInboxSession {
    fn hello(&self, config: &RemoteInboxConfig) -> SessionHello {
        SessionHello {
            session_id: self.session_id.clone(),
            session_epoch: self.session_epoch.clone(),
            host_id: config.host_id.clone().filter(|id| !id.trim().is_empty()),
            host_label: config
                .host_label
                .clone()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| "Every Code".to_string()),
            cwd: self.cwd.clone(),
            branch: self.branch.clone(),
            pid: self.pid,
            origin: self.origin.clone(),
        }
    }
}

fn shell_command_label(command: &[String]) -> String {
    shlex::try_join(command.iter().map(String::as_str)).unwrap_or_else(|_| command.join(" "))
}

fn exec_command_end_detail(command: &[String], event: &ExecCommandEndEvent) -> String {
    let command_label = if command.is_empty() {
        event.call_id.clone()
    } else {
        shell_command_label(command)
    };
    let duration = event.duration.as_secs_f32();
    let mut detail = format!(
        "{command_label}\nExited with code {} after {:.1}s.",
        event.exit_code, duration
    );
    let output = exec_output_tail(event);

    if !output.is_empty() {
        detail.push_str("\n");
        detail.push_str(&output);
    }

    detail
}

fn exec_output_tail(event: &ExecCommandEndEvent) -> String {
    let output = if event.stderr.trim().is_empty() {
        event.stdout.trim()
    } else {
        event.stderr.trim()
    };

    const MAX_OUTPUT_CHARS: usize = 500;
    if output.chars().count() <= MAX_OUTPUT_CHARS {
        return output.to_string();
    }

    let tail: String = output
        .chars()
        .rev()
        .take(MAX_OUTPUT_CHARS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("...{tail}")
}

fn mcp_tool_label(event: &McpToolCallBeginEvent) -> String {
    format!("{}.{}", event.invocation.server, event.invocation.tool)
}

fn should_mirror_custom_tool(tool_name: &str) -> bool {
    !matches!(tool_name, "wait" | "kill")
}

fn custom_tool_title(tool_name: &str) -> String {
    if tool_name.starts_with("browser_") {
        "Browser tool".to_string()
    } else if tool_name.starts_with("agent_") {
        "Agent tool".to_string()
    } else {
        "Tool".to_string()
    }
}

fn tool_begin_detail(label: &str, parameters: Option<&Value>) -> String {
    match parameters {
        Some(parameters) => format!("{label}\n{}", compact_json_value(parameters)),
        None => label.to_string(),
    }
}

fn tool_end_detail(label: &str, duration: Duration, error: Option<&str>) -> String {
    let mut detail = format!("{label}\n{} after {:.1}s.", if error.is_some() { "Failed" } else { "Completed" }, duration.as_secs_f32());

    if let Some(error) = error.map(str::trim).filter(|error| !error.is_empty()) {
        detail.push_str("\n");
        detail.push_str(&bounded_detail(error, 500));
    }

    detail
}

fn patch_step_id(turn_id: &str, call_id: &str) -> String {
    format!("{turn_id}:patch:{call_id}")
}

fn artifact_step_id(turn_id: &str, call_id: &str) -> String {
    format!("{turn_id}:artifact:{call_id}")
}

fn patch_begin_detail(event: &PatchApplyBeginEvent) -> String {
    let approval = if event.auto_approved {
        "Applying auto-approved patch."
    } else {
        "Applying approved patch."
    };
    let changes = summarize_file_changes(&event.changes);
    if changes.is_empty() {
        approval.to_string()
    } else {
        format!("{approval}\n{changes}")
    }
}

fn patch_end_detail(event: &PatchApplyEndEvent) -> String {
    let mut detail = if event.success {
        "Patch applied.".to_string()
    } else {
        "Patch failed.".to_string()
    };

    if let Some(stdout) = non_empty_section("stdout", &event.stdout, 1_000) {
        detail.push('\n');
        detail.push_str(&stdout);
    }
    if let Some(stderr) = non_empty_section("stderr", &event.stderr, 1_000) {
        detail.push('\n');
        detail.push_str(&stderr);
    }

    detail
}

fn image_generation_detail(event: &ImageGenerationEndEvent) -> String {
    let mut detail = format!("Status: {}", event.status);

    if let Some(saved_path) = event.saved_path.as_ref() {
        detail.push_str("\nSaved: ");
        detail.push_str(&saved_path.display().to_string());
    } else {
        detail.push_str("\nNo saved artifact path was reported.");
    }

    if let Some(prompt) = event
        .revised_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        detail.push_str("\nPrompt: ");
        detail.push_str(&bounded_detail(prompt, 1_000));
    }

    detail
}

fn summarize_file_changes(changes: &HashMap<PathBuf, FileChange>) -> String {
    let mut lines = changes
        .iter()
        .map(|(path, change)| match change {
            FileChange::Update {
                move_path: Some(move_path),
                ..
            } => format!("Moved {} -> {}", path.display(), move_path.display()),
            _ => format!("{} {}", file_change_label(change), path.display()),
        })
        .collect::<Vec<_>>();
    lines.sort();
    bounded_detail(&lines.join("\n"), 2_000)
}

fn file_change_label(change: &FileChange) -> &'static str {
    match change {
        FileChange::Add { .. } => "Added",
        FileChange::Delete => "Deleted",
        FileChange::Update { .. } => "Updated",
    }
}

fn non_empty_section(label: &str, value: &str, max_chars: usize) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(format!("{label}:\n{}", bounded_detail(trimmed, max_chars)))
}

fn compact_json_value(value: &Value) -> String {
    bounded_detail(&value.to_string(), 500)
}

fn bounded_detail(value: &str, max_chars: usize) -> String {
    truncate_chars(&strip_control_chars(value), max_chars)
}

fn strip_control_chars(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let tail: String = value.chars().take(max_chars).collect();
    format!("{tail}...")
}

fn queue_remote_new_session(
    app_event_tx: &AppEventSender,
    command_id: String,
    issued_by: Option<String>,
) -> bool {
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    app_event_tx.send_with_result(AppEvent::RemoteInboxNewSession {
        command_id,
        issued_by,
        response_tx: Redacted(response_tx),
    })
}

async fn connect_once(
    bridge_url: &str,
    config: &RemoteInboxConfig,
    session: &RemoteInboxSession,
    app_event_tx: &AppEventSender,
    processed_command_ids: &mut ProcessedCommandIds,
    status_rx: &mut tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
    latest_status_snapshot: &LatestStatusSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut request = bridge_url.into_client_request()?;
    if let Some(token) = config.token.as_deref().filter(|token| !token.trim().is_empty()) {
        let header = HeaderValue::from_str(&format!("Bearer {token}"))?;
        request.headers_mut().insert(AUTHORIZATION, header);
    }

    let (ws, _) = connect_async(request).await?;
    let bridge_endpoint = redact_bridge_url_for_logs(bridge_url);
    tracing::info!(
        session_id = %session.session_id,
        bridge_endpoint = %bridge_endpoint,
        "remote inbox connected"
    );
    let (mut write, mut read) = ws.split();

    send_json(&mut write, &ClientMessage::Hello(session.hello(config))).await?;
    send_latest_status_snapshot_if_idle(&mut write, status_rx, latest_status_snapshot).await?;

    let mut heartbeat = tokio::time::interval(HEARTBEAT_INTERVAL);
    let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();
    let mut pending_command_ids = HashSet::new();
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                send_json(
                    &mut write,
                    &ClientMessage::Heartbeat(SessionHeartbeat {
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                    }),
                ).await?;
            }
            maybe_status = status_rx.recv() => {
                let Some(status) = maybe_status else {
                    break;
                };
                send_json(&mut write, &status).await?;
            }
            maybe_message = read.next() => {
                let Some(message) = maybe_message else {
                    break;
                };
                let message = message?;
                if message.is_text() {
                    handle_text_message(
                        message.to_text()?,
                        session,
                        app_event_tx,
                        &mut write,
                        processed_command_ids,
                        &mut pending_command_ids,
                        &mut pending_command_acceptances,
                    )
                    .await?;
                } else if message.is_close() {
                    break;
                }
            }
            pending_result = pending_command_acceptances.next(), if !pending_command_acceptances.is_empty() => {
                if let Some(result) = pending_result {
                    pending_command_ids.remove(&result.command_id);

                    match result.outcome {
                        PendingCommandOutcome::Accepted => {
                            processed_command_ids.insert(result.command_id.clone());
                            send_json(
                                &mut write,
                                &ClientMessage::CommandAck(CommandAck {
                                    command_id: result.command_id,
                                    session_id: result.session_id,
                                    session_epoch: result.session_epoch,
                                }),
                            )
                            .await?;
                        }
                        PendingCommandOutcome::Rejected { reason, cache_command_id } => {
                            if cache_command_id {
                                processed_command_ids.insert(result.command_id.clone());
                            }
                            send_json(
                                &mut write,
                                &ClientMessage::CommandReject(CommandReject {
                                    command_id: Some(result.command_id),
                                    session_id: result.session_id,
                                    session_epoch: result.session_epoch,
                                    reason,
                                }),
                            )
                            .await?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn redact_bridge_url_for_logs(bridge_url: &str) -> String {
    match url::Url::parse(bridge_url) {
        Ok(mut url) => {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => "<invalid bridge_url>".to_string(),
    }
}

async fn send_latest_status_snapshot_if_idle<S>(
    write: &mut S,
    status_rx: &tokio::sync::mpsc::UnboundedReceiver<ClientMessage>,
    latest_status_snapshot: &LatestStatusSnapshot,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    if !status_rx.is_empty() {
        return Ok(());
    }
    let latest_status = match latest_status_snapshot.lock() {
        Ok(latest) => latest.clone(),
        Err(err) => {
            tracing::warn!("failed to load remote inbox status snapshot: {err}");
            None
        }
    };
    if let Some(message) = latest_status {
        send_json(write, &message).await?;
    }
    Ok(())
}

fn is_replayable_status_snapshot(message: &ClientMessage) -> bool {
    matches!(
        message,
        ClientMessage::StatusChanged(_)
            | ClientMessage::TurnComplete(_)
            | ClientMessage::Error(_)
            | ClientMessage::ApprovalRequest(_)
            | ClientMessage::RequestUserInput(_)
    )
}

async fn handle_text_message<S>(
    text: &str,
    session: &RemoteInboxSession,
    app_event_tx: &AppEventSender,
    write: &mut S,
    processed_command_ids: &mut ProcessedCommandIds,
    pending_command_ids: &mut HashSet<String>,
    pending_command_acceptances: &mut FuturesUnordered<PendingCommandFuture>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    let parsed: ServerMessage = match serde_json::from_str(text) {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::warn!("failed to parse remote inbox message: {err}");
            return Ok(());
        }
    };

    match parsed {
        ServerMessage::Command(command) => {
            if command.session_id != session.session_id
                || command.session_epoch != session.session_epoch
            {
                send_json(
                    write,
                    &ClientMessage::CommandReject(CommandReject {
                        command_id: Some(command.command_id),
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "command targets a different session".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            if processed_command_ids.contains(&command.command_id) {
                tracing::info!(
                    command_id = command.command_id,
                    "acknowledging duplicate remote inbox command"
                );
                send_json(
                    write,
                    &ClientMessage::CommandAck(CommandAck {
                        command_id: command.command_id,
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                    }),
                )
                .await?;
                return Ok(());
            }

            if pending_command_ids.contains(&command.command_id) {
                tracing::info!(
                    command_id = command.command_id,
                    "rejecting duplicate pending remote inbox command"
                );
                send_json(
                    write,
                    &ClientMessage::CommandReject(CommandReject {
                        command_id: Some(command.command_id),
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "command is already pending acceptance".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            match command.kind {
                RemoteCommandKind::Reply => {
                    let Some(reply) = command.text.filter(|text| !text.trim().is_empty()) else {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command.command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "reply command did not include text".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    };

                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(AppEvent::RemoteInboxReply {
                        command_id: command_id.clone(),
                        text: reply,
                        issued_by: command.issued_by,
                        response_tx: Redacted(response_tx),
                    });
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::ContinueAutonomously => {
                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(
                        AppEvent::RemoteInboxContinueAutonomously {
                            command_id: command_id.clone(),
                            issued_by: command.issued_by,
                            response_tx: Redacted(response_tx),
                        },
                    );
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::PauseCurrentTurn => {
                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(AppEvent::RemoteInboxPauseCurrentTurn {
                        command_id: command_id.clone(),
                        issued_by: command.issued_by,
                        response_tx: Redacted(response_tx),
                    });
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::NewSession => {
                    let command_id = command.command_id.clone();
                    processed_command_ids.insert(command_id.clone());
                    send_json(
                        write,
                        &ClientMessage::CommandAck(CommandAck {
                            command_id: command_id.clone(),
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;

                    let accepted = queue_remote_new_session(
                        app_event_tx,
                        command_id.clone(),
                        command.issued_by,
                    );
                    if !accepted {
                        tracing::warn!(command_id, "failed to queue remote inbox new_session after ack");
                    }
                }
                RemoteCommandKind::EndSession => {
                    send_json(
                        write,
                        &ClientMessage::CommandAck(CommandAck {
                            command_id: command.command_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;
                    if !app_event_tx.send_with_result(AppEvent::ExitRequest) {
                        tracing::warn!("failed to queue exit request after remote end_session command");
                    }
                }
                RemoteCommandKind::RequestUserInputResponse => {
                    let Some(turn_id) = command.turn_id.filter(|turn_id| !turn_id.trim().is_empty()) else {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command.command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "request_user_input response did not include turn_id"
                                    .to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    };
                    let Some(response) = command.response else {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command.command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "request_user_input response did not include answers"
                                    .to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    };

                    let command_id = command.command_id.clone();
                    let (response_tx, response_rx) = tokio::sync::oneshot::channel();
                    let accepted = app_event_tx.send_with_result(
                        AppEvent::RemoteInboxRequestUserInputAnswer {
                            command_id: command_id.clone(),
                            call_id: command.call_id,
                            turn_id,
                            response,
                            issued_by: command.issued_by,
                            response_tx: Redacted(response_tx),
                        },
                    );
                    if !accepted {
                        send_json(
                            write,
                            &ClientMessage::CommandReject(CommandReject {
                                command_id: Some(command_id),
                                session_id: session.session_id.clone(),
                                session_epoch: session.session_epoch.clone(),
                                reason: "app event channel is closed".to_string(),
                            }),
                        )
                        .await?;
                        return Ok(());
                    }

                    pending_command_ids.insert(command_id.clone());
                    pending_command_acceptances.push(wait_for_command_acceptance(
                        command_id,
                        session.session_id.clone(),
                        session.session_epoch.clone(),
                        response_rx,
                    ));
                }
                RemoteCommandKind::StatusRequest => {
                    send_json(
                        write,
                        &ClientMessage::CommandAck(CommandAck {
                            command_id: command.command_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;
                }
            }
        }
        ServerMessage::ApprovalDecision(decision) => {
            if decision.session_id != session.session_id
                || decision.session_epoch != session.session_epoch
            {
                send_json(
                    write,
                    &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                        approval_id: decision.approval_id,
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "approval decision targets a different session".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            let (response_tx, response_rx) = tokio::sync::oneshot::channel();
            let approval_id = decision.approval_id.clone();
            let accepted = app_event_tx.send_with_result(AppEvent::RemoteInboxApprovalDecision {
                approval_id: approval_id.clone(),
                decision: decision.decision,
                response_tx: Redacted(response_tx),
            });
            if !accepted {
                send_json(
                    write,
                    &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                        approval_id,
                        session_id: session.session_id.clone(),
                        session_epoch: session.session_epoch.clone(),
                        reason: "app event channel is closed".to_string(),
                    }),
                )
                .await?;
                return Ok(());
            }

            match tokio::time::timeout(APPROVAL_DECISION_TIMEOUT, response_rx).await {
                Ok(Ok(Ok(()))) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionAck(RemoteApprovalDecisionAck {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                        }),
                    )
                    .await?;
                }
                Ok(Ok(Err(reason))) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                            reason,
                        }),
                    )
                    .await?;
                }
                Ok(Err(_)) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                            reason: "remote approval decision acceptance was canceled".to_string(),
                        }),
                    )
                    .await?;
                }
                Err(_) => {
                    send_json(
                        write,
                        &ClientMessage::ApprovalDecisionReject(RemoteApprovalDecisionReject {
                            approval_id,
                            session_id: session.session_id.clone(),
                            session_epoch: session.session_epoch.clone(),
                            reason: "timed out waiting for app to accept approval decision"
                                .to_string(),
                        }),
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}

async fn send_json<S, T>(
    write: &mut S,
    message: &T,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
    T: serde::Serialize,
{
    write
        .send(Message::Text(serde_json::to_string(message)?))
        .await?;
    Ok(())
}

fn wait_for_command_acceptance(
    command_id: String,
    session_id: String,
    session_epoch: String,
    response_rx: tokio::sync::oneshot::Receiver<Result<(), String>>,
) -> PendingCommandFuture {
    Box::pin(async move {
        let outcome = match tokio::time::timeout(COMMAND_ACCEPT_TIMEOUT, response_rx).await {
            Ok(Ok(Ok(()))) => PendingCommandOutcome::Accepted,
            Ok(Ok(Err(reason))) => PendingCommandOutcome::Rejected {
                reason,
                cache_command_id: false,
            },
            Ok(Err(_)) => PendingCommandOutcome::Rejected {
                reason: "remote inbox command acceptance was canceled".to_string(),
                cache_command_id: false,
            },
            Err(_) => PendingCommandOutcome::Rejected {
                reason: "timed out waiting for app to accept command".to_string(),
                cache_command_id: true,
            },
        };

        PendingCommandResult {
            command_id,
            session_id,
            session_epoch,
            outcome,
        }
    })
}

struct PendingCommandResult {
    command_id: String,
    session_id: String,
    session_epoch: String,
    outcome: PendingCommandOutcome,
}

enum PendingCommandOutcome {
    Accepted,
    Rejected {
        reason: String,
        cache_command_id: bool,
    },
}

fn current_branch(cwd: &std::path::Path) -> Option<String> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--abbrev-ref")
        .arg("HEAD")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!branch.is_empty()).then_some(branch)
}

#[derive(Default)]
struct ProcessedCommandIds {
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl ProcessedCommandIds {
    fn contains(&self, command_id: &str) -> bool {
        self.ids.contains(command_id)
    }

    fn insert(&mut self, command_id: String) {
        if !self.ids.insert(command_id.clone()) {
            return;
        }

        self.order.push_back(command_id);
        while self.order.len() > MAX_PROCESSED_COMMAND_IDS {
            if let Some(expired) = self.order.pop_front() {
                self.ids.remove(&expired);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;

    use futures::Sink;
    use serde_json::json;

    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[derive(Default)]
    struct RecordingSink {
        messages: Vec<Message>,
    }

    impl Sink<Message> for RecordingSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, item: Message) -> Result<(), Self::Error> {
            self.get_mut().messages.push(item);
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    fn test_session() -> RemoteInboxSession {
        RemoteInboxSession {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            cwd: "/tmp/project".to_string(),
            branch: Some("main".to_string()),
            pid: 42,
            origin: None,
        }
    }

    #[test]
    fn session_origin_is_read_from_every_code_env() {
        let _guard = ENV_MUTEX.lock().expect("env mutex");
        let backup = EnvBackup::new(&[
            "EVERY_CODE_SESSION_ORIGIN",
            "EVERY_CODE_ORIGIN",
            "LAUNCHPLANE_EVERY_CODE_ORIGIN",
            "EVERY_CODE_REQUEST_ID",
            "EVERY_CODE_REPOSITORY",
            "EVERY_CODE_ISSUE_NUMBER",
            "EVERY_CODE_ISSUE_URL",
        ]);
        backup.set("EVERY_CODE_SESSION_ORIGIN", "every_code");
        backup.set("EVERY_CODE_REQUEST_ID", "every-code-cbusillo-syo-67");
        backup.set("EVERY_CODE_REPOSITORY", "cbusillo/sellyouroutboard");
        backup.set("EVERY_CODE_ISSUE_NUMBER", "67");
        backup.set(
            "EVERY_CODE_ISSUE_URL",
            "https://github.com/cbusillo/sellyouroutboard/issues/67",
        );

        let origin = session_origin_from_env().expect("origin from env");

        assert_eq!(origin.kind, "every_code");
        assert_eq!(
            origin.request_id.as_deref(),
            Some("every-code-cbusillo-syo-67")
        );
        assert_eq!(origin.repository.as_deref(), Some("cbusillo/sellyouroutboard"));
        assert_eq!(origin.issue_number, Some(67));
        assert_eq!(
            origin.issue_url.as_deref(),
            Some("https://github.com/cbusillo/sellyouroutboard/issues/67")
        );
    }

    struct EnvBackup {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvBackup {
        fn new(keys: &[&'static str]) -> Self {
            let values = keys
                .iter()
                .map(|key| {
                    let previous = std::env::var(key).ok();
                    unsafe { std::env::remove_var(key) };
                    (*key, previous)
                })
                .collect();
            Self { values }
        }

        fn set(&self, key: &str, value: &str) {
            unsafe { std::env::set_var(key, value) };
        }
    }

    impl Drop for EnvBackup {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    fn app_event_sender() -> (AppEventSender, mpsc::Receiver<AppEvent>) {
        let (tx, rx) = mpsc::channel();
        (AppEventSender::new(tx), rx)
    }

    #[test]
    fn client_handle_fans_out_status_and_routes_timeline_to_enabled_sinks() {
        let (status_only_tx, mut status_only_rx) = tokio::sync::mpsc::unbounded_channel();
        let (timeline_tx, mut timeline_rx) = tokio::sync::mpsc::unbounded_channel();
        let handle = RemoteInboxClientHandle {
            handles: Vec::new(),
            status_txs: vec![status_only_tx, timeline_tx.clone()],
            timeline_status_txs: vec![timeline_tx],
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot: Arc::new(Mutex::new(None)),
            timeline_enabled: true,
        };

        handle.send_waiting_for_model();
        assert!(matches!(
            status_only_rx.try_recv().expect("status-only sink status"),
            ClientMessage::StatusChanged(_)
        ));
        assert!(matches!(
            timeline_rx
                .try_recv()
                .expect("timeline sink status"),
            ClientMessage::StatusChanged(_)
        ));

        handle.send_status(ClientMessage::TurnStep(RemoteTurnStep {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            turn_id: "turn-1".to_string(),
            step_id: "turn-1:tool:call-1".to_string(),
            kind: "tool".to_string(),
            title: "Shell command".to_string(),
            detail: "pnpm test".to_string(),
            state: "running".to_string(),
        }));

        assert!(status_only_rx.try_recv().is_err());
        assert!(matches!(
            timeline_rx
                .try_recv()
                .expect("timeline sink step"),
            ClientMessage::TurnStep(step) if step.step_id == "turn-1:tool:call-1"
        ));
    }

    #[test]
    fn client_handle_mirrors_custom_tool_timeline_steps_and_skips_internal_waiters() {
        let (handle, mut status_rx) = test_remote_inbox_client_handle();

        handle.send_custom_tool_begin(
            "turn-1",
            &CustomToolCallBeginEvent {
                call_id: "call-browser".to_string(),
                tool_name: "browser_open".to_string(),
                parameters: Some(json!({ "url": "http://127.0.0.1:3000" })),
            },
        );

        let step = match status_rx.try_recv().expect("browser tool step") {
            ClientMessage::TurnStep(step) => step,
            other => panic!("expected turn step, got {other:?}"),
        };
        assert_eq!(step.turn_id, "turn-1");
        assert_eq!(step.step_id, "turn-1:tool:call-browser");
        assert_eq!(step.title, "Browser tool");
        assert_eq!(step.state, "running");
        assert!(step.detail.contains("browser_open"));

        handle.send_custom_tool_begin(
            "turn-1",
            &CustomToolCallBeginEvent {
                call_id: "call-wait".to_string(),
                tool_name: "wait".to_string(),
                parameters: None,
            },
        );

        assert!(status_rx.try_recv().is_err());
    }

    #[test]
    fn client_handle_mirrors_patch_diff_and_error_timeline_steps() {
        let (handle, mut status_rx) = test_remote_inbox_client_handle();
        let mut changes = HashMap::new();
        changes.insert(
            PathBuf::from("src/main.rs"),
            FileChange::Update {
                unified_diff: "@@ -1 +1 @@".to_string(),
                move_path: None,
                original_content: "old".to_string(),
                new_content: "new".to_string(),
            },
        );

        handle.send_patch_apply_begin(
            "turn-1",
            &PatchApplyBeginEvent {
                call_id: "patch-1".to_string(),
                auto_approved: true,
                changes,
            },
        );
        let step = match status_rx.try_recv().expect("patch begin step") {
            ClientMessage::TurnStep(step) => step,
            other => panic!("expected turn step, got {other:?}"),
        };
        assert_eq!(step.step_id, "turn-1:patch:patch-1");
        assert_eq!(step.kind, "diff");
        assert_eq!(step.title, "Patch apply");
        assert_eq!(step.state, "running");
        assert!(step.detail.contains("Updated src/main.rs"));

        handle.send_patch_apply_end(
            "turn-1",
            &PatchApplyEndEvent {
                call_id: "patch-1".to_string(),
                stdout: "done".to_string(),
                stderr: "\u{0007}warning".to_string(),
                success: false,
            },
        );
        let step = match status_rx.try_recv().expect("patch end step") {
            ClientMessage::TurnStep(step) => step,
            other => panic!("expected turn step, got {other:?}"),
        };
        assert_eq!(step.step_id, "turn-1:patch:patch-1");
        assert_eq!(step.title, "Patch failed");
        assert_eq!(step.state, "error");
        assert!(!step.detail.contains('\u{0007}'));

        handle.send_turn_diff(
            "turn-1",
            &TurnDiffEvent {
                unified_diff: "diff\n".repeat(1_000),
            },
        );
        let step = match status_rx.try_recv().expect("turn diff step") {
            ClientMessage::TurnStep(step) => step,
            other => panic!("expected turn step, got {other:?}"),
        };
        assert_eq!(step.step_id, "turn-1:diff");
        assert_eq!(step.kind, "diff");
        assert!(step.detail.ends_with("..."));

        handle.send_turn_error("turn-1", "failure\u{0007}");
        let step = match status_rx.try_recv().expect("turn error step") {
            ClientMessage::TurnStep(step) => step,
            other => panic!("expected turn step, got {other:?}"),
        };
        assert_eq!(step.step_id, "turn-1:error");
        assert_eq!(step.kind, "error");
        assert_eq!(step.state, "error");
        assert_eq!(step.detail, "failure");
    }

    fn command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "reply",
            "text": "remote text",
            "issued_by": "123",
        })
        .to_string()
    }

    fn continue_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "continue_autonomously",
            "issued_by": "123",
        })
        .to_string()
    }

    fn pause_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "pause_current_turn",
            "issued_by": "123",
        })
        .to_string()
    }

    fn new_session_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "new_session",
            "issued_by": "123",
        })
        .to_string()
    }

    fn end_session_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "end_session",
            "issued_by": "123",
        })
        .to_string()
    }

    fn request_user_input_response_command_message(command_id: &str) -> String {
        json!({
            "type": "command",
            "command_id": command_id,
            "session_id": "session-1",
            "session_epoch": "epoch-1",
            "kind": "request_user_input_response",
            "call_id": "call-1",
            "turn_id": "turn-1",
            "response": {
                "answers": {
                    "mode": {"answers": ["Safe"]}
                }
            },
            "issued_by": "123",
        })
        .to_string()
    }

    fn sent_payload(sink: &RecordingSink, index: usize) -> serde_json::Value {
        let text = sink.messages[index].to_text().expect("text message");
        serde_json::from_str(text).expect("json message")
    }

    fn status_event(message: &str, assistant_message: Option<&str>) -> SessionStatusEvent {
        SessionStatusEvent {
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            turn_id: None,
            message: Some(message.to_string()),
            assistant_message: assistant_message.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn reply_command_sends_app_event_and_rejects_duplicate_while_pending() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("first command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxReply { command_id, text, issued_by, .. }
                if command_id == "cmd-1"
                    && text == "remote text"
                    && issued_by.as_deref() == Some("123")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);

        handle_text_message(
            &command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("duplicate command handled");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_reject",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "reason": "command is already pending acceptance",
            })
        );
    }

    #[tokio::test]
    async fn continue_command_sends_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &continue_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxContinueAutonomously { command_id, issued_by, .. }
                if command_id == "cmd-1" && issued_by.as_deref() == Some("123")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);
    }

    #[tokio::test]
    async fn end_session_command_acks_and_requests_exit() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &end_session_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        assert!(matches!(app_event_rx.try_recv(), Ok(AppEvent::ExitRequest)));
        assert!(pending_command_ids.is_empty());
        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_ack",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
            })
        );
    }

    #[tokio::test]
    async fn new_session_command_acks_before_sending_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &new_session_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_ack",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
            })
        );
        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxNewSession { command_id, issued_by, .. }
                if command_id == "cmd-1" && issued_by.as_deref() == Some("123")
        ));
        assert!(processed_command_ids.contains("cmd-1"));
        assert!(pending_command_ids.is_empty());
        assert!(pending_command_acceptances.is_empty());
    }

    #[tokio::test]
    async fn pause_current_turn_command_sends_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &pause_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxPauseCurrentTurn { command_id, issued_by, .. }
                if command_id == "cmd-1" && issued_by.as_deref() == Some("123")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);
    }

    #[tokio::test]
    async fn request_user_input_response_command_sends_app_event() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &request_user_input_response_command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("command handled");

        let event = app_event_rx.try_recv().expect("remote inbox event");
        assert!(matches!(
            event,
            AppEvent::RemoteInboxRequestUserInputAnswer {
                command_id,
                call_id,
                turn_id,
                issued_by,
                response,
                ..
            } if command_id == "cmd-1"
                && call_id.as_deref() == Some("call-1")
                && turn_id == "turn-1"
                && issued_by.as_deref() == Some("123")
                && response
                    .answers
                    .get("mode")
                    .and_then(|answer| answer.answers.first())
                    .map(String::as_str)
                    == Some("Safe")
        ));
        assert!(pending_command_ids.contains("cmd-1"));
        assert_eq!(sink.messages.len(), 0);
    }

    #[tokio::test]
    async fn latest_status_snapshot_replays_when_reconnect_has_no_queued_status() {
        let (_status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(Some(ClientMessage::TurnComplete(
            status_event("Turn complete", Some("Done.")),
        ))));
        let mut sink = RecordingSink::default();

        send_latest_status_snapshot_if_idle(&mut sink, &status_rx, &latest_status_snapshot)
            .await
            .expect("snapshot sent");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "turn_complete",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "message": "Turn complete",
                "assistant_message": "Done.",
            })
        );
    }

    #[tokio::test]
    async fn latest_status_snapshot_waits_when_reconnect_has_queued_status() {
        let (status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
        status_tx
            .send(ClientMessage::StatusChanged(status_event("Turn started", None)))
            .expect("queue status");
        let latest_status_snapshot = Arc::new(Mutex::new(Some(ClientMessage::TurnComplete(
            status_event("Turn complete", Some("Done.")),
        ))));
        let mut sink = RecordingSink::default();

        send_latest_status_snapshot_if_idle(&mut sink, &status_rx, &latest_status_snapshot)
            .await
            .expect("snapshot skipped");

        assert!(sink.messages.is_empty());
    }

    #[tokio::test]
    async fn latest_status_snapshot_replays_request_user_input_prompt() {
        let (_status_tx, status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(Some(ClientMessage::RequestUserInput(
            RemoteRequestUserInput {
                call_id: "call-1".to_string(),
                turn_id: "turn-1".to_string(),
                session_id: "session-1".to_string(),
                session_epoch: "epoch-1".to_string(),
                questions: vec![code_protocol::request_user_input::RequestUserInputQuestion {
                    id: "mode".to_string(),
                    header: "Build mode".to_string(),
                    question: "Choose a mode".to_string(),
                    is_other: false,
                    is_secret: false,
                    options: Some(vec![]),
                }],
            },
        ))));
        let mut sink = RecordingSink::default();

        send_latest_status_snapshot_if_idle(&mut sink, &status_rx, &latest_status_snapshot)
            .await
            .expect("request_user_input snapshot sent");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "request_user_input",
                "call_id": "call-1",
                "turn_id": "turn-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "questions": [{
                    "id": "mode",
                    "header": "Build mode",
                    "question": "Choose a mode",
                    "isOther": false,
                    "isSecret": false,
                    "options": [],
                }],
            })
        );
    }

    #[tokio::test]
    async fn send_request_user_input_updates_latest_status_snapshot() {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(None));
        let handle = RemoteInboxClientHandle {
            handles: Vec::new(),
            status_txs: vec![status_tx.clone()],
            timeline_status_txs: vec![status_tx],
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot: latest_status_snapshot.clone(),
            timeline_enabled: true,
        };

        handle.send_request_user_input(&RequestUserInputEvent {
            call_id: "call-1".to_string(),
            turn_id: "turn-1".to_string(),
            questions: vec![code_protocol::request_user_input::RequestUserInputQuestion {
                id: "mode".to_string(),
                header: "Build mode".to_string(),
                question: "Choose a mode".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![]),
            }],
        });

        let queued = status_rx.recv().await.expect("queued status");
        assert!(matches!(queued, ClientMessage::RequestUserInput(_)));
        let latest = latest_status_snapshot
            .lock()
            .expect("latest status snapshot")
            .clone();
        assert!(matches!(latest, Some(ClientMessage::RequestUserInput(_))));
    }

    #[tokio::test]
    async fn send_waiting_for_model_replaces_request_user_input_snapshot() {
        let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel();
        let latest_status_snapshot = Arc::new(Mutex::new(None));
        let handle = RemoteInboxClientHandle {
            handles: Vec::new(),
            status_txs: vec![status_tx.clone()],
            timeline_status_txs: vec![status_tx],
            session_id: "session-1".to_string(),
            session_epoch: "epoch-1".to_string(),
            latest_status_snapshot: latest_status_snapshot.clone(),
            timeline_enabled: true,
        };

        handle.send_request_user_input(&RequestUserInputEvent {
            call_id: "call-1".to_string(),
            turn_id: "turn-1".to_string(),
            questions: vec![code_protocol::request_user_input::RequestUserInputQuestion {
                id: "mode".to_string(),
                header: "Build mode".to_string(),
                question: "Choose a mode".to_string(),
                is_other: false,
                is_secret: false,
                options: Some(vec![]),
            }],
        });
        let _ = status_rx.recv().await.expect("queued prompt");

        handle.send_waiting_for_model();

        let queued = status_rx.recv().await.expect("queued waiting status");
        assert!(matches!(queued, ClientMessage::StatusChanged(_)));
        let latest = latest_status_snapshot
            .lock()
            .expect("latest status snapshot")
            .clone();
        assert!(matches!(latest, Some(ClientMessage::StatusChanged(_))));

        let (_status_tx, replay_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut sink = RecordingSink::default();
        send_latest_status_snapshot_if_idle(&mut sink, &replay_rx, &latest_status_snapshot)
            .await
            .expect("replay snapshot sent");
        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "status_changed",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
                "message": "Waiting for model",
            })
        );
    }

    #[tokio::test]
    async fn processed_duplicate_command_is_acknowledged_without_resubmitting() {
        let session = test_session();
        let (app_event_tx, app_event_rx) = app_event_sender();
        let mut sink = RecordingSink::default();
        let mut processed_command_ids = ProcessedCommandIds::default();
        processed_command_ids.insert("cmd-1".to_string());
        let mut pending_command_ids = HashSet::new();
        let mut pending_command_acceptances = FuturesUnordered::<PendingCommandFuture>::new();

        handle_text_message(
            &command_message("cmd-1"),
            &session,
            &app_event_tx,
            &mut sink,
            &mut processed_command_ids,
            &mut pending_command_ids,
            &mut pending_command_acceptances,
        )
        .await
        .expect("processed duplicate handled");

        assert_eq!(
            sent_payload(&sink, 0),
            json!({
                "type": "command_ack",
                "command_id": "cmd-1",
                "session_id": "session-1",
                "session_epoch": "epoch-1",
            })
        );
        assert!(app_event_rx.try_recv().is_err());
    }
}
