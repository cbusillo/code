# Zed Integration

To point Zed at Every Code's (Code) ACP server, add this block to `settings.json`:

```jsonc
{
  "agent_servers": {
    "Code": {
      "command": "code",
      "args": ["acp"]
    }
  }
}
```

Adjust `command` to an absolute binary path if Zed cannot find `code` on PATH.

## Zed prerequisites

- Zed Stable `0.201.5` (released August 27, 2025) or newer adds ACP support with the Agent Panel. Update via `Zed → Check for Updates` before wiring Every Code in. Zed’s docs call out ACP as the mechanism powering Gemini CLI and other external agents.
- External agents live inside the Agent Panel (`cmd-?`). Use the `+` button to start a new thread and pick `Code` (Every Code) from the external agent list. Zed runs our CLI as a subprocess over JSON‑RPC, so all prompts and diff previews stay local.
- Zed launches the configured command directly. Use the repo-built or GitHub Release binary instead of package-manager shims so the integration follows the same update source as the terminal CLI.

## How Every Code implements ACP

- The Rust MCP server exposes ACP tools: `session/new`, `session/prompt`, and fast interrupts via `session/cancel`. These are backed by the same conversation manager that powers the TUI, so approvals, confirm guards, and sandbox policies remain intact.
- Streaming `session/update` notifications bridge Every Code events into Zed. You get Answer/Reasoning updates, shell command progress, approvals, and apply_patch diffs in the Zed UI without losing terminal parity.
- MCP configuration stays centralized in `CODE_HOME/config.toml` (Every Code also reads `CODEX_HOME/config.toml` for compatibility). Use `[experimental_client_tools]` to delegate file read/write and permission requests back to Zed when you want its UI to handle approvals. A minimal setup looks like:

```toml
[experimental_client_tools]
request_permission = { mcp_server = "zed", tool_name = "requestPermission" }
read_text_file = { mcp_server = "zed", tool_name = "readTextFile" }
write_text_file = { mcp_server = "zed", tool_name = "writeTextFile" }
```

Zed wires these tools automatically when you add the Code (Every Code) agent, so the identifiers above match the defaults.
- The CLI entry point (`code acp`) runs the Rust MCP server that ships alongside the rest of Every Code. Build-from-source workflows plug in by swapping `command` for an absolute path to that binary.

## Tips and troubleshooting

- Need to inspect the handshake? Run Zed’s `dev: open acp logs` command from the Command Palette; the log shows JSON‑RPC requests and Every Code replies.
- If prompts hang, make sure no other process is bound to the same MCP port and that your `CODE_HOME` (or legacy `CODEX_HOME`) points to the intended config directory. The ACP server inherits all of Every Code’s sandbox settings, so restrictive policies (e.g., `approval_policy = "never"`) still apply.
- Zed currently skips history restores and checkpoint UI for third-party agents. Stick to the TUI if you rely on those features; ACP support is still evolving upstream.
- After a session starts, the model selector in Zed lists Every Code’s built-in presets (e.g., `gpt-5.1-codex`, `gpt-5.1` high/medium/low). Picking a new preset updates the running Every Code session immediately, so you don’t have to restart the agent to change models.
