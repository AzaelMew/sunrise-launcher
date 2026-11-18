"""Keep the launcher's daily game-copy drift issue in sync with the comparison."""

import argparse
import json
import os
import subprocess
from itertools import count
from pathlib import Path
from urllib.request import Request, urlopen

TITLE = "Game copies drifted"
DISCLOSURE = "> 🤖 Filed by Aria's agent swarm, Dyna, not by Aria."


def github(method, path, payload=None):
    request = Request(
        f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}/issues{path}",
        data=json.dumps(payload).encode("utf-8") if payload is not None else None,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {os.environ['GITHUB_TOKEN']}",
            "Content-Type": "application/json",
        },
        method=method,
    )
    with urlopen(request, timeout=30) as response:
        return json.load(response)


def open_issue():
    # List rather than search: the search index may not include a newly created issue yet.
    for page in count(1):
        issues = github("GET", f"?state=open&per_page=100&page={page}")
        for issue in issues:
            if issue["title"] == TITLE and "pull_request" not in issue:
                return issue
        if len(issues) < 100:
            return None


def issue_body(game, pin, head, rows):
    lines = [
        DISCLOSURE, "", f"Launcher pin: `{pin}`  ", f"Sunrise main compared: `{head}`", "",
        "Update the affected copies and the pin together in a launcher pull request.", "",
        "| Table | Game path | Launcher path |", "| --- | --- | --- |",
    ]
    lines.extend(f"| `{table}` | `{path}` | `{copy}` |" for table, path, copy in rows)
    lines.extend(["", "### Suspect Sunrise commits since the pin"])
    for path in dict.fromkeys(path for _, path, _ in rows):
        log = subprocess.check_output([
            "git", "--literal-pathspecs", "-C", game, "log", "--oneline", "--no-decorate", "--no-color",
            f"{pin}..{head}", "--", path,
        ], encoding="utf-8", errors="replace")
        lines.extend(["", f"`{path}`", ""])
        lines.extend(f"    {line}" for line in log.splitlines() or ["No commits in this range touched this path."])
    return "\n".join(lines) + "\n"


def reconcile(game, pin, rows):
    head = subprocess.check_output(["git", "-C", game, "rev-parse", "HEAD"], text=True).strip()
    issue = open_issue()
    if rows:
        body = issue_body(game, pin, head, rows)
        if issue is None:
            issue = github("POST", "", {"title": TITLE, "body": body, "labels": ["bug"]})
        else:
            labels = [label["name"] for label in issue["labels"]]
            github("PATCH", f"/{issue['number']}", {"body": body, "labels": sorted(set(labels) | {"bug"})})
        print(f"{TITLE}: #{issue['number']}")
        return 1
    if issue is not None:
        path = f"/{issue['number']}"
        github("POST", f"{path}/comments", {
            "body": f"🤖 Agent: The game copies match Sunrise main at `{head}`. Closing this drift issue.",
        })
        github("PATCH", path, {"state": "closed", "state_reason": "completed"})
        print(f"Closed #{issue['number']}: the copies match")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("game", type=Path, help="the compared game checkout (with history if there is drift)")
    parser.add_argument("pin", type=Path, help="the launcher's game commit pin file")
    parser.add_argument("report", type=Path, help="the copy check's JSON failure report")
    args = parser.parse_args()
    return reconcile(args.game, args.pin.read_text().strip(), json.loads(args.report.read_text(encoding="utf-8")))


if __name__ == "__main__":
    raise SystemExit(main())
