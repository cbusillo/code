# every-code-webui notes

## Browser-first UX review

- When reviewing this web app's UI/UX, prefer a real browser workflow over
  static code reading.
- The working native Gemini path is to use the `browser-ui-review` skill from
  `~/.agents/skills/browser-ui-review`, which drives the `ui-browser` helper.
- Before asking Gemini for a broad critique, do a tiny smoke test first to
  confirm live browser access. Example:

```text
Use the browser-ui-review skill. Open http://127.0.0.1:4185/ with the real
browser helper, then reply with only: TITLE: <page title>.
```

- If that smoke test succeeds, Gemini can be trusted for live exploratory UI
  review on this app.

## Claude/Gemini agent caveat

- Do not assume Code's `agent.create` flow gives Claude or Gemini shared access
  to the main session browser tool.
- Native Gemini can use `browser-ui-review` directly when launched with the
  right prompt and approval mode.
- Claude may still fail in Code's read-only agent flow because the current
  harness restricts allowed tools and can block `ui-browser` even when the
  Claude skill is installed.

## Local app URL

- Current local web UI entry URL used for browser review: `http://127.0.0.1:4185/`
- Shared-session app-server is often expected at `ws://127.0.0.1:8877` for
  browser companion flows unless a session explicitly uses a different port.
