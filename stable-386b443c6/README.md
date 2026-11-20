# Stable channel follow-up renders

Source: `386b443c6cf9eb9da09d72ddfdd5fbc32535002d`. These are screenshots of the real Linux launcher in isolated fixture data, 1280×800 and 800×600.

- `nightly-*`: unmodified binary from that source. The catalogue has no offered builds. The selected global Stable install never rescues the existing unpinned Nightly profile. With two same-day Nightlies and a verified rolling-install receipt, the exact current build is displayed as the Play target in Versions.
- `architecture-*`: fixture build of the same source, supplying a simulated Windows ARM host, ARM64 release assets, and a synthetic transfer progress/error result. The real Versions UI and repair eligibility code are used. Clicking Repair asserted the tag was the installed `26.3-0.1.0`, not the offered `26.3-0.1.1`. Windows ARM GUI and production transfers were not exercised.
- `native-held-*`: same-source fixture supplies a synthetic native fit result to the actual Build picker. With no live catalogue, Home and the picker both report the hold on Stable `0.1.1`, the newer installed build on the native pin's base channel.

All accounts, builds, release dates, mods, and compatibility outcomes shown here are fixtures. No production sign-in or game download took place.
