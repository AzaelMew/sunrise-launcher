#!/usr/bin/env bash
# Usage: copy-notices.sh <repository root> <folder to put the notices in>
# Copies every file of packaging/notices.txt into the folder, under its listed name.
set -euo pipefail
repo=$1
dest=$2
mkdir -p "$dest"
while read -r name source; do
  case $name in '' | '#'*) continue ;; esac
  cp "$repo/$source" "$dest/$name"
done < "$repo/packaging/notices.txt"
