# Contributing

## Licence and DCO

The launcher's code is under the [PolyForm Shield licence](LICENSE), and so is what you contribute: it comes in under the
same licence it goes out under, and you keep your copyright. There is no CLA. Instead, sign off every commit
(`git commit -s`) to certify the [Developer Certificate of Origin](https://developercertificate.org/): that you wrote it,
or have the right to submit it under the project's licence. The DCO check fails a pull request with a commit that isn't
signed off; `git rebase --signoff origin/main` fixes a branch.

Code that came from Horizon is also available under its original MIT licence ([LICENSES/MIT-Horizon.txt](LICENSES/MIT-Horizon.txt)),
and the `scenery` crate is MIT-licensed (its `Cargo.toml`).

## History

This repository was split out of [sunrise-org/sunrise](https://github.com/sunrise-org/sunrise), where the launcher lived
in `launcher/`, with its history (`git filter-repo`, from `sunrise-org/sunrise` at `d5813c112f0`). Commit messages are
unchanged, so a bare `#N` in a commit from before the split refers to sunrise-org/sunrise, or to Horizon for the oldest
ones. The crates under `crates/` were copied into the launcher from the game's own crates during the split: their
history before the copy stays in sunrise-org/sunrise.
