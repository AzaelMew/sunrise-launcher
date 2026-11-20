# Launcher rollup 5 (#138, head f809e472): screenshots

Real debug build of `f809e472` (`cargo build`), run under Xvfb with scratch folders only, at 1280x800 and 800x600.
Launcher is signed out. Scratch files were written by a script (worlds, servers.dat, packs, mod files);
a second script plants a newer launcher release (v9.9.9) in the scratch GitHub answer cache, so the update row can show.
File names: `<pr>-<what>-<W>x<H>.png`. At 800x600 the Home list and the data pack list are cut off by the window height, and the proxy box shows only the tail of the 80-character line (the box is narrower there).

| Image | PR | State shown and how it was reached |
|---|---|---|
| 127-add-server-bad-address | #127 | Servers > Join: typed `example.com:notaport`, pressed Save. The new error shows and nothing is saved. |
| 127-servers-icon-no-padding | #127 | Servers > Join, with a servers.dat whose icon has its trailing `=` removed: "Shown One" shows its picture. Hidden entry is not listed. |
| 127-create-server-copy-world-list | #127 | Servers > Host > Create a server > Copy one of my worlds. Lists worlds by their real name (folder `Twin A` is "Twin Alpha"), newest LastPlayed first, equal ones by folder name; a folder with a garbage level.dat and one with only level.dat_old are not listed or preselected. |
| 127-home-worlds-and-135-mod-rows | #127, #135 | Home. Worlds read from level.dat with short, double and float tags (Hardcore, Adventure, Creative modes and dates), equal LastPlayed in folder order. Mods card: working `Ruby` and unreadable `ruby` rows (1280x800 only shows both). |
| 135-mods-unreadable-file-row | #135 | Mods > Installed with a corrupt `ruby.smod` beside a working mod `ruby` that is on: the corrupt file shows off, switch disabled; the working mod keeps its own state ("Needs permission" is the unrelated native build consent). |
| 136-resource-packs-list | #136 | Mods > Resource, shader and data packs > Resource packs. Seeded `Good.zip`, a folder with BOM and comments in pack.mcmeta, `Renamed.jar`, `Upper.ZIP`, `Paused.zip.disabled`, and `NoDescription.zip`. Only the first two are listed. |
| 136-data-packs-list | #136 | Same page > Data packs. In a world: a zip, a folder with BOM and comments (both listed) and a zip with `{"pack":{}}` (not listed). |
| 130-open-folder-toast | #130 | Home, pressed the folder button: the "Opened <path>" card shows at the bottom (copy and open actions). |
| 130-proxy-box-long-line | #130 | Settings > Launcher: typed an 80-character proxy list; the box shows it whole at 1280x800 (it used to stop at 44). |
| 130-launcher-update-row-package-manager | #130 | Settings > Launcher with `SUNRISE_LAUNCHER_NO_SELF_UPDATE=1` and a (planted) newer release: "Update this launcher with your package manager." beside the disabled button. |

## Not visible (no player-visible change)
- #126: removal of lease-less versions, SOCKS child env, prune guard for native bases, repeated progress lines (CLI output only).
- #128: update over a missing or unparsable mod file, server jar sha1 check (error paths and files only; needs a failing install or network fault).
- #129: ":" in Modrinth names, report temp files, store refill, macOS wasm std, Store Java folder, GitHub link case, cmd junction paths.
- #132: exports leave out servers.dat, player data and UUIDs (zip contents only).
- #133: macOS jre.bundle Java, Windows ComSpec (platform build paths).
- #134: settings.json mode 0600, one-line server_command for AI agents, Windows build folder owner check.
- #130: the `prepare --download` "Sign in with the Microsoft account..." line is CLI text.

## Not shown
- #123: child-account sign-in error with code and family link. XSTS answers come from xsts.auth.xboxlive.com over TLS; the launcher's only preview hook (`SUNRISE_MSA_MICROSOFT_ENDPOINT`) covers the Microsoft endpoint, not Xbox or XSTS, and a real account is not allowed. Covered by the `refusals_have_messages` unit test only.
- #129: Play button steps ("Checking game files...", "Updating mods...", "Signing in...") need a signed-in account and a game start.
- #129: "Reading the mods you added by hand..." line on Mods > Installed is on screen for a fraction of a second with a small folder; not captured.
- #136: the refusal messages when importing `Upper.ZIP` or a pack without a description need a file picker or a drag and drop, which Xvfb cannot give; the list effect is shown above.
- #127: eula.txt parsing (needs a server start), servers.dat write order (file only).
