# Rollup 7 launcher captures

Source: `1fecadf299fc407149d7c24211093dce1a403160`. Linux launcher SHA-256: `903028b7ddff6a0fb570c200ed40d19afc4298964a118eb613a0ca0c826c3ddd`. Each state is captured at 1280×800 and 800×600.

- **Home and the build picker (`empty/`, `stable/`):** Empty Mods, settled Stable, an older Stable profile with a legacy pin and a newer cached Stable, a Stable profile playing a committed Nightly fallback, and a profile already on the newer Stable. The picker distinguishes a cached build from the one the profile will play. The release catalogue, account display and committed profile inputs here are local fixtures, not live signed game or Microsoft sessions.
- **Installed and the rejected update (`override/`):** Compact versions start beneath the names. A local override can advertise an update but cannot authorize it; the refused action left the pointer, browser rows, enable list and archives byte-for-byte unchanged.
- **Remove (`remove/`):** The warning names Lantern Core, its two required dependents and three generated saves. Escape preserved the exact committed state. Approving removed only Lantern Core; both dependents stayed installed and byte-identical with their toggles off.
- **Sunrise registry Update (`update/`):** The launcher fetched the built-in registry over live HTTPS, downloaded and checked the published Frontier 1.0.1 archive, and showed the warning for its disabled 1.0.0 predecessor and three generated saves. Escape preserved the pointer, browser rows, enable list and archive bytes. The fixture used an unsigned dummy game, and no Frontier code was built or run.

The GitHub-specific warning renderer has a copy test but no reachable GitHub Update button in this checkout; it is not shown here. No native Windows or macOS run is represented.
