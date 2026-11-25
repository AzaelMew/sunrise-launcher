import io
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

from tools.ci import structure_check as check

BASE_FILES = {
    "src/main.rs": "fn main() {}\n",
    "src/net.rs": (
        "pub fn get() -> Result<Vec<String>, String> { Ok(read().unwrap()) }\n"
        "pub fn parse() -> Result<u8, std::io::Error> { todo!() }\n"
    ),
    "tools/ci/manual-tests.txt": "# comment\nsunrise-launcher net::tests::live_get by-hand manual # network\n",
}


class Repository:
    def __init__(self, root: Path):
        self.root = root
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")

    def git(self, *args: str) -> str:
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, files: dict[str, str | None]) -> str:
        for path, text in files.items():
            target = self.root / path
            if text is None:
                target.unlink()
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text)
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", "change")
        return self.git("rev-parse", "HEAD")


class StructureCheck(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Repository(Path(directory.name))
        self.base = self.repo.commit(BASE_FILES)
        self.addCleanup(os.chdir, Path.cwd())
        os.chdir(self.repo.root)

    def run_check(self, head: str) -> tuple[int, str]:
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(io.StringIO()):
            code = check.main(["--base", self.base, "--head", head])
        return code, out.getvalue()

    def test_the_base_is_counted(self):
        counts = check.measure(self.base)
        self.assertEqual(counts["src_files"], 2)
        self.assertEqual(counts["unwraps"], 1)
        self.assertEqual(counts["string_errors"], 1)
        self.assertEqual(counts["stale_manual_tests"], 1)

    def test_a_count_going_up_fails(self):
        head = self.repo.commit({
            "src/glass_extra.rs": "#[allow(dead_code, clippy::too_many_arguments)]\nfn f() { x.unwrap(); }\n",
            "src/lib.rs": '#[path = "glass_extra.rs"]\nmod extra;\n' + "\n" * check.LONG_FILE,
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 1)
        counts = check.measure(head)
        self.assertEqual(counts["src_files"], 4)
        self.assertEqual(counts["long_files"], 1)
        self.assertEqual(counts["path_attributes"], 1)
        self.assertEqual(counts["allow_dead_code"], 1)
        self.assertEqual(counts["allow_clippy"], 1)
        self.assertEqual(counts["unwraps"], 2)
        self.assertEqual(counts["retired_names"], 1)
        self.assertIn("went up", out)

    def test_a_count_going_down_passes(self):
        head = self.repo.commit({
            "src/net.rs": "pub fn get() -> Result<Vec<String>, Error> { read() }\n#[test]\nfn live_get() {}\n",
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 0)
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"], counts["stale_manual_tests"]), (0, 0, 0))
        self.assertNotIn("went up", out)

    def test_test_code_is_not_counted(self):
        test_code = "fn t() -> Result<(), String> { x.unwrap(); Ok(()) }\n"
        head = self.repo.commit({
            "src/net.rs": BASE_FILES["src/net.rs"] + "#[cfg(test)]\nmod tests {\n" + test_code + "}\n",
            "src/net/tests.rs": test_code,
            "src/mods/net_tests.rs": test_code,
            "src/mods/test_support.rs": test_code,
            "src/mods/test_support/fixtures.rs": test_code,
            "tests/play.rs": test_code,
            "crates/smod/tests/index.rs": test_code,
        })
        code, out = self.run_check(head)
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"]), (1, 1))
        self.assertEqual(code, 0, out)


if __name__ == "__main__":
    unittest.main()
