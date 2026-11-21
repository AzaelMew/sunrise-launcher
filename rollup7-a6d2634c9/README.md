# Rollup 7 final warning captures

These are Linux screenshots of launcher source `a6d2634c929f038d36ce9d02970899a9af127122`. The executable SHA-256 is `203ec0b03e76fabad109122158d2a48b1ceb0c74fd80d13d0bf6cedf376d1506`.

| State | 800×600 | 1280×800 |
|---|---|---|
| Update cannot confirm Sunrise registry origin | [Compact](refusal/registry-unconfirmed-800x600.png) | [Wide](refusal/registry-unconfirmed-1280x800.png) |
| Remove disambiguates the two Lanterns by profile-relative archive name and shows Fireflies by name only | [Compact](remove/remove-duplicate-names-800x600.png) | [Wide](remove/remove-duplicate-names-1280x800.png) |
| Nightly is installed even though the rolling receipt could not be saved | [Compact](receipt/nightly-receipt-800x600.png) | [Wide](receipt/nightly-receipt-1280x800.png) |

The mod dialogs use an isolated profile with generated mod archives and saves. The refusal and Escape from Remove preserved the committed pointer, mod list, rows and archive bytes. The receipt warning is from the built-in Versions download of the published, signed `nightly-2026-11-20-9425651`: a directory at the receipt location forced the write to fail after verified publication. The installed row still offers Prepare. No game was launched.
