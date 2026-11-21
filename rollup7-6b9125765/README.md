# Identical mod labels

Source: `6b9125765fc186b46b3a67f4b96c08b914ae14f5`. Linux launcher executable SHA-256: `fe44b71a14ed01da3be33198180392d74d8a4f48224ab8c9ef3aab45ec90ec89`.

| State | 800×600 | 1280×800 |
|---|---|---|
| Installed shows `Lanterns · 1` and `Lanterns · 2` | [Compact](names/installed-identical-800x600.png) | [Wide](names/installed-identical-1280x800.png) |
| Remove uses the same label for the selected archive and its dependent | [Compact](names/remove-identical-800x600.png) | [Wide](names/remove-identical-1280x800.png) |

There are two regular local archives and a valid, off, unpacked mod. All three have the same display metadata. The unpacked mod's ID sorts before both archives, but it has no Installed row and does not affect the numbering. The first visible archive is `Lanterns · 1` both in the list and in `Remove Lanterns · 1?`; the review says that `Lanterns · 2` will be turned off and its files kept. Escape preserved the profile pointer, rows, enable list, both archives and the unpacked manifest.
