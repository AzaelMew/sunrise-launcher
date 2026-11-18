# Third-party notices

## cap-std / cap-fs-ext

`crates/smod` (the mod package reader) directly uses `cap-std` and `cap-fs-ext` from
[the cap-std project](https://github.com/bytecodealliance/cap-std) for confined mod file access.
Copyrights are retained by their contributors. Both crates offer Apache-2.0 WITH LLVM-exception,
Apache-2.0 or MIT; the MIT option is reproduced here:

Permission is hereby granted, free of charge, to any
person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the
Software without restriction, including without
limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software
is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice
shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.

## Fonts and icons that ship in the launcher

Every route that ships the launcher (the AppImage, the macOS app, the archives, Windows Setup and a
self-updated Windows install) puts the notices below in its `LICENSES` folder, from the list in
`packaging/notices.txt`.

## Figtree

`assets/fonts/Figtree-*.ttf` is [Figtree](https://github.com/erikdkennedy/figtree) by The Figtree
Project Authors, under the SIL Open Font License 1.1 (`assets/fonts/OFL-Figtree.txt`).

## Geist and Geist Mono

`crates/theme/fonts/Geist-*.ttf` and `GeistMono-Regular.ttf` are
[Geist](https://github.com/vercel/geist-font) by The Geist Project Authors, under the SIL Open Font
License 1.1 (`crates/theme/fonts/OFL-Geist.txt`).

## Bricolage Grotesque

`crates/theme/fonts/BricolageGrotesque*.ttf` is
[Bricolage Grotesque](https://github.com/ateliertriay/bricolage) by The Bricolage Grotesque Project
Authors, under the SIL Open Font License 1.1 (`crates/theme/fonts/OFL-BricolageGrotesque.txt`).

## Pixelify Sans

`crates/theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/theme/icons/pixel/LICENSE-pixelarticons.txt`). `star.svg` is Sunrise's own, drawn on
the same grid.

## Hugeicons

`crates/theme/icons/glass/*.svg` are icons from the free set of
[Hugeicons](https://github.com/hugeicons/hugeicons) (`@hugeicons/core-free-icons` 4.4.0), under
the MIT license (`crates/theme/icons/glass/LICENSE-hugeicons.txt`), exported with a 1.6
stroke in `currentColor`.

`crates/theme/icons/glass/discord.svg` is Discord's logo as drawn by
[Simple Icons](https://github.com/simple-icons/simple-icons) 13.21.0 (CC0 1.0). The logo is Discord's
trademark, used only on the button that opens Sunrise's Discord server.

## Mod loader logos

The launcher shows the logo of the mod loader an instance plays with, and offers the logos as
instance icons.

- `assets/logos/{fabric,quilt,neoforge,forge}.png` are the loaders' logos as the launcher
  mockup on [playsunrise.net/launcher](https://playsunrise.net/launcher) shows them (sunrise-site
  `public/media/launcher/*.webp`, converted to PNG), each its project's own mark:
  [Fabric](https://fabricmc.net/), [Quilt](https://quiltmc.org/) (its art is
  [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/)),
  [NeoForged](https://neoforged.net/) (its art, by Ridanisaurus, is
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)) and
  [Minecraft Forge](https://minecraftforge.net/). They name the loader an instance runs.

Each logo belongs to its project. Showing it says which loader an instance runs; it does not mean
the project endorses Sunrise.

## Sign in with Microsoft

The launcher's sign-in buttons follow Microsoft's published branding guidelines for apps that sign in
with Microsoft accounts
([Sign in with Microsoft branding](https://learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps)).
The four-square Microsoft symbol in `src/microsoft.rs` reproduces the squares, positions and
colours of Microsoft's official `ms-symbollockup_mssymbol_19.svg` from that page, unmodified. Microsoft
and its logo are trademarks of the Microsoft group of companies; Sunrise is not endorsed by Microsoft.
