## Every Code v0.6.114

This release focuses on stronger Auto Review durability, cleaner resumed sessions, and more predictable agent/release operations.

### Changes

- Auto Review runs now persist more reliably, surface currentness and stale state in the TUI, recover lost or cancelled findings after restart, and deduplicate repeated reviews by diff fingerprint.
- Resumed conversations avoid duplicate assistant replay and preserve final answers after history snapshots, while exec stream history handles UTF-8 truncation more safely.
- Agent launches support preloaded context files with explicit large-context budgets, more reliable large prompt delivery, isolated Antigravity launch caches, and preferred-account routing.
- Logout, self-update, managed worktree cleanup, and release staging are tightened so local operations behave more predictably for users and release operators.

### Install

```bash
gh release download v0.6.114 --repo cbusillo/code
```

Compare: https://github.com/cbusillo/code/compare/v0.6.113...v0.6.114
