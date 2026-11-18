#!/usr/bin/env bash
# Fetch the game inputs for test_game_copies.py into a new or empty directory.
set -euo pipefail

if [[ $# != 2 || ! $1 =~ ^[0-9a-f]{40}$ ]]; then
    echo "Usage: $0 SHA DIR (full Sunrise commit SHA)" >&2
    exit 2
fi

sha=$1
dir=$2
launcher=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)

export GIT_TERMINAL_PROMPT=0

git init --quiet -- "$dir"
git -C "$dir" remote add origin https://github.com/sunrise-org/sunrise
git -C "$dir" fetch --quiet --no-tags --depth 1 --filter=blob:none origin "$sha"
python3 "$launcher/tools/tests/test_game_copies.py" --paths |
    sed 's|^|/|' |
    git -C "$dir" sparse-checkout set --no-cone --stdin
git -C "$dir" checkout --quiet --detach FETCH_HEAD
