import io
import json
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path
from unittest.mock import patch
from urllib.parse import urlsplit

from tools.ci import game_copies_issue as drift
from tools.tests import test_game_copies as copies


class CopyReport(unittest.TestCase):
    def test_pass_mismatch_and_missing_game_file_are_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            game = root / "game"
            game.mkdir()
            (root / "copy").write_text("same")
            source = game / "source"
            source.write_text("same")
            row = ["SAME_BYTES", "source", "copy"]
            report = root / "report.json"
            with (
                patch.dict(os.environ, SUNRISE_GAME_DIR=str(game)),
                patch.object(copies, "LAUNCHER", root),
                patch.object(copies, "SAME_BYTES", (("source", "copy"),)),
                redirect_stderr(io.StringIO()),
            ):
                for action, expected, rows in [
                    (lambda: None, 0, []),
                    (lambda: source.write_text("changed"), 1, [row]),
                    (source.unlink, 1, [row]),
                ]:
                    action()
                    suite = unittest.TestSuite([copies.LauncherCopies("test_copied_files_are_the_games_byte_for_byte")])
                    self.assertEqual(copies.write_report(report, suite), expected)
                    self.assertEqual(json.loads(report.read_text()), rows)

    def test_unconfigured_checkout_is_not_a_pass_or_a_drift_report(self):
        with tempfile.TemporaryDirectory() as directory:
            report = Path(directory) / "report.json"
            for game_dir in ("", str(Path(directory) / "missing")):
                with patch.dict(os.environ, SUNRISE_GAME_DIR=game_dir), redirect_stderr(io.StringIO()):
                    suite = unittest.defaultTestLoader.loadTestsFromTestCase(copies.LauncherCopies)
                    self.assertEqual(copies.write_report(report, suite), 2)
                self.assertFalse(report.exists())


class DriftIssue(unittest.TestCase):
    def git(self, *args):
        return subprocess.check_output(
            ["git", "-C", self.game, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.com",
             "-c", "commit.gpgSign=false", *args],
            text=True,
        ).strip()

    def commit(self, path, subject):
        (self.game / path).write_text(subject)
        self.git("add", path)
        self.git("commit", "--quiet", "-m", subject)
        return self.git("rev-parse", "HEAD")

    def setUp(self):
        self.game = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.git("init", "--quiet")
        self.pin = self.commit("shared.rs", "Original copy at the pin")
        self.commit("other.rs", "Unrelated change")
        self.head = self.commit("shared.rs", "Change the shared file")
        self.enterContext(patch.dict(os.environ, GITHUB_TOKEN="test-token", GITHUB_REPOSITORY="example/launcher"))
        self.urlopen = self.enterContext(patch.object(drift, "urlopen"))

    def respond(self, *responses):
        self.urlopen.reset_mock()
        self.urlopen.side_effect = [io.BytesIO(json.dumps(response).encode()) for response in responses]

    def requests(self):
        requests = [call.args[0] for call in self.urlopen.call_args_list]
        for request in requests:
            self.assertEqual(request.get_header("Authorization"), "Bearer test-token")
            self.assertEqual(urlsplit(request.full_url).netloc, "api.github.com")
        return [(request.method, request.full_url.split("/repos/example/launcher/issues", 1)[1],
                 json.loads(request.data) if request.data else None) for request in requests]

    def test_open_update_close_then_leave_a_passing_run_alone(self):
        listing = ("GET", "?state=open&per_page=100&page=1", None)
        current = {"title": "Game copies drifted", "number": 7, "labels": [{"name": "bug"}]}
        pull = {"title": "Game copies drifted", "number": 99, "pull_request": {}}
        rows = [["SAME_CODE", "shared.rs", "copy.rs"], ["SAME_ITEMS", "shared.rs", "other/copy.rs"]]
        self.respond([pull], current)
        self.assertEqual(drift.reconcile(self.game, self.pin, rows), 1)
        get, (method, path, created) = self.requests()
        self.assertEqual(get, listing)
        self.assertEqual((method, path), ("POST", ""))
        self.assertEqual(created["title"], "Game copies drifted")
        self.assertEqual(created["labels"], ["bug"])
        body = created["body"]
        self.assertEqual(body.splitlines()[0], "> 🤖 Filed by Aria's agent swarm, Dyna, not by Aria.")
        for snippet in [self.pin, self.head, "`SAME_CODE` | `shared.rs` | `copy.rs`", "`SAME_ITEMS` | `shared.rs` | `other/copy.rs`"]:
            self.assertIn(snippet, body)
        self.assertIn(self.head[:7], body)
        self.assertEqual(body.count("Change the shared file"), 1)
        self.assertNotIn("Unrelated change", body)
        self.assertNotIn("Original copy at the pin", body)

        later = self.commit("shared.rs", "Change the shared file again")
        self.respond([pull, current], current)
        self.assertEqual(drift.reconcile(self.game, self.pin, rows[:1]), 1)
        get, (method, path, updated) = self.requests()
        self.assertEqual((get, method, path), (listing, "PATCH", "/7"))
        self.assertEqual(updated["labels"], ["bug"])
        self.assertIn(later, updated["body"])
        self.assertIn("Change the shared file again", updated["body"])
        self.assertNotIn("SAME_ITEMS", updated["body"])

        self.respond([current], {}, {})
        self.assertEqual(drift.reconcile(self.game, self.pin, []), 0)
        get, (method, path, comment), close = self.requests()
        self.assertEqual((get, method, path), (listing, "POST", "/7/comments"))
        self.assertTrue(comment["body"].startswith("🤖 Agent:"))
        self.assertIn(later, comment["body"])
        self.assertEqual(close, ("PATCH", "/7", {"state": "closed", "state_reason": "completed"}))

        self.respond([])
        self.assertEqual(drift.reconcile(self.game, self.pin, []), 0)
        self.assertEqual(self.requests(), [listing])

    def test_lookup_checks_later_pages(self):
        current = {"title": "Game copies drifted", "number": 101, "labels": []}
        self.respond([{"title": "Unrelated issue"}] * 100, [current])
        self.assertEqual(drift.open_issue(), current)
        self.assertEqual(self.requests(), [
            ("GET", "?state=open&per_page=100&page=1", None),
            ("GET", "?state=open&per_page=100&page=2", None),
        ])
