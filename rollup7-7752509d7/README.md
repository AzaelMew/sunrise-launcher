# Launcher rollup 7 — real Linux captures

Captured from the executable built at `095d78a7160395cf14dbb1c43e292a75bdc5855d`, SHA-256 `a35f01a59650e18ad8a087ce94fd7847ed79e67d0a108430812e5363898e42b5`. The DCO-only rewrite to `7752509d786d5a01439669787f716c97560fcc5e` has the identical Git tree `6da3a079b8cb27d9793db8bc7f383d62c6ac954a`.

| State | 1280×800 | 800×600 |
| --- | --- | --- |
| Compact Home, long build label and empty Mods | [Wide](empty-home-1280x800.png) | [Narrow](empty-home-800x600.png) |
| Stable Home without a newer release | [Wide](stable-home-1280x800.png) | [Narrow](stable-home-800x600.png) |
| Stable Update offered while 0.1.0 remains the profile base | [Wide](stable-update-1280x800.png) | [Narrow](stable-update-800x600.png) |
| Update has opened the chooser; 0.1.0 is still active | [Wide](stable-update-picker-1280x800.png) | [Narrow](stable-update-picker-800x600.png) |
| Installed, before a mutation | [Wide](installed-1280x800.png) | [Narrow](installed-800x600.png) |
| The local index can show a newer version, but cannot authorize replacement | [Wide](override-refused-1280x800.png) | [Narrow](override-refused-800x600.png) |
| Remove review: exact archive, two dependents, three saved worlds | [Wide](remove-warning-1280x800.png) | [Narrow](remove-warning-800x600.png) |
| After Escape, all three mods are still installed and on | [Wide](cancelled-1280x800.png) | [Narrow](cancelled-800x600.png) |
| After Remove commits, both dependents are still installed and off | [Wide](removed-1280x800.png) | [Narrow](removed-800x600.png) |

The real launcher ran in isolated homes. Home/chooser use synthetic account display metadata, release metadata, and a display-only profile pointer; no sign-in, game preparation, or Switch was attempted in those cases. The Installed scenarios use an unsigned developer game, ordinary registry Install against a local index, and three generated saves. The override refusal and Escape preserved the exact pointer, rows, enable file and archive bytes. The approved Remove waited for the journal to settle and verified the exact dependent archive bytes survived, both off. Positive official-registry, native-fit and Switch paths were tested separately; this capture run did not grant a local index official authority.
