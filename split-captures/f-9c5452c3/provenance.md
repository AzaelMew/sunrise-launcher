# Profile and world lifecycle captures

These screenshots were taken from the Linux launcher built by the real CLI lifecycle test target at `9c5452c33fe1dbfd9a86bd3bd2c8cdae28111691`, on a real X11 window at each listed size. Launcher, game and user-data directories were isolated and self-updates were disabled.

- Profile Copy and busy refusal were driven through the New profile dialog. The harness checked the copied save bytes and that refusal did not create an instance.
- The bound Copy picker and result followed a normal launcher `play` invocation using a local unsigned game fixture. The launcher itself published Active; the harness checked that the copy retained the same build and received a different profile identity. The existing Home version selector clips the long unsigned developer label at 800×600; the picker itself shows the full build at both sizes.
- The terminal captures are real CLI World Copy, profile Trash (isolated fixture), and offline Move runs. The harness verified the new files and location pointer. Relocation progress omits machine paths in these captures.

No signed game, online sign-in, system Trash or physical power loss was used.
