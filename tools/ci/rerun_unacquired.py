"""Re-run a CI run whose GitHub-hosted job never got a runner: rerun_unacquired.py RUN_ID ATTEMPT

When GitHub's hosted pool is short (macOS arm64 most often), it cancels a queued job it could not hand to a runner
after about 15 minutes, with the annotation "The job was not acquired by Runner of type hosted even after multiple
attempts", whatever the job's timeout-minutes; `build` then fails on a pull request whose code never ran there. This
re-runs that run's failed and cancelled jobs, so the queue is waited out instead of failing the pull request, at most
until attempt MAX_ATTEMPTS. A run with any other failure is left alone: a red that ran code stays red.
"""

import json
import os
import sys
from urllib.request import Request, urlopen

NOT_ACQUIRED = "was not acquired by Runner of type hosted"
MAX_ATTEMPTS = 4


def github(method, path):
    request = Request(
        f"https://api.github.com/repos/{os.environ['GITHUB_REPOSITORY']}/{path}",
        data=b"" if method == "POST" else None,
        headers={"Accept": "application/vnd.github+json", "Authorization": f"Bearer {os.environ['GITHUB_TOKEN']}"},
        method=method,
    )
    with urlopen(request, timeout=30) as response:
        body = response.read()
        return json.loads(body) if body else None


def decide(jobs, annotations, attempt):
    """'rerun', or why not. annotations(job_id) lists a job's check-run annotations."""
    starved = [job for job in jobs if job["conclusion"] == "cancelled" and not job["runner_name"]
               and any(NOT_ACQUIRED in a["message"] for a in annotations(job["id"]))]
    if not starved:
        return "no job of this run was left without a runner"
    # A failure elsewhere means code ran and failed: re-running would hide it. The aggregate (`build`, or `checks of
    # PR #N` on a dispatch) only reports the others.
    real = [job["name"] for job in jobs if job["conclusion"] in ("failure", "timed_out")
            and job["name"] != "build" and not job["name"].startswith("checks of PR #")]
    if real:
        return f"other jobs failed ({', '.join(real)}), so the run stays red"
    if attempt >= MAX_ATTEMPTS:
        return f"attempt {attempt} is the last this re-runs; re-run it by hand once GitHub's queue clears"
    return "rerun"


def main():
    run_id, attempt = sys.argv[1], int(sys.argv[2])
    jobs = github("GET", f"actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100")["jobs"]
    verdict = decide(jobs, lambda job_id: github("GET", f"check-runs/{job_id}/annotations"), attempt)
    if verdict != "rerun":
        print(f"run {run_id} attempt {attempt}: {verdict}")
        return
    names = ", ".join(job["name"] for job in jobs if job["conclusion"] == "cancelled" and not job["runner_name"])
    github("POST", f"actions/runs/{run_id}/rerun-failed-jobs")
    print(f"::notice::run {run_id} attempt {attempt}: GitHub gave {names} no runner; re-running its failed jobs")


if __name__ == "__main__":
    main()
