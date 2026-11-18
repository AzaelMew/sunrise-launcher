import unittest

from tools.ci import rerun_unacquired as rerun

NOT_ACQUIRED = [{"message": "The job was not acquired by Runner of type hosted even after multiple attempts"}]


def job(job_id, name, conclusion, runner="GitHub Actions 1"):
    return {"id": job_id, "name": name, "conclusion": conclusion, "runner_name": runner}


def run(macos):
    return [job(1, "route", "success"), job(2, "format", "success"), job(3, "check (linux)", "success"),
            macos, job(5, "build", "failure")]


def annotations(job_id):
    return NOT_ACQUIRED if job_id == 4 else []


class RerunUnacquired(unittest.TestCase):
    def test_a_job_that_never_got_a_runner_is_rerun(self):
        self.assertEqual(rerun.decide(run(job(4, "check (macos)", "cancelled", "")), annotations, 1), "rerun")

    def test_a_dispatch_aggregate_failing_on_it_is_rerun_too(self):
        jobs = run(job(4, "check (macos)", "cancelled", ""))
        jobs[-1]["name"] = "checks of PR #12"
        self.assertEqual(rerun.decide(jobs, annotations, 1), "rerun")

    def test_a_failure_that_ran_code_stays_red(self):
        jobs = run(job(4, "check (macos)", "cancelled", "")) + [job(6, "check (windows)", "failure")]
        self.assertIn("check (windows)", rerun.decide(jobs, annotations, 1))

    def test_other_cancellations_are_not_rerun(self):
        for macos, notes in [
            (job(4, "check (macos)", "cancelled", "GitHub Actions 9"), annotations),  # it had a runner
            (job(4, "check (macos)", "cancelled", ""), lambda job_id: []),  # cancelled, but not for want of a runner
            (job(4, "check (macos)", "failure"), annotations),
        ]:
            with self.subTest(macos=macos):
                self.assertNotEqual(rerun.decide(run(macos), notes, 1), "rerun")

    def test_reruns_stop_at_the_last_attempt(self):
        jobs = run(job(4, "check (macos)", "cancelled", ""))
        self.assertEqual(rerun.decide(jobs, annotations, rerun.MAX_ATTEMPTS - 1), "rerun")
        self.assertIn("by hand", rerun.decide(jobs, annotations, rerun.MAX_ATTEMPTS))


if __name__ == "__main__":
    unittest.main()
