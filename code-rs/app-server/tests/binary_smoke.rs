use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use serde_json::Value;
use serde_json::json;

fn app_server_bin() -> PathBuf {
    PathBuf::from(assert_cmd::cargo::cargo_bin!("code-app-server"))
}

fn run_jsonrpc_script_with_args(args: &[&str], requests: &[Value]) -> BTreeMap<i64, Value> {
    let mut child = Command::new(app_server_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
        .map(|line| {
            let message: Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("invalid JSON-RPC line `{line}`: {e}"));
            let id = message
                .get("id")
                .and_then(Value::as_i64)
                .unwrap_or_else(|| panic!("JSON-RPC message missing numeric id: {message}"));
            (id, message)
        })
        .collect()
}

fn run_jsonrpc_script(requests: &[Value]) -> BTreeMap<i64, Value> {
    run_jsonrpc_script_with_args(&[], requests)
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
fn binary_smoke_accepts_desktop_startup_polling_methods() {
    let requests = vec![
        json!({
            "jsonrpc":"2.0",
            "id":1,
            "method":"initialize",
            "params":{
                "clientInfo":{
                    "name":"codex-desktop-smoke",
                    "version":"0.1.0"
                }
            }
        }),
        json!({"jsonrpc":"2.0","id":2,"method":"thread/list","params":{}}),
        json!({"jsonrpc":"2.0","id":3,"method":"model/list","params":{}}),
        json!({"jsonrpc":"2.0","id":4,"method":"skills/list","params":{}}),
        json!({"jsonrpc":"2.0","id":5,"method":"plugin/list","params":{}}),
        json!({"jsonrpc":"2.0","id":6,"method":"hooks/list","params":{}}),
        json!({"jsonrpc":"2.0","id":7,"method":"mcpServerStatus/list","params":{}}),
        json!({"jsonrpc":"2.0","id":8,"method":"remoteControl/status/read","params":{}}),
        json!({"jsonrpc":"2.0","id":9,"method":"remoteControl/enable","params":{"enabled":true}}),
        json!({"jsonrpc":"2.0","id":10,"method":"collaborationMode/list","params":{}}),
        json!({"jsonrpc":"2.0","id":11,"method":"experimentalFeature/list","params":{}}),
        json!({"jsonrpc":"2.0","id":12,"method":"experimentalFeature/enablement/set","params":{"featureId":"desktop-smoke","enabled":true}}),
    ];

    let responses = run_jsonrpc_script_with_args(&["--analytics-default-enabled"], &requests);

    for id in 2..=12 {
        let response = responses
            .get(&id)
            .unwrap_or_else(|| panic!("missing response for request id {id}"));
        assert!(
            response.get("error").is_none(),
            "desktop startup method returned error for id {id}: {response}"
        );
    }

    for id in [2, 3, 4, 5, 6, 7, 10, 11] {
        let result = responses
            .get(&id)
            .and_then(|response| response.get("result"))
            .expect("expected list result");
        assert_eq!(result.get("data"), Some(&json!([])));
        assert_eq!(result.get("nextCursor"), Some(&json!(null)));
    }
    assert_eq!(
        responses
            .get(&8)
            .and_then(|response| response.get("result"))
            .and_then(|result| result.get("enabled")),
        Some(&json!(false))
    );
    assert_eq!(
        responses
            .get(&9)
            .and_then(|response| response.get("result"))
            .and_then(|result| result.get("unsupported")),
        Some(&json!(true))
    );
    assert_eq!(
        responses
            .get(&12)
            .and_then(|response| response.get("result"))
            .and_then(|result| result.get("unsupported")),
        Some(&json!(true))
    );
}
