# Packs tab (#140)

Real launcher captures from `f59e9821f`, based on `7fc04e63f`. No capture-only source patch. Default Sunrise theme and an isolated profile with live catalogue results.

| Image | State |
| --- | --- |
| `packs-1280x800.png` | Click Packs: Discover, with all three pack types. |
| `packs-800x600.png` | Click Packs at the minimum window size, with the default collapsed sidebar. |
| `packs-expanded-sidebar-800x600.png` | Expand the sidebar with Ctrl+B at the minimum size; the header uses text-only tabs. |
| `packs-installed-1280x800.png` | Open Installed; Packs stays selected. |
| `mods-1280x800.png` | Mods is selected with the puzzle glyph and no subtitle link. |
| `search-packs-1280x800.png` | Ctrl+K → Packs shows one Go to result. |
| `packs-lt-expanded-sidebar-800x600.png` | Lithuanian signed-out header: all five tabs remain readable, and the sign-in button shortens its label. |
| `packs-lt-account-expanded-sidebar-800x600.png` | Lithuanian header with a synthetic saved account: the account chip yields space to the tabs. |

Collapsing the sidebar again at 800×600 and opening Packs with Enter from command search at 1280×800 reproduced the respective direct-click renders byte for byte.
