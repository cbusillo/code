use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use code_protocol::ThreadId;
use code_protocol::models::ContentItem;
use code_protocol::models::ResponseItem;
use code_protocol::protocol::AgentMessageEvent;
use code_protocol::protocol::EventMsg as ProtoEventMsg;
use code_protocol::protocol::RecordedEvent;
use code_protocol::protocol::RolloutItem;
use code_protocol::protocol::RolloutLine;
use code_protocol::protocol::SessionMeta;
use code_protocol::protocol::SessionMetaLine;
use code_protocol::protocol::SessionSource;
use code_protocol::protocol::UserMessageEvent;
use serde_json::Value;
use serde_json::json;
use uuid::Uuid;

fn app_server_bin() -> PathBuf {
    PathBuf::from(assert_cmd::cargo::cargo_bin!("code-app-server"))
}

fn run_jsonrpc_script(requests: &[Value]) -> BTreeMap<i64, Value> {
    run_jsonrpc_script_with_code_home(requests, None)
}

fn run_jsonrpc_script_with_code_home(
    requests: &[Value],
    code_home: Option<&Path>,
) -> BTreeMap<i64, Value> {
    let mut child = Command::new(app_server_bin());
    child
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(code_home) = code_home {
        child.env("CODE_HOME", code_home).env_remove("CODEX_HOME");
    }

    let mut child = child
        .spawn()
        .expect("failed to spawn code-app-server");

    let mut stdin = child.stdin.take().expect("child stdin is not piped");
    for request in requests {
        let line = serde_json::to_string(request).expect("request must be valid JSON");
        use std::io::Write as _;
        writeln!(stdin, "{line}").expect("failed to write JSON-RPC request line");
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .expect("failed waiting for code-app-server output");

    assert!(
        output.status.success(),
        "code-app-server exited with {status}; stderr:\n{stderr}",
        status = output.status,
        stderr = String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let message: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("invalid JSON-RPC line `{line}`: {e}"));
            let id = message
                .get("id")
                .and_then(Value::as_i64)
                ?;
            Some((id, message))
        })
        .collect()
}

struct TempCodeHome {
    path: PathBuf,
}

impl TempCodeHome {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("code-app-server-binary-smoke-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).expect("create temp code home");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCodeHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn create_session_fixture(code_home: &Path, id: &Uuid) -> PathBuf {
    let created_at = "2025-10-06T12:00:00Z";
    let last_event_at = "2025-10-06T12:01:00Z";
    let cwd = Path::new("/tmp/thread-native-smoke-project");
    let sessions_dir = code_home.join("sessions").join("2025").join("10").join("06");
    fs::create_dir_all(&sessions_dir).expect("create sessions dir");

    let path = sessions_dir.join(format!(
        "rollout-{}-{}.jsonl",
        created_at.replace(':', "-"),
        id
    ));

    let session_meta = SessionMeta {
        id: ThreadId::from_string(&id.to_string()).expect("valid thread id"),
        timestamp: created_at.to_string(),
        cwd: cwd.to_path_buf(),
        originator: "binary-smoke".to_string(),
        cli_version: "0.0.0-test".to_string(),
        source: SessionSource::Cli,
        model_provider: Some("openai".to_string()),
        base_instructions: None,
        dynamic_tools: None,
        forked_from_id: None,
    };

    let lines = vec![
        RolloutLine {
            timestamp: created_at.to_string(),
            item: RolloutItem::SessionMeta(SessionMetaLine {
                meta: session_meta,
                git: None,
            }),
        },
        RolloutLine {
            timestamp: last_event_at.to_string(),
            item: RolloutItem::Event(RecordedEvent {
                id: "event-0".to_string(),
                event_seq: 0,
                order: None,
                msg: ProtoEventMsg::UserMessage(UserMessageEvent {
                    message: "How does path-based resume work?".to_string(),
                    images: None,
                    local_images: vec![],
                    text_elements: vec![],
                }),
            }),
        },
        RolloutLine {
            timestamp: last_event_at.to_string(),
            item: RolloutItem::Event(RecordedEvent {
                id: "event-1".to_string(),
                event_seq: 1,
                order: None,
                msg: ProtoEventMsg::AgentMessage(AgentMessageEvent {
                    message: "By preferring the rollout path.".to_string(),
                }),
            }),
        },
        RolloutLine {
            timestamp: last_event_at.to_string(),
            item: RolloutItem::ResponseItem(ResponseItem::Message {
                id: Some(format!("user-{id}")),
                role: "user".to_string(),
                content: vec![ContentItem::InputText {
                    text: "How does path-based resume work?".to_string(),
                }],
                end_turn: None,
                phase: None,
            }),
        },
        RolloutLine {
            timestamp: last_event_at.to_string(),
            item: RolloutItem::ResponseItem(ResponseItem::Message {
                id: Some(format!("assistant-{id}")),
                role: "assistant".to_string(),
                content: vec![ContentItem::OutputText {
                    text: "By preferring the rollout path.".to_string(),
                }],
                end_turn: None,
                phase: None,
            }),
        },
    ];

    let mut writer = std::io::BufWriter::new(fs::File::create(&path).expect("open session file"));
    for line in lines {
        serde_json::to_writer(&mut writer, &line).expect("write rollout line");
        writer.write_all(b"\n").expect("write newline");
    }
    writer.flush().expect("flush rollout file");

    path
}

#[test]
fn binary_smoke_requires_init_and_executes_command() {
    let marker = "hello-from-app-server-binary-smoke";
    let requests = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"getUserAgent"}),
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"initialize",
            "params":{
                "clientInfo":{
                    "name":"app-server-binary-smoke",
                    "version":"0.1.0"
                }
            }
        }),
        json!({"jsonrpc":"2.0","id":3,"method":"getUserAgent"}),
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"execOneOffCommand",
            "params":{
                "command":["bash","-lc", format!("echo {marker}")],
                "timeoutMs":5000
            }
        }),
    ];

    let responses = run_jsonrpc_script(&requests);

    let pre_init_error = responses
        .get(&1)
        .and_then(|v| v.get("error"))
        .and_then(|v| v.get("message"))
        .and_then(Value::as_str)
        .expect("expected error response for pre-initialize getUserAgent");
    assert!(
        pre_init_error.contains("Not initialized"),
        "unexpected pre-init error message: {pre_init_error}"
    );

    let user_agent = responses
        .get(&3)
        .and_then(|v| v.get("result"))
        .and_then(|v| v.get("userAgent"))
        .and_then(Value::as_str)
        .expect("expected getUserAgent response after initialize");
    assert!(
        user_agent.contains("(app-server-binary-smoke; 0.1.0)"),
        "user agent did not include initialize client info: {user_agent}"
    );

    let exec_result = responses
        .get(&4)
        .and_then(|v| v.get("result"))
        .expect("expected execOneOffCommand response");
    let exit_code = exec_result
        .get("exitCode")
        .and_then(Value::as_i64)
        .expect("execOneOffCommand result missing exitCode");
    let stdout = exec_result
        .get("stdout")
        .and_then(Value::as_str)
        .expect("execOneOffCommand result missing stdout");

    assert_eq!(exit_code, 0, "execOneOffCommand returned non-zero exit");
    assert!(
        stdout.contains(marker),
        "execOneOffCommand stdout missing marker. stdout was: {stdout}"
    );
}

#[test]
fn binary_smoke_supports_thread_native_requests() {
    let requests = vec![
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "clientInfo":{
                    "name":"app-server-thread-smoke",
                    "version":"0.1.0"
                }
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"thread/list",
            "params":{
                "cursor":"bad-cursor"
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"thread/list",
            "params":{}
        }),
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"thread/read",
            "params":{
                "threadId":"00000000-0000-0000-0000-000000000000",
                "includeTurns":true
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":5,
            "method":"thread/start",
            "params":{}
        }),
        json!({
            "jsonrpc":"2.0",
            "id":6,
            "method":"thread/loaded/list",
            "params":{}
        }),
    ];

    let responses = run_jsonrpc_script(&requests);

    let bad_cursor_error = responses
        .get(&2)
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .expect("expected invalid cursor error");
    assert!(
        bad_cursor_error.contains("invalid thread cursor"),
        "unexpected cursor error: {bad_cursor_error}"
    );

    let thread_list = responses
        .get(&3)
        .and_then(|value| value.get("result"))
        .expect("expected thread/list response");
    assert!(
        thread_list.get("data").and_then(Value::as_array).is_some(),
        "thread/list result missing data array: {thread_list}"
    );

    let missing_thread_error = responses
        .get(&4)
        .and_then(|value| value.get("error"))
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .expect("expected missing thread error");
    assert!(
        missing_thread_error.contains("thread not found"),
        "unexpected thread/read error: {missing_thread_error}"
    );

    let thread_start = responses
        .get(&5)
        .and_then(|value| value.get("result"))
        .expect("expected thread/start response");
    let thread_id = thread_start
        .get("thread")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .expect("thread/start response missing thread id");
    let model = thread_start
        .get("model")
        .and_then(Value::as_str)
        .expect("thread/start response missing model");
    assert!(!thread_id.is_empty(), "thread/start returned empty thread id");
    assert!(!model.is_empty(), "thread/start returned empty model");

    let loaded_threads = responses
        .get(&6)
        .and_then(|value| value.get("result"))
        .and_then(|value| value.get("data"))
        .and_then(Value::as_array)
        .expect("expected thread/loaded/list response");
    assert!(
        loaded_threads.iter().any(|value| value.as_str() == Some(thread_id)),
        "thread/loaded/list should include the started thread: {loaded_threads:?}"
    );
}

#[test]
fn binary_smoke_reads_and_resumes_persisted_thread_history() {
    let code_home = TempCodeHome::new();
    let session_id = Uuid::new_v4();
    let rollout_path = create_session_fixture(code_home.path(), &session_id);
    let bogus_thread_id = "00000000-0000-0000-0000-000000000000";

    let requests = vec![
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "clientInfo":{
                    "name":"app-server-thread-history-smoke",
                    "version":"0.1.0"
                }
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":2,
            "method":"thread/read",
            "params":{
                "threadId":session_id.to_string(),
                "includeTurns":true
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":3,
            "method":"thread/resume",
            "params":{
                "threadId":bogus_thread_id,
                "path":rollout_path,
                "cwd":"/tmp/thread-native-smoke-project"
            }
        }),
        json!({
            "jsonrpc":"2.0",
            "id":4,
            "method":"thread/fork",
            "params":{
                "threadId":bogus_thread_id,
                "path":rollout_path,
                "cwd":"/tmp/thread-native-smoke-project"
            }
        }),
    ];

    let responses = run_jsonrpc_script_with_code_home(&requests, Some(code_home.path()));

    let thread = responses
        .get(&2)
        .and_then(|value| value.get("result"))
        .and_then(|value| value.get("thread"))
        .expect("expected thread/read response");
    let turns = thread
        .get("turns")
        .and_then(Value::as_array)
        .expect("thread/read should include turns array");
    assert_eq!(turns.len(), 1, "expected one reconstructed turn");
    assert_eq!(
        turns[0]
            .get("status")
            .and_then(Value::as_str)
            .expect("turn status should be present"),
        "completed"
    );
    let items = turns[0]
        .get("items")
        .and_then(Value::as_array)
        .expect("turn items should be present");
    assert_eq!(items.len(), 2, "expected user and agent items in reconstructed turn");
    assert_eq!(
        items[0]
            .get("type")
            .and_then(Value::as_str)
            .expect("user item type should be present"),
        "userMessage"
    );
    assert_eq!(
        items[1]
            .get("type")
            .and_then(Value::as_str)
            .expect("agent item type should be present"),
        "agentMessage"
    );

    let resumed_thread = responses
        .get(&3)
        .and_then(|value| value.get("result"))
        .and_then(|value| value.get("thread"))
        .expect("expected thread/resume response");
    let resumed_turns = resumed_thread
        .get("turns")
        .and_then(Value::as_array)
        .expect("thread/resume should include turns array");
    assert_eq!(
        resumed_turns.len(), 1,
        "thread/resume should preload persisted history even when path takes precedence"
    );
    let resumed_thread_id = resumed_thread
        .get("id")
        .and_then(Value::as_str)
        .expect("resumed thread id should be present");
    assert_ne!(
        resumed_thread_id, bogus_thread_id,
        "thread/resume should ignore threadId when path is provided"
    );

    let forked_thread = responses
        .get(&4)
        .and_then(|value| value.get("result"))
        .and_then(|value| value.get("thread"))
        .expect("expected thread/fork response");
    let forked_turns = forked_thread
        .get("turns")
        .and_then(Value::as_array)
        .expect("thread/fork should include turns array");
    assert_eq!(
        forked_turns.len(), 1,
        "thread/fork should preload persisted history when path takes precedence"
    );
    let forked_thread_id = forked_thread
        .get("id")
        .and_then(Value::as_str)
        .expect("forked thread id should be present");
    assert_ne!(
        forked_thread_id, bogus_thread_id,
        "thread/fork should ignore threadId when path is provided"
    );
}
