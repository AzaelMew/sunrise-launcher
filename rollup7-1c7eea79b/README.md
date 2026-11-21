# Nightly retry and identical mod names

Source: `1c7eea79bc8bccd6dd73bc47aa3552f1527c55ef`. Linux launcher executable SHA-256: `0b2061243d87c674a24dd513c8a565b14268416dcf8ea355e4f70123376b0f99`.

| State | 800×600 | 1280×800 |
|---|---|---|
| Nightly is installed; **Try again** records it as the newest Nightly | [Compact](receipt/nightly-receipt-800x600.png) | [Wide](receipt/nightly-receipt-1280x800.png) |
| The notice clears after a successful retry | [Compact](receipt/nightly-retried-800x600.png) | [Wide](receipt/nightly-retried-1280x800.png) |
| Installed distinguishes equal names, authors, versions and sources | [Compact](names/installed-identical-800x600.png) | [Wide](names/installed-identical-1280x800.png) |
| Remove uses the same ordinal labels and keeps the world warning | [Compact](names/remove-identical-800x600.png) | [Wide](names/remove-identical-1280x800.png) |

Versions downloaded and verified the published signed Nov 20 Nightly. The first **Try again** left the build installed and the notice visible while the record path was still blocked. After the obstruction was removed, the same button wrote the record and cleared the notice; the installed image, archive proof, settings, and profile selection did not change. The OS error appeared only in the launcher log. The game was not started.

The two Lanterns are separate valid archives with identical display metadata from an isolated local registry fixture. Installed and Remove show the same ordinals with no paths or hashes. Escape preserved the recorded mod state and archive bytes.
