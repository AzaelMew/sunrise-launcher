<!--
Fill in what applies and delete the rest, including these comments.

Opened by an AI agent? Copy this line out of the comment to the very first line of the PR body
before you delete the comment, so it shows on GitHub. Say who opened the PR and on whose behalf:
> 🤖 Opened by <name>'s AI agent, not by <name> personally.
-->

## What changed

<!-- What a player or a developer notices, then how it works. Link what it fixes: "Fixes #123" or "Fixes sunrise-org/sunrise#123". -->

## Why

<!-- The problem or the cause, and why this way. For a bug, what went wrong and how you found it. -->

<!-- Stacked or split work: "Based on #123" or "Extracted from #123 (`abc1234`)", and what has to merge first. -->

## Self-check

- Head: `<full commit SHA>`
- Local: <!-- what you ran and on which OS, e.g. `cargo fmt --all --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked --workspace --lib --bins` (CI picks the integration tests per platform) -->
- Manual: <!-- what you tried in the running launcher -->
- Not checked: <!-- what you couldn't or didn't test (other platforms, a live server, a signed game), or "nothing" -->

<!-- UI or design changes: a before and an after of each state it changes (not just each screen),
at 1280×800 and 800×600, dark and light where they differ. Take the before on `main`, at the same
window size and in the same state as the after. For stacked work, take the before on the head of the
PR it builds on, so it shows only what this PR changes. A state that's new has no before: say so.

### Captures

| Screen | Before | After |
|---|---|---|
| … at 1280×800 | … | … |
| … at 800×600 | … | … |
| new state | none (new) | … |
-->

<!--
Before you open it:
- Every commit is signed off by its author (`git commit -s`). For your own commits,
  `git rebase --signoff origin/main` adds a missing sign-off; it can't sign off for someone
  else's commit, so ask that author instead. The DCO check fails without it.
- Copies of game files changed? Update `tools/ci/sunrise-commit` in the same PR (CONTRIBUTING.md).
- Not ready for review yet? Open it as a draft.
-->
