# Sunrise Launcher

This is Sunrise's launcher, not the official Horizon launcher. It keeps its own
settings, game data and account storage; on its first start it copies the Horizon
launcher's (or the arismoko fork launcher's) settings, accounts and worlds, leaving
theirs as they are. It installs and updates only from
[sunrise-org/sunrise](https://github.com/sunrise-org/sunrise).

Microsoft sign-in is configured using the application ID upstream authorized the
fork to use. Signed Nightly builds require a signed-in account that owns Minecraft.
Accounts it signs in are stored under its own name, apart from the official launcher's.

## Downloads

- **Linux x86_64:** download `sunrise-launcher-linux-x86_64.AppImage`, make it
  executable and run it. The `.tar.gz` archive is also available.
- **Windows x64:** download `sunrise-launcher-setup.exe`, or extract
  `sunrise-launcher-windows-x64.zip` and run `sunrise-launcher.exe`.
  This fork is **not Authenticode-signed**. Expect a SmartScreen or unknown-publisher
  warning; signed checksums do not remove Windows warnings.
- **macOS, Apple Silicon:** download `sunrise-launcher-macos-arm64.dmg` and drag
  Sunrise Launcher to Applications. It is signed ad hoc and **not notarized**: open it
  once, then allow it under System Settings › Privacy & Security › Open Anyway.

These launcher releases contain Linux x86_64, macOS Apple Silicon and Windows x64 packages.
The launcher checks signed updates from this fork automatically.
Older launchers build native mods from source against newer Nightlies until they update.

## Verify the download

Download `SHA256SUMS`, `SHA256SUMS.sig` and all the packages from this release
into one folder. The signature binds the checksums to this fork and the exact
release tag; comparing a checksum alone does not authenticate the download.

With Rust installed, use the verifier from the fork's tagged source checkout:

```sh
git clone --branch '{tag}' --depth 1 https://github.com/sunrise-org/sunrise.git horizon-fork-source
cd horizon-fork-source
cargo run --locked -p horizon-release-sign -- verify --repository sunrise-org/sunrise --tag '{tag}' --publishing /absolute/path/to/downloads
```

The verifier uses the fork's built-in public keys and checks every package
against the signed manifest. An invalid signature, a wrong release tag, a missing
package or a changed package is refused. The launcher performs the corresponding
signature and checksum checks automatically for each download.
