#!/bin/sh
# Linux preview only. Never register the app, replace the stable launcher, or isolate
# player data. A failed copy must never look like a completed pre-preview backup.
set -eu
umask 077
state="$HOME/.local/share/sunrise/launcher"
backup="$HOME/.local/share/sunrise/launcher.pre-preview-backup"
if [ -L "$state" ] || [ -L "$backup" ]; then
    printf '%s\n' 'Refusing a symlink at the launcher state or backup path.' >&2
    exit 1
fi
if [ ! -d "$backup" ]; then
    [ -d "$state" ] || { printf '%s\n' "No launcher state to back up at $state" >&2; exit 1; }
    tmp=$(mktemp -d -- "${backup}.XXXXXX")
    trap 'rm -rf -- "$tmp"' EXIT
    cp -a --reflink=auto -- "$state/." "$tmp/"
    chmod 700 -- "$tmp"
    mv -Tn -- "$tmp" "$backup"
    # With -n, a concurrent completed backup wins and the staging copy stays ours.
    if [ -d "$tmp" ]; then rm -rf -- "$tmp"; fi
    trap - EXIT
fi
[ -d "$backup" ] || { printf '%s\n' "Backup missing at $backup" >&2; exit 1; }
export SUNRISE_LAUNCHER_NO_SELF_UPDATE=1
exec "$(dirname -- "$0")/sunrise-launcher"
