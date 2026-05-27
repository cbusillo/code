# Every Code CLI (Rust Implementation)

Every Code provides a standalone native `code` executable. This directory is the
Rust workspace that builds the product CLI used for local dogfooding and GitHub
Release updates.

## Installing Every Code

The canonical internal install and update source is the repository's GitHub
Releases, including the generated `update-manifest.json` consumed by the CLI
updater. npm and Homebrew publishing are deferred unless package-manager
distribution becomes intentional again.

```shell
code update-check
code update --yes
```

For local development, build from the repository root with:

```shell
./build-fast.sh
```

## What's new in the Rust CLI

The Rust implementation is the maintained Every Code CLI and serves as the
default experience. It includes a number of features that the legacy TypeScript
CLI never supported.

### Config

Code supports a rich set of configuration options. Note that the Rust CLI uses `config.toml` instead of `config.json`. See [`docs/config.md`](../docs/config.md) for details.

### Model Context Protocol Support

Code CLI functions as an MCP client that can connect to MCP servers on startup. See the [`mcp_servers`](../docs/config.md#mcp_servers) section in the configuration documentation for details.

It is still experimental, but you can also launch Code as an MCP _server_ by running `code mcp-server`. Use the [`@modelcontextprotocol/inspector`](https://github.com/modelcontextprotocol/inspector) to try it out:

```shell
npx @modelcontextprotocol/inspector code mcp-server
```

Use `code mcp` to add/list/get/remove MCP server launchers defined in `config.toml`, and `code mcp-server` to run the MCP server directly.

### Notifications

You can enable notifications by configuring a script that is run whenever the agent finishes a turn. The [notify documentation](../docs/config.md#notify) includes a detailed example that explains how to get desktop notifications via [terminal-notifier](https://github.com/julienXX/terminal-notifier) on macOS.

### `code exec` to run Code programmatically/non-interactively

To run Code non-interactively, run `code exec PROMPT` (you can also pass the prompt via `stdin`) and Code will work on your task until it decides that it is done and exits. Output is printed to the terminal directly. You can set the `RUST_LOG` environment variable to see more about what's going on.

### Use `@` for file search

Typing `@` triggers a fuzzy-filename search over the workspace root. Use up/down to select among the results and Tab or Enter to replace the `@` with the selected path. You can use Esc to cancel the search.

### Esc–Esc to edit a previous message

When the chat composer is empty, press Esc to prime “backtrack” mode. Press Esc again to open a transcript preview highlighting the last user message; press Esc repeatedly to step to older user messages. Press Enter to confirm and Code will fork the conversation from that point, trim the visible transcript accordingly, and pre‑fill the composer with the selected user message so you can edit and resubmit it.

In the transcript preview, the footer shows an `Esc edit prev` hint while editing is active.

### `--cd`/`-C` flag

Sometimes it is not convenient to `cd` to the directory you want Code to use as the "working root" before running Code. Fortunately, `code` supports a `--cd` option so you can specify whatever folder you want. You can confirm that Code is honoring `--cd` by double-checking the **workdir** it reports in the TUI at the start of a new session.

### Shell completions

Generate shell completion scripts via:

```shell
code completion bash
code completion zsh
code completion fish
```

### Experimenting with the Code Sandbox

To test to see what happens when a command is run under the sandbox provided by Code, we provide the following subcommands in Code CLI:

```
# macOS
code debug seatbelt [--full-auto] [COMMAND]...

# Linux
code debug landlock [--full-auto] [COMMAND]...
```

### Selecting a sandbox policy via `--sandbox`

The Rust CLI exposes a dedicated `--sandbox` (`-s`) flag that lets you pick the sandbox policy **without** having to reach for the generic `-c/--config` option:

```shell
# Run Code with the default, read-only sandbox
code --sandbox read-only

# Allow the agent to write within the current workspace while still blocking network access
code --sandbox workspace-write

# Danger! Disable sandboxing entirely (only do this if you are already running in a container or other isolated env)
code --sandbox danger-full-access
```

The same setting can be persisted in `~/.code/config.toml` via the top-level `sandbox_mode = "MODE"` key (Code will also read legacy `~/.codex/config.toml`), e.g. `sandbox_mode = "workspace-write"`.

If you want to prevent the agent from updating Git metadata (e.g., local safety), you can opt‑out with a workspace‑write tweak:

```toml
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
allow_git_writes = false   # default is true; set false to protect .git
```

### TUI anti-truncation fallback

If the transcript's last line intermittently clips, Code keeps a guarded
bottom spacer enabled. The TUI adds a 1–2 row overscan pad when the computed
history height looks like it might land flush with the viewport, reducing the
chance the final row disappears mid-stream. Enable `RUST_LOG=debug` to log when
the fallback fires while you iterate on layouts.

### Debugging Virtual Cursor

Use these console helpers to diagnose motion/cancellation behavior when testing in a real browser:

- Disable clickPulse transforms and force long CSS duration:

  `window.__vc && (window.__vc.clickPulse = () => (console.debug('[VC] clickPulse disabled'), 0), window.__vc.setMotion({ engine: 'css', cssDurationMs: 10000 }))`

- Wrap `moveTo` to log duplicates with sequence and inter-call delta:

  `(() => { const vc = window.__vc; if (!vc || vc.__wrapped) return; const orig = vc.moveTo; let seq=0, last=0; vc.moveTo = function(x,y,o){ const now=Date.now(); console.debug('[VC] moveTo call',{seq:++seq,x,y,o,sincePrevMs:last?now-last:null}); last=now; return orig.call(this,x,y,o); }; vc.__wrapped = true; console.debug('[VC] moveTo wrapper installed'); })();`

- Trigger a test move (adjust coordinates as needed):

  `window.__vc && window.__vc.moveTo(200, 200)`

## Code Organization

This folder is the root of a Cargo workspace. It contains quite a bit of experimental code, but here are the key crates:

- [`core/`](./core) contains the business logic for Code. Ultimately, we hope this to be a library crate that is generally useful for building other Rust/native applications that use Code.
- [`exec/`](./exec) "headless" CLI for use in automation.
- [`tui/`](./tui) CLI that launches a fullscreen TUI built with [Ratatui](https://ratatui.rs/).
- [`cli/`](./cli) CLI multitool that provides the aforementioned CLIs via subcommands.
