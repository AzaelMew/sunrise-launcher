# One-click mod install — Sunrise Launcher PR #139

Real Linux launcher renders from [PR #139](https://github.com/sunrise-org/sunrise-launcher/pull/139) at [`0ba35e8d43a000e12bec9e18cf8b83fdbae17ff0`](https://github.com/sunrise-org/sunrise-launcher/commit/0ba35e8d43a000e12bec9e18cf8b83fdbae17ff0), captured under Xvfb with scratch profiles, an invented account, a local registry of invented packages, and a local Nightly/SDK mirror. No real sign-in, game or player data was used. A capture-only patch feeds inputs for the rows marked **state injected**; it does not change the views or their copy. The proposed version dropdown (mock states 18–20) is not in this build.

| Screenshot | What it shows and how it was reached |
|---|---|
| [01 Discover, 1280×800](01-discover-1280x800.png) | Install, download at 40%, Waiting, and an incompatible Nightly. **State injected:** download progress and queued request, so they stay on screen together. The incompatible card is visible with Fits my game off. The fixture packages are unsigned. |
| [01 Discover, Taiga](01-discover-taiga-1280x800.png) | Same state and injection in Taiga. The launcher has no light theme; Taiga is the legacy light setting’s replacement. |
| [01 Discover, 800×600](01-discover-800x600.png) | Compact two-column grid with the same injected download and queue. |
| [01 Discover, 800×600, scrolled](01-discover-scrolled-800x600.png) | Scrolled to the second row of a five-card fixture to show Waiting and the incompatible Nightly in full. On this head the four-card fixture stops scrolling just short of its last row's bottom. Same injected download and queue. |
| [01c Cannot install](01c-cant-install-1280x800.png) | Live fit checks show another Nightly, a takedown reason, and Update beside the mod that needs a newer Sunrise. The local mirror offers a newer build. |
| [02 Getting build tools](02-build-tools-1280x800.png) | Card and strip show the platform's tool download estimate. **State injected:** the build started with no toolchain; no compiler downloaded. |
| [03 Building](03-building-1280x800.png) | Card, progress strip and Stop. **State injected:** a native build worker step; no compiler ran. |
| [04 Installed](04-installed-1280x800.png) | Real install added Lantern Core and updated Flywheel 0.3.0 → 0.4.0. **State injected:** local-registry provenance only; the launcher planned, verified and installed the fixture files. |
| [05 First native install](05-first-native-1280x800.png) | Clicked Install on an invented native package with no prior consent. The package is unsigned and makes no source-review claim. |
| [05 First native install, Taiga](05-first-native-taiga-1280x800.png) | Same real consent prompt in Taiga. |
| [06 Install failed](06-install-failed-1280x800.png) | A real install refused a fixture file after its SHA-256 changed. **State injected:** local-registry provenance only. The launcher shows the red card line as well as a more specific page notice. |
| [06b Install failure Details](06b-install-failed-details-1280x800.png) | Clicked real Details on the preceding failure. Same provenance injection. |
| [06c Install conflict](06c-install-conflict-1280x800.png) | Lumen and Bridge Cranes require incompatible versions of Flywheel, and Lumen also depends on Bridge Cranes. The real install planner reports “Conflicts with Bridge Cranes.” **State injected:** local-registry provenance only. |
| [08 Turn on](08-turned-off-1280x800.png) | Loaded an installed but disabled package and opened Discover. |
| [09 Game running](09-game-running-1280x800.png) | The running-game strip and disabled “Close the game to install” actions. **State injected:** a harmless stand-in process; no game launched. |
| [10 Resolve](10-resolve-1280x800.png) | A real install click found one existing mod locked to the older Flywheel. The sheet names the mod and defaults to “Don't install.” |
| [10b Resolve, two blockers](10b-resolve-two-1280x800.png) | A real install click opened the same sheet with Bridge Cranes and Rail Markers, both locked to the older Flywheel. |
| [11 Install to Creative](11-profiles-1280x800.png) | Opened Discover on the Creative profile in a scratch home with two profiles. |
| [12 Picker: all fit](12-picker-all-fit-1280x800.png) | Compared a scratch profile to another installed Nightly with a fitting package. All four picker shots use the Versions page. |
| [12 Picker: removes mods](12-picker-not-ready-1280x800.png) | Selected an older Nightly with no fitting version of two installed packages. |
| [12 Picker: checking](12-picker-checking-1280x800.png) | A delayed local SDK response kept the real check pending. |
| [12 Picker: unavailable](12-picker-unavailable-1280x800.png) | A missing local SDK made the real check unavailable. |
| [14 Build failed](14-discover-build-failed-1280x800.png) | Discover after a native compile failure. **State injected:** worker result and invented diagnostic; no SDK or compiler was needed. Replaces 07. |
| [14b Build failure Details](14b-build-failed-details-1280x800.png) | Clicked real Details on the preceding failure; reason, log, Copy and Save. Same injected worker result. |
| [15a Removal, build failures only](15a-removal-builds-collapsed-1280x800.png) | Collapsed sheet names the two mods that couldn't be built. **State injected:** pending removal list from a simulated native build result. |
| [15b Removal, Show why](15b-removal-why-1280x800.png) | Clicked Show why on a mixed list: two compile failures and one takedown. **State injected:** pending removal list and invented logs. |
| [15b Removal, 800×600](15b-removal-why-800x600.png) | Same state and injection at the compact size, including both actions. |
| [15c Removal on version switch](15c-version-switch-removal-why-1280x800.png) | Selected an older Nightly and clicked Show why. **State injected:** resulting list with a compile failure and a mod without a matching version. |
| [15d Removal, mixed list collapsed](15d-removal-all-collapsed-1280x800.png) | “3 mods will be removed” and the same names as the expanded list. Same injection as 15b. |
| [16 World question](16-world-question-1280x800.png) | Clicked Keep this build after selecting an older Nightly. The launcher read two invented `level.dat` saves and warned about Seabound Boats. |
| [17 Settings › Mods](17-settings-mods-1280x800.png) | Opened Mods settings with auto-update enabled. |
| [21 Collection, two unavailable defaults](21-collection-partial-1280x800.png) | Opened a real collection from the local registry. Stonecutter Plus and Cloudline are Needed and unavailable. Install is disabled with the reason, and the heading says “7 of 9 selected.” |
| [22 Discover, checking](22-checking-1280x800.png) | The fixture's build manifest leaves its game ID empty, so Discover keeps the card and shows “Checking…” instead of Install. No view or worker-state injection. |
