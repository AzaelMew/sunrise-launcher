# Sunrise modpack refusal captures

The Linux launcher was built at `0c8bb58212ad7ad639533032ac7225963dd800e9` and run in real X11 windows. Game, launcher and user-data directories were isolated, self-updates were disabled, and the registry was a local fixture.

- Home: selected an existing modpack profile, clicked Play, and opened Details. The fixture account only enables the control; no real Microsoft sign-in or game launch occurred. At 800×600 the short message behind Details is clipped by the existing Home layout; Details shows the full refusal at both sizes.
- Terminal: ran actual install, explicit Play, Remove and profile Trash commands against a separate copy of that fixture. The capture harness checked that the existing modpack files did not change. The integration test additionally covers an absent pack and the corresponding MCP routes.
