#!/usr/bin/env bash
# Writes SHA256SUMS for the files given and signs it with a throwaway key, for dry runs and tests: SHA256SUMS.sig over "sunrise-launcher release <tag>\n" + SHA256SUMS, the key's public half in ephemeral.pub.
# No real key is ever here: sunrise-org/sunrise's launcher-bridge.yml signs real releases with the game's RELEASE_SIGNING_KEY, the same message.
#   tools/ci/sign_release.sh v1.1.0 dist/sunrise-launcher-linux-x86_64.tar.gz dist/sunrise-launcher-windows-x64.zip ...
set -euo pipefail
tag=$1; shift
dir=$(dirname "$1")
openssl=${OPENSSL:-openssl}
if command -v sha256sum > /dev/null; then sha=(sha256sum); else sha=(shasum -a 256); fi
umask 077
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

names=()
for file in "$@"; do
  [[ "$(dirname "$file")" == "$dir" ]] || { echo "$file: every file must be in $dir" >&2; exit 1; }
  names+=("$(basename "$file")")
done
(cd "$dir" && "${sha[@]}" -- "${names[@]}") > "$dir/SHA256SUMS"
{ printf 'sunrise-launcher release %s\n' "$tag"; cat "$dir/SHA256SUMS"; } > "$work/message"
"$openssl" genpkey -algorithm ed25519 -outform DER -out "$work/key"
"$openssl" pkey -inform DER -in "$work/key" -pubout -outform DER | tail -c 32 | base64 > "$dir/ephemeral.pub"
"$openssl" pkeyutl -sign -rawin -inkey "$work/key" -keyform DER -in "$work/message" -out "$work/signature"
# One base64 line of the 64 bytes (GNU base64 wraps at 76 characters).
{ base64 < "$work/signature" | tr -d '\n'; echo; } > "$dir/SHA256SUMS.sig"
echo "$tag: SHA256SUMS lists ${#names[@]} files, signed with a throwaway key"
