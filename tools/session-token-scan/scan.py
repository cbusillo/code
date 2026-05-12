#!/usr/bin/env python3
from __future__ import annotations

import argparse
import glob
import json
import math
import re
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterable


TOKEN_KEYS = ("input_tokens", "cached_input_tokens", "output_tokens", "reasoning_output_tokens", "total_tokens")
IMAGE_RE = re.compile(r"data:image/[A-Za-z0-9.+-]+;base64,[A-Za-z0-9+/=]+")
WHITESPACE_RE = re.compile(r"\s+")


@dataclass
class TokenUsage:
    input_tokens: int = 0
    cached_input_tokens: int = 0
    output_tokens: int = 0
    reasoning_output_tokens: int = 0
    total_tokens: int = 0

    @classmethod
    def from_mapping(cls, value: Any) -> "TokenUsage":
        if not isinstance(value, dict):
            return cls()
        return cls(**{key: as_int(value.get(key)) for key in TOKEN_KEYS})

    def add(self, other: "TokenUsage") -> None:
        for key in TOKEN_KEYS:
            setattr(self, key, getattr(self, key) + getattr(other, key))

    def to_dict(self) -> dict[str, int]:
        return {key: getattr(self, key) for key in TOKEN_KEYS}


@dataclass
class TokenEvent:
    line: int
    timestamp: str
    requested_model: str | None
    latest_response_model: str | None
    total: TokenUsage
    last: TokenUsage


@dataclass
class LargePayload:
    path: str
    line: int
    timestamp: str
    record_type: str
    payload_type: str
    label: str
    raw_bytes: int
    string_path: str
    string_bytes: int
    snippet: str


@dataclass
class SessionReport:
    path: str
    bytes: int
    lines: int = 0
    invalid_json: int = 0
    started_at: str | None = None
    ended_at: str | None = None
    session_id: str | None = None
    cwd: str | None = None
    branch: str | None = None
    repo: str | None = None
    base_instruction_bytes: int = 0
    project_doc_sections: int = 0
    available_skill_sections: int = 0
    skill_entry_count: int = 0
    image_payload_count: int = 0
    image_payload_bytes: int = 0
    input_image_mentions: int = 0
    token_events: list[TokenEvent] = field(default_factory=list)
    payload_type_counts: dict[str, int] = field(default_factory=dict)
    function_call_counts: dict[str, int] = field(default_factory=dict)
    agent_event_count: int = 0
    large_payloads: list[LargePayload] = field(default_factory=list)
    usage_entries: int = 0
    usage_tokens: TokenUsage = field(default_factory=TokenUsage)

    @property
    def final_total(self) -> TokenUsage:
        return self.token_events[-1].total if self.token_events else TokenUsage()

    @property
    def peak_total(self) -> TokenUsage:
        best = TokenUsage()
        for event in self.token_events:
            if event.total.total_tokens > best.total_tokens:
                best = event.total
        return best

    @property
    def max_last(self) -> TokenUsage:
        best = TokenUsage()
        for event in self.token_events:
            if event.last.total_tokens > best.total_tokens:
                best = event.last
        return best

    @property
    def token_total_resets(self) -> int:
        resets = 0
        previous = 0
        for event in self.token_events:
            current = event.total.total_tokens
            if previous and current < previous:
                resets += 1
            previous = current
        return resets

    @property
    def cache_ratio(self) -> float:
        final = self.final_total
        return final.cached_input_tokens / final.input_tokens if final.input_tokens else 0.0

    @property
    def duplicate_instruction_flags(self) -> list[str]:
        flags = []
        if self.project_doc_sections > 1:
            flags.append(f"project-doc x{self.project_doc_sections}")
        if self.available_skill_sections > 1:
            flags.append(f"skills x{self.available_skill_sections}")
        return flags


def as_int(value: Any) -> int:
    if isinstance(value, bool):
        return 0
    if isinstance(value, int):
        return value
    if isinstance(value, float) and math.isfinite(value):
        return int(value)
    return 0


def parse_timestamp(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        return None


def safe_rel(path: Path) -> str:
    try:
        return str(path.expanduser().resolve())
    except OSError:
        return str(path)


def human_bytes(value: int) -> str:
    units = ["B", "KB", "MB", "GB"]
    amount = float(value)
    for unit in units:
        if abs(amount) < 1024 or unit == units[-1]:
            return f"{amount:.1f}{unit}" if unit != "B" else f"{int(amount)}B"
        amount /= 1024
    return f"{value}B"


def human_tokens(value: int) -> str:
    if abs(value) >= 1_000_000:
        return f"{value / 1_000_000:.2f}M"
    if abs(value) >= 1_000:
        return f"{value / 1_000:.1f}k"
    return str(value)


def compact(text: str, limit: int = 180) -> str:
    text = WHITESPACE_RE.sub(" ", text).strip()
    if text.startswith("data:image/"):
        return "<data:image payload>"
    if len(text) <= limit:
        return text
    return text[: limit - 3] + "..."


def iter_strings(value: Any, prefix: str = "") -> Iterable[tuple[str, str]]:
    if isinstance(value, str):
        yield prefix, value
    elif isinstance(value, list):
        for index, item in enumerate(value):
            next_prefix = f"{prefix}[{index}]" if prefix else f"[{index}]"
            yield from iter_strings(item, next_prefix)
    elif isinstance(value, dict):
        for key, item in value.items():
            next_prefix = f"{prefix}.{key}" if prefix else str(key)
            yield from iter_strings(item, next_prefix)


def record_type(record: dict[str, Any]) -> tuple[str, str, str]:
    top_type = str(record.get("type") or "")
    raw_payload = record.get("payload")
    payload: dict[str, Any] = raw_payload if isinstance(raw_payload, dict) else {}
    raw_msg = payload.get("msg")
    msg: dict[str, Any] = raw_msg if isinstance(raw_msg, dict) else {}
    raw_item = payload.get("item")
    item: dict[str, Any] = raw_item if isinstance(raw_item, dict) else {}
    payload_type = str(payload.get("type") or item.get("type") or msg.get("type") or "")
    label = str(payload.get("name") or payload.get("role") or item.get("name") or item.get("role") or msg.get("type") or "")
    return top_type, payload_type, label


def analyze_session(path: Path, large_threshold: int, top_payloads: int) -> SessionReport:
    report = SessionReport(path=safe_rel(path), bytes=path.stat().st_size)
    largest_payloads: list[LargePayload] = []

    with path.open("r", encoding="utf-8", errors="replace") as handle:
        for line_number, line in enumerate(handle, start=1):
            report.lines = line_number
            raw_bytes = len(line.encode("utf-8", errors="replace"))
            image_matches = IMAGE_RE.findall(line)
            report.image_payload_count += len(image_matches)
            report.image_payload_bytes += sum(len(match) for match in image_matches)
            report.input_image_mentions += line.count("input_image")

            try:
                record = json.loads(line)
            except json.JSONDecodeError:
                report.invalid_json += 1
                continue

            timestamp = str(record.get("timestamp") or "")
            if timestamp:
                report.started_at = report.started_at or timestamp
                report.ended_at = timestamp

            top_type, payload_type, label = record_type(record)
            if payload_type:
                report.payload_type_counts[payload_type] = report.payload_type_counts.get(payload_type, 0) + 1

            raw_payload = record.get("payload")
            payload: dict[str, Any] = raw_payload if isinstance(raw_payload, dict) else {}
            if top_type == "session_meta":
                analyze_session_meta(report, payload)

            if payload_type == "token_count":
                raw_msg = payload.get("msg")
                msg: dict[str, Any] = raw_msg if isinstance(raw_msg, dict) else {}
                raw_info = msg.get("info")
                info: dict[str, Any] = raw_info if isinstance(raw_info, dict) else {}
                report.token_events.append(TokenEvent(
                    line=line_number,
                    timestamp=timestamp,
                    requested_model=string_or_none(info.get("requested_model")),
                    latest_response_model=string_or_none(info.get("latest_response_model")),
                    total=TokenUsage.from_mapping(info.get("total_token_usage")),
                    last=TokenUsage.from_mapping(info.get("last_token_usage")),
                ))

            if payload_type == "function_call":
                name = string_or_none(payload.get("name")) or "unknown"
                report.function_call_counts[name] = report.function_call_counts.get(name, 0) + 1

            if "agent" in payload_type.lower() or "agent" in label.lower():
                report.agent_event_count += 1

            string_path, string_value = largest_string(record)
            string_bytes = len(string_value.encode("utf-8", errors="replace")) if string_value else 0
            if raw_bytes >= large_threshold or string_bytes >= large_threshold:
                largest_payloads.append(LargePayload(
                    path=report.path,
                    line=line_number,
                    timestamp=timestamp,
                    record_type=top_type,
                    payload_type=payload_type,
                    label=label,
                    raw_bytes=raw_bytes,
                    string_path=string_path,
                    string_bytes=string_bytes,
                    snippet=compact(string_value),
                ))

    largest_payloads.sort(key=lambda item: max(item.raw_bytes, item.string_bytes), reverse=True)
    report.large_payloads = largest_payloads[:top_payloads]
    return report


def analyze_session_meta(report: SessionReport, payload: dict[str, Any]) -> None:
    report.session_id = string_or_none(payload.get("id")) or report.session_id
    report.cwd = string_or_none(payload.get("cwd")) or report.cwd
    raw_git = payload.get("git")
    git: dict[str, Any] = raw_git if isinstance(raw_git, dict) else {}
    report.branch = string_or_none(git.get("branch")) or report.branch
    report.repo = string_or_none(git.get("repository_url")) or report.repo
    raw_base = payload.get("base_instructions")
    base: dict[str, Any] = raw_base if isinstance(raw_base, dict) else {}
    text = string_or_none(base.get("text")) or ""
    report.base_instruction_bytes = len(text.encode("utf-8", errors="replace"))
    report.project_doc_sections = text.count("--- project-doc ---")
    report.available_skill_sections = text.count("### Available skills")
    report.skill_entry_count = len(re.findall(r"(?m)^- [A-Za-z0-9_.-]+: .*\(file: ", text))


def string_or_none(value: Any) -> str | None:
    return value if isinstance(value, str) else None


def largest_string(value: Any) -> tuple[str, str]:
    best_path = ""
    best_value = ""
    best_len = -1
    for path, text in iter_strings(value):
        size = len(text)
        if size > best_len:
            best_path = path
            best_value = text
            best_len = size
    return best_path, best_value


def discover_sessions(inputs: list[str], pattern: str, limit: int | None) -> list[Path]:
    paths: list[Path] = []
    for raw in inputs:
        expanded = Path(raw).expanduser()
        if expanded.is_file():
            paths.append(expanded)
        elif expanded.is_dir():
            paths.extend(sorted(expanded.rglob(pattern)))
        else:
            matches = sorted(Path(match) for match in glob.glob(str(expanded), recursive=True))
            paths.extend(path for path in matches if path.is_file())
    unique = sorted({path.resolve() for path in paths}, key=lambda path: path.stat().st_mtime, reverse=True)
    if limit is not None:
        unique = unique[:limit]
    return sorted(unique)


def load_usage_entries(root: Path) -> list[tuple[datetime, TokenUsage, str]]:
    entries: list[tuple[datetime, TokenUsage, str]] = []
    if not root.exists():
        return entries
    for path in sorted(root.glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            continue
        raw_entries = data.get("hourly_entries")
        if not isinstance(raw_entries, list):
            continue
        account = string_or_none(data.get("account_id")) or path.stem
        for entry in raw_entries:
            if not isinstance(entry, dict):
                continue
            timestamp = parse_timestamp(string_or_none(entry.get("timestamp")))
            if timestamp is None:
                continue
            entries.append((timestamp, TokenUsage.from_mapping(entry.get("tokens")), account))
    return entries


def attach_usage(reports: list[SessionReport], usage_entries: list[tuple[datetime, TokenUsage, str]]) -> None:
    if not usage_entries:
        return
    for report in reports:
        start = parse_timestamp(report.started_at)
        end = parse_timestamp(report.ended_at)
        if start is None or end is None:
            continue
        if start.tzinfo is None:
            start = start.replace(tzinfo=timezone.utc)
        if end.tzinfo is None:
            end = end.replace(tzinfo=timezone.utc)
        for timestamp, tokens, _account in usage_entries:
            if start <= timestamp <= end:
                report.usage_entries += 1
                report.usage_tokens.add(tokens)


def to_jsonable(report: SessionReport) -> dict[str, Any]:
    return {
        "path": report.path,
        "bytes": report.bytes,
        "lines": report.lines,
        "invalid_json": report.invalid_json,
        "started_at": report.started_at,
        "ended_at": report.ended_at,
        "session_id": report.session_id,
        "cwd": report.cwd,
        "branch": report.branch,
        "repo": report.repo,
        "base_instruction_bytes": report.base_instruction_bytes,
        "project_doc_sections": report.project_doc_sections,
        "available_skill_sections": report.available_skill_sections,
        "skill_entry_count": report.skill_entry_count,
        "image_payload_count": report.image_payload_count,
        "image_payload_bytes": report.image_payload_bytes,
        "input_image_mentions": report.input_image_mentions,
        "final_total": report.final_total.to_dict(),
        "peak_total": report.peak_total.to_dict(),
        "max_last": report.max_last.to_dict(),
        "token_total_resets": report.token_total_resets,
        "cache_ratio": report.cache_ratio,
        "payload_type_counts": report.payload_type_counts,
        "function_call_counts": report.function_call_counts,
        "agent_event_count": report.agent_event_count,
        "usage_entries": report.usage_entries,
        "usage_tokens": report.usage_tokens.to_dict(),
        "large_payloads": [payload.__dict__ for payload in report.large_payloads],
        "token_events": [
            {
                "line": event.line,
                "timestamp": event.timestamp,
                "requested_model": event.requested_model,
                "latest_response_model": event.latest_response_model,
                "total": event.total.to_dict(),
                "last": event.last.to_dict(),
            }
            for event in report.token_events
        ],
    }


def print_text_report(reports: list[SessionReport], top: int) -> None:
    print("# Session Token Scan")
    print()
    print(f"Sessions analyzed: {len(reports)}")
    print()
    if not reports:
        return

    print("## Sessions")
    header = f"{'peak':>9} {'max-last':>9} {'cache':>6} {'file':>8} {'images':>8} {'base':>8} {'resets':>6} {'flags':<22} path"
    print(header)
    print("-" * len(header))
    for report in sorted(reports, key=lambda item: item.peak_total.total_tokens, reverse=True):
        flags = ", ".join(report.duplicate_instruction_flags) or "-"
        print(
            f"{human_tokens(report.peak_total.total_tokens):>9} "
            f"{human_tokens(report.max_last.total_tokens):>9} "
            f"{report.cache_ratio:>5.0%} "
            f"{human_bytes(report.bytes):>8} "
            f"{human_bytes(report.image_payload_bytes):>8} "
            f"{human_bytes(report.base_instruction_bytes):>8} "
            f"{report.token_total_resets:>6} "
            f"{flags:<22} {report.path}"
        )
    print()

    token_events = [event_summary(report, event) for report in reports for event in report.token_events]
    token_events.sort(key=lambda item: item["last_total"], reverse=True)
    print("## Largest Token Events")
    for item in token_events[:top]:
        print(
            f"- {human_tokens(item['last_total'])} last / {human_tokens(item['total'])} cumulative "
            f"at line {item['line']} ({item['model'] or 'unknown model'}) in {item['path']}"
        )
    print()

    large_payloads = [payload for report in reports for payload in report.large_payloads]
    large_payloads.sort(key=lambda item: max(item.raw_bytes, item.string_bytes), reverse=True)
    print("## Largest Records")
    for payload in large_payloads[:top]:
        print(
            f"- {human_bytes(payload.raw_bytes)} raw / {human_bytes(payload.string_bytes)} string "
            f"line {payload.line} {payload.payload_type or payload.record_type} {payload.label} in {payload.path}"
        )
        if payload.snippet:
            print(f"  {payload.string_path}: {payload.snippet}")
    print()

    image_reports = sorted((report for report in reports if report.image_payload_bytes), key=lambda item: item.image_payload_bytes, reverse=True)
    if image_reports:
        print("## Image Payload Suspects")
        for report in image_reports[:top]:
            print(
                f"- {human_bytes(report.image_payload_bytes)} across {report.image_payload_count} data:image payload(s), "
                f"{report.input_image_mentions} input_image mention(s): {report.path}"
            )
        print()

    usage_reports = [report for report in reports if report.usage_entries]
    if usage_reports:
        print("## Usage Correlation")
        for report in sorted(usage_reports, key=lambda item: item.usage_tokens.total_tokens, reverse=True)[:top]:
            print(
                f"- {human_tokens(report.usage_tokens.total_tokens)} usage tokens from {report.usage_entries} usage entries: {report.path}"
            )
        print()


def event_summary(report: SessionReport, event: TokenEvent) -> dict[str, Any]:
    return {
        "path": report.path,
        "line": event.line,
        "model": event.latest_response_model or event.requested_model,
        "last_total": event.last.total_tokens,
        "total": event.total.total_tokens,
    }


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Scan Every Code rollout/session files for token-efficiency suspects.")
    parser.add_argument("inputs", nargs="*", default=["~/.code/sessions"], help="Session files, directories, or globs. Defaults to ~/.code/sessions.")
    parser.add_argument("--pattern", default="rollout-*.jsonl", help="Filename pattern used when scanning directories.")
    parser.add_argument("--limit", type=int, default=20, help="Analyze the most recently modified N sessions. Use 0 for all discovered sessions.")
    parser.add_argument("--top", type=int, default=10, help="Number of top suspects to print per section.")
    parser.add_argument("--large-threshold", type=int, default=16_384, help="Record/string byte threshold for large-record suspects.")
    parser.add_argument("--usage-root", default="~/.code/usage", help="Usage directory for optional timestamp correlation.")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable JSON instead of text.")
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    limit = None if args.limit == 0 else args.limit
    sessions = discover_sessions(args.inputs, args.pattern, limit)
    reports = [analyze_session(path, args.large_threshold, args.top) for path in sessions]
    attach_usage(reports, load_usage_entries(Path(args.usage_root).expanduser()))
    if args.json:
        print(json.dumps({"sessions": [to_jsonable(report) for report in reports]}, indent=2, sort_keys=True))
    else:
        print_text_report(reports, args.top)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
