# Profile and world lifecycle captures

These screenshots were taken from the Linux launcher built at `e149662fc54791640f1b78111b675b1c3ab91c49`, using real X11 windows at 1280×800 and 800×600. Launcher, game and user-data directories were isolated and launcher self-updates were disabled. The executable digest is in `provenance.json`.

- Profile Copy and busy refusal used the New profile dialog. The fixture checked the copied save bytes and confirmed that a refusal did not create a profile.
- The bound Copy picker and result followed launcher Play using a local unsigned game. The launcher published Active; the fixture checked that the copy retained the exact build and received a different identity. At 800×600 the existing Home version selector clips this long unsigned developer label; the picker shows it in full.
- The terminal captures are real CLI World Copy, profile Trash in the test directory, and offline Move. The fixture checked the resulting files and location pointer. The Move progress shown here omits machine paths.
- The case follow-up has no new dialog: a real CLI writer can use an existing case-distinct sibling on a case-sensitive volume, but cannot open an alternate spelling of a root reserved while absent. Both behaviors passed on Linux, including a contender after both spellings already existed.

A signed game, online sign-in, real system Trash, Move across distinct OS accounts and physical power loss were not checked.
