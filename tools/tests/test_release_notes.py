import importlib.util
import unittest
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location("release_notes", Path(__file__).resolve().parents[1] / "release_notes.py")
notes = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(notes)


def pull(number, title, *labels):
    return {"number": number, "title": title, "labels": [{"name": label} for label in labels]}


class ReleaseNotes(unittest.TestCase):
    def test_features_come_first_and_skipped_pull_requests_are_left_out(self):
        title, body = notes.notes("v1.2.0", [
            pull(7, "Fix the update check offline"),
            pull(8, "Instances keep their own build", "feature"),
            pull(9, "CI: daily ignored tests", "skip-release"),
        ])
        self.assertEqual(title, "Sunrise Launcher 1.2.0")
        self.assertLess(body.index("Instances keep their own build (#8)"), body.index("Fix the update check offline (#7)"))
        self.assertNotIn("#9", body)

    def test_a_release_with_nothing_to_tell_says_so(self):
        self.assertIn("Maintenance release.", notes.notes("v1.2.1", [pull(1, "CI", "skip-release")])[1])

    def test_the_tag_is_cargo_tomls_version_and_never_released_twice(self):
        with mock.patch.object(notes, "version", return_value="1.3.0"), mock.patch.object(notes, "tags", return_value=["v1.2.0"]):
            self.assertEqual(notes.decide("abc"), {"version": "1.3.0", "tag": "v1.3.0", "head": "abc"})
        with mock.patch.object(notes, "version", return_value="1.2.0"), mock.patch.object(notes, "tags", return_value=["v1.2.0"]):
            with self.assertRaises(SystemExit):
                notes.decide("abc")
        # 1.0.0 went out from the game repository: installed copies would never take another 1.0.0.
        with mock.patch.object(notes, "version", return_value="1.0.0"), mock.patch.object(notes, "tags", return_value=[]):
            with self.assertRaises(SystemExit):
                notes.decide("abc")


if __name__ == "__main__":
    unittest.main()
