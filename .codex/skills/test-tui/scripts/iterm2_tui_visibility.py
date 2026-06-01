# /// script
# requires-python = ">=3.11"
# dependencies = [
#   "iterm2>=2.9",
#   "pyobjc-framework-Quartz>=10.0",
# ]
# ///
"""Read-only macOS/iTerm2 visibility helper for Every Code TUI dogfooding."""

from __future__ import annotations

import argparse
import asyncio
import json
import re
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Awaitable, Callable, TypeVar


T = TypeVar("T")


@dataclass
class Advice:
    summary: str
    action: str
    install: str | None = None


class UserFacingError(RuntimeError):
    def __init__(self, message: str, advice: Advice | None = None) -> None:
        super().__init__(message)
        self.advice = advice


def repo_root() -> Path:
    path = Path(__file__).resolve()
    for parent in path.parents:
        if (parent / ".git").exists():
            return parent
    return path.parents[4]


def default_capture_dir() -> Path:
    return repo_root() / ".code" / "tui-captures"


def utc_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def json_print(payload: Any) -> None:
    print(json.dumps(payload, ensure_ascii=False, indent=2, sort_keys=True))


def advice_payload(error: UserFacingError) -> dict[str, Any]:
    payload: dict[str, Any] = {"ok": False, "error": str(error)}
    if error.advice is not None:
        payload["llm_advice"] = asdict(error.advice)
    return payload


def ensure_macos(feature: str) -> None:
    if sys.platform == "darwin":
        return
    raise UserFacingError(
        f"{feature} requires macOS",
        Advice(
            summary="This helper targets iTerm2 on macOS.",
            action="Advise the user to use VT100 snapshot tests or a platform-specific terminal capture helper on this system.",
        ),
    )


def import_iterm2() -> Any:
    try:
        import iterm2  # type: ignore[import-not-found]
    except Exception as exc:
        raise UserFacingError(
            "the iTerm2 Python SDK is not available",
            Advice(
                summary="The helper needs the iTerm2 Python SDK to list and capture sessions.",
                action="Advise the user to run this script through uv so PEP 723 dependencies are installed automatically.",
                install="uv run .codex/skills/test-tui/scripts/iterm2_tui_visibility.py list",
            ),
        ) from exc
    return iterm2


def import_quartz() -> tuple[Any, int, int]:
    try:
        from Quartz import CGWindowListCopyWindowInfo  # type: ignore[import-not-found]
        from Quartz import kCGNullWindowID  # type: ignore[import-not-found]
        from Quartz import kCGWindowListOptionOnScreenOnly  # type: ignore[import-not-found]
    except Exception as exc:
        raise UserFacingError(
            "Quartz Python bindings are not available",
            Advice(
                summary="Window-id screenshots need pyobjc Quartz bindings.",
                action="Advise the user to run this script through uv so PEP 723 dependencies are installed automatically.",
                install="uv run .codex/skills/test-tui/scripts/iterm2_tui_visibility.py windows",
            ),
        ) from exc
    return CGWindowListCopyWindowInfo, kCGWindowListOptionOnScreenOnly, kCGNullWindowID


def run_command(argv: list[str]) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(argv, check=False, text=True, capture_output=True)
    except FileNotFoundError as exc:
        raise UserFacingError(
            f"required command is missing: {argv[0]}",
            Advice(
                summary=f"{argv[0]} is not available on PATH.",
                action="Advise the user to install the missing macOS command or use the iTerm2 text capture path instead.",
            ),
        ) from exc


def process_for_tty(tty: str | None) -> str | None:
    if not tty:
        return None
    proc = run_command(["ps", "-t", tty.replace("/dev/", ""), "-o", "comm=", "-o", "args="])
    if proc.returncode != 0:
        return None
    lines = [line.strip() for line in proc.stdout.splitlines() if line.strip()]
    return lines[-1] if lines else None


async def session_variable(session: Any, name: str) -> Any:
    try:
        return await session.async_get_variable(name)
    except Exception:
        return None


def likely_every_code_score(session: dict[str, Any]) -> int:
    haystack = " ".join(
        str(session.get(key) or "")
        for key in ("name", "tty", "foreground_process", "session_id")
    ).lower()
    score = 0
    if re.search(r"\b(code|codex)\b", haystack):
        score += 3
    if "every code" in haystack:
        score += 5
    if "code-rs" in haystack:
        score += 2
    return score


async def session_metadata(
    session: Any,
    window_index: int,
    tab_index: int,
    session_index: int,
) -> dict[str, Any]:
    grid_size = getattr(session, "grid_size", None)
    tty = await session_variable(session, "tty")
    cwd = await session_variable(session, "path")
    job_name = await session_variable(session, "jobName")
    metadata = {
        "window_index": window_index,
        "tab_index": tab_index,
        "session_index": session_index,
        "session_id": getattr(session, "session_id", None),
        "name": getattr(session, "name", None),
        "tty": tty,
        "cwd": cwd,
        "job_name": job_name,
        "foreground_process": process_for_tty(tty),
        "grid_size": {
            "width": getattr(grid_size, "width", None),
            "height": getattr(grid_size, "height", None),
        },
    }
    metadata["likely_every_code_score"] = likely_every_code_score(metadata)
    return metadata


async def list_sessions(connection: Any) -> list[dict[str, Any]]:
    app = await import_iterm2().async_get_app(connection)
    sessions: list[dict[str, Any]] = []
    for window_index, window in enumerate(app.windows, start=1):
        for tab_index, tab in enumerate(window.tabs, start=1):
            for session_index, session in enumerate(tab.sessions, start=1):
                sessions.append(
                    await session_metadata(session, window_index, tab_index, session_index)
                )
    return sessions


async def find_session(connection: Any, session_id: str) -> Any:
    app = await import_iterm2().async_get_app(connection)
    for window in app.windows:
        for tab in window.tabs:
            for session in tab.sessions:
                if getattr(session, "session_id", None) == session_id:
                    return session
    raise UserFacingError(
        f"iTerm2 session not found: {session_id}",
        Advice(
            summary="The requested session id is not currently visible to iTerm2.",
            action="Advise the user to rerun the list command and select a current session id.",
        ),
    )


async def run_with_iterm2(callback: Callable[[Any], Awaitable[T]]) -> T:
    ensure_macos("iTerm2 session capture")
    iterm2 = import_iterm2()
    try:
        connection = await iterm2.Connection.async_create()
        return await callback(connection)
    except UserFacingError:
        raise
    except Exception as exc:
        raise UserFacingError(
            f"iTerm2 SDK request failed: {exc}",
            Advice(
                summary="The helper could not connect to iTerm2 through the Python SDK.",
                action="Advise the user to ensure iTerm2 is running and enable iTerm2's Python API/Scripting support, then retry with uv run .codex/skills/test-tui/scripts/iterm2_tui_visibility.py list.",
            ),
        ) from exc


async def command_list_async(_args: argparse.Namespace) -> int:
    sessions = await run_with_iterm2(list_sessions)
    json_print({"ok": True, "sessions": sessions})
    return 0


async def command_text_async(args: argparse.Namespace) -> int:
    async def capture(connection: Any) -> dict[str, Any]:
        session = await find_session(connection, args.session_id)
        line_info = await session.async_get_line_info()
        first_line = line_info.first_visible_line_number
        line_count = line_info.mutable_area_height
        lines = await session.async_get_contents(first_line, line_count)
        return {
            "session_id": getattr(session, "session_id", None),
            "name": getattr(session, "name", None),
            "first_visible_line_number": first_line,
            "rows": len(lines),
            "contents": "\n".join(line.string for line in lines),
        }

    payload = await run_with_iterm2(capture)
    json_print(
        {
            "ok": True,
            "capture": {
                "kind": "iterm2_session_text",
                "timestamp": utc_stamp(),
                **payload,
            },
        }
    )
    return 0


def command_windows(_args: argparse.Namespace) -> int:
    ensure_macos("macOS window listing")
    copy_window_info, on_screen_only, null_window_id = import_quartz()
    windows = copy_window_info(on_screen_only, null_window_id)
    iterm_windows: list[dict[str, Any]] = []
    for window in windows or []:
        owner = window.get("kCGWindowOwnerName")
        if owner not in {"iTerm2", "iTerm"}:
            continue
        bounds = window.get("kCGWindowBounds") or {}
        iterm_windows.append(
            {
                "window_id": window.get("kCGWindowNumber"),
                "owner": owner,
                "title": window.get("kCGWindowName"),
                "bounds": {
                    "x": bounds.get("X"),
                    "y": bounds.get("Y"),
                    "width": bounds.get("Width"),
                    "height": bounds.get("Height"),
                },
            }
        )
    json_print({"ok": True, "windows": iterm_windows})
    return 0


def command_screenshot(args: argparse.Namespace) -> int:
    ensure_macos("macOS window screenshots")
    if shutil.which("screencapture") is None:
        raise UserFacingError(
            "screencapture is not available",
            Advice(
                summary="The macOS screencapture command is required for pixel evidence.",
                action="Advise the user to use iTerm2 text capture until screencapture is available.",
            ),
        )
    if not args.window_id:
        raise UserFacingError(
            "screenshot requires --window-id",
            Advice(
                summary="The helper captures windows, not x/y rectangles by default.",
                action="Advise the user to run the windows command, choose the intended iTerm2 window id, then rerun screenshot with --window-id.",
                install="uv run .codex/skills/test-tui/scripts/iterm2_tui_visibility.py windows",
            ),
        )
    output_dir = Path(args.output_dir) if args.output_dir else default_capture_dir()
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / f"iterm2-window-{args.window_id}-{utc_stamp()}.png"
    proc = run_command(["screencapture", "-x", "-l", str(args.window_id), str(output)])
    if proc.returncode != 0:
        raise UserFacingError(
            f"screencapture failed: {proc.stderr.strip() or proc.stdout.strip()}",
            Advice(
                summary="macOS refused or failed the targeted window screenshot.",
                action="Advise the user to grant Screen Recording permission to the app running this helper and retry.",
            ),
        )
    json_print(
        {
            "ok": True,
            "capture": {
                "kind": "macos_window_screenshot",
                "timestamp": utc_stamp(),
                "session_id": args.session_id,
                "window_id": str(args.window_id),
                "output": str(output),
            },
        }
    )
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Read-only iTerm2/macOS visibility helper for Every Code TUI sessions."
    )
    sub = parser.add_subparsers(dest="command", required=True)

    list_parser = sub.add_parser("list", help="List iTerm2 sessions with metadata only.")
    list_parser.set_defaults(func=lambda args: asyncio.run(command_list_async(args)))

    windows_parser = sub.add_parser(
        "windows", help="List visible iTerm2 macOS windows and screenshot window ids."
    )
    windows_parser.set_defaults(func=command_windows)

    text_parser = sub.add_parser("text", help="Capture one iTerm2 session's visible text.")
    text_parser.add_argument("session_id", help="iTerm2 session unique id from the list command.")
    text_parser.set_defaults(func=lambda args: asyncio.run(command_text_async(args)))

    screenshot_parser = sub.add_parser("screenshot", help="Capture a targeted macOS window.")
    screenshot_parser.add_argument("session_id", help="iTerm2 session id for labeling context.")
    screenshot_parser.add_argument("--window-id", help="Known macOS CGWindowID to capture.")
    screenshot_parser.add_argument(
        "--output-dir", help="Directory for screenshot output. Defaults to .code/tui-captures/."
    )
    screenshot_parser.set_defaults(func=command_screenshot)

    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except UserFacingError as exc:
        json_print(advice_payload(exc))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
