from __future__ import annotations

import json
import sys
import unittest
from datetime import datetime
from pathlib import Path
from tempfile import TemporaryDirectory

sys.path.insert(0, str(Path(__file__).resolve().parent))

import scan


def ts(value: str) -> datetime:
    timestamp = scan.parse_timestamp(value)
    assert timestamp is not None
    return timestamp


class SessionTokenScanTests(unittest.TestCase):
    def test_analyze_session_reports_expected_suspects(self) -> None:
        with TemporaryDirectory() as directory:
            path = Path(directory) / "rollout-test.jsonl"
            base_text = "\n".join([
                "--- project-doc ---",
                "### Available skills",
                "- github: use GitHub (file: /tmp/github/SKILL.md)",
                "--- project-doc ---",
                "### Available skills",
                "- plan: use plans (file: /tmp/plan/SKILL.md)",
            ])
            records = [
                {
                    "timestamp": "2026-05-12T00:00:00Z",
                    "type": "session_meta",
                    "payload": {"id": "s1", "cwd": "/repo", "base_instructions": {"text": base_text}},
                },
                token_count("2026-05-12T00:00:01Z", total=100, last=100),
                token_count("2026-05-12T00:00:02Z", total=75, last=20),
                {
                    "timestamp": "2026-05-12T00:00:03Z",
                    "type": "response_item",
                    "payload": {
                        "type": "function_call_output",
                        "output": [
                            {"type": "output_text", "text": "ok"},
                            {"type": "input_image", "image_url": "data:image/png;base64," + "A" * 40},
                        ],
                    },
                },
            ]
            path.write_text("".join(json.dumps(record) + "\n" for record in records), encoding="utf-8")

            report = scan.analyze_session(path, large_threshold=16, top_payloads=5)

            self.assertEqual(report.session_id, "s1")
            self.assertEqual(report.project_doc_sections, 2)
            self.assertEqual(report.available_skill_sections, 2)
            self.assertEqual(report.skill_entry_count, 2)
            self.assertEqual(report.image_payload_count, 1)
            self.assertEqual(report.input_image_mentions, 1)
            self.assertEqual(report.peak_total.total_tokens, 100)
            self.assertEqual(report.final_total.total_tokens, 75)
            self.assertEqual(report.token_total_resets, 1)
            self.assertTrue(report.large_payloads)

    def test_attach_usage_does_not_mix_multiple_accounts_without_filter(self) -> None:
        report = scan.SessionReport(
            path="rollout-test.jsonl",
            bytes=0,
            started_at="2026-05-12T00:00:00Z",
            ended_at="2026-05-12T00:10:00Z",
        )
        usage_entries = [
            (ts("2026-05-12T00:01:00Z"), scan.TokenUsage(total_tokens=100), "acct-a"),
            (ts("2026-05-12T00:02:00Z"), scan.TokenUsage(total_tokens=200), "acct-b"),
        ]

        scan.attach_usage([report], usage_entries)

        self.assertEqual(report.usage_entries, 0)
        self.assertEqual(report.usage_tokens.total_tokens, 0)
        self.assertIn("pass --account-id", report.usage_note or "")

    def test_attach_usage_filters_by_requested_account(self) -> None:
        report = scan.SessionReport(
            path="rollout-test.jsonl",
            bytes=0,
            started_at="2026-05-12T00:00:00Z",
            ended_at="2026-05-12T00:10:00Z",
        )
        usage_entries = [
            (ts("2026-05-12T00:01:00Z"), scan.TokenUsage(total_tokens=100), "acct-a"),
            (ts("2026-05-12T00:02:00Z"), scan.TokenUsage(total_tokens=200), "acct-b"),
        ]

        scan.attach_usage([report], usage_entries, account_id="acct-b")

        self.assertEqual(report.usage_entries, 1)
        self.assertEqual(report.usage_tokens.total_tokens, 200)

    def test_attach_usage_reports_unknown_requested_account(self) -> None:
        report = scan.SessionReport(
            path="rollout-test.jsonl",
            bytes=0,
            started_at="2026-05-12T00:00:00Z",
            ended_at="2026-05-12T00:10:00Z",
        )
        usage_entries = [
            (ts("2026-05-12T00:01:00Z"), scan.TokenUsage(total_tokens=100), "acct-a"),
        ]

        scan.attach_usage([report], usage_entries, account_id="acct-missing")

        self.assertEqual(report.usage_entries, 0)
        self.assertIn("was not found", report.usage_note or "")

    def test_attach_usage_correlates_single_account_without_filter(self) -> None:
        report = scan.SessionReport(
            path="rollout-test.jsonl",
            bytes=0,
            started_at="2026-05-12T00:00:00Z",
            ended_at="2026-05-12T00:10:00Z",
        )
        usage_entries = [
            (ts("2026-05-12T00:01:00Z"), scan.TokenUsage(total_tokens=100), "acct-a"),
        ]

        scan.attach_usage([report], usage_entries)

        self.assertEqual(report.usage_entries, 1)
        self.assertEqual(report.usage_tokens.total_tokens, 100)


def token_count(timestamp: str, *, total: int, last: int) -> dict[str, object]:
    return {
        "timestamp": timestamp,
        "type": "event",
        "payload": {
            "msg": {
                "type": "token_count",
                "info": {
                    "latest_response_model": "test-model",
                    "total_token_usage": {"total_tokens": total, "input_tokens": total},
                    "last_token_usage": {"total_tokens": last, "input_tokens": last},
                },
            }
        },
    }


if __name__ == "__main__":
    unittest.main()
