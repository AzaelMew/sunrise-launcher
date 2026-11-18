//! The launcher's paint: every colour a screen draws with, as named roles. A screen asks
//! [`palette`] for a role (`palette().text`), never for a hex value or a dark/light branch.
//!
//! The roles are Xmilon's 1.0.x glass material (a theme-tinted plate over the scenery, white
//! text, one accent) under the names of the theme-file contract, for the player's theme and
//! accent. [`begin`] sets the palette once per frame from the settings and the theme in use;
//! [`Palette::of`] computes it. The accent the player (or the theme) picked is never changed:
//! [`Palette::accent`] is that colour, and each role that must read is a separate fit of it, toward
//! white or black (`fit`), so a pale or dark accent keeps its character.
//!
//! Every `u32` is an opaque `0xRRGGBB`, so a field, a menu, a selection or a tooltip reads on
//! its own, whatever is behind it. Only the plates are see-through: [`Palette::card`] and
//! [`Palette::chrome_plate`] take the theme's tint at an opacity ([`Palette::alpha`]) that never
//! falls below what text on them needs over the theme's brightest sky, and is 1 when the scenery
//! is off.
use crate::theme::Theme;
use gpui::{rgb, Hsla};
use std::cell::Cell;
use sunrise_launcher::backend::Settings;
use sunrise_launcher_theme::contrast;
use sunrise_launcher_theme::glass::{fill, SCRIM};
use sunrise_launcher_theme::night::{over, Night};

/// What a dialog's scrim may dim a bright page by: the opacity is the one in this range that
/// sets the dialog furthest apart from the dimmed page.
const SCRIM_ALPHAS: std::ops::RangeInclusive<u32> = 45..=75;
/// The brightest pages a scrim is judged over (`BRIGHT_BACKDROP`, and white).
const BRIGHT: [u32; 2] = [sunrise_launcher_theme::BRIGHT_BACKDROP, 0xffffff];
/// The glass plates' own opacity, as the mockup's `.hzl-glass` mixes it (`Glass::new`).
const PLATE_BASE: f32 = 0.81;
/// Text reads at this contrast on its surface; edges, marks and the focus ring at the other.
const TEXT: f32 = 4.5;
const MARK: f32 = 3.;

/// What the info colour starts from (a blue), on dark and on light themes, before it is fitted.
const INFO_ON_DARK: u32 = 0x7fb0ff;
const INFO_ON_LIGHT: u32 = 0x395b84;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Palette {
    /// The page, and normal list rows.
    pub background: u32,
    /// The window around the page: the sidebar and the bottom panel (see [`chrome_plate`]).
    ///
    /// [`chrome_plate`]: Palette::chrome_plate
    pub chrome: u32,
    /// Plates and captions (see [`card`](Palette::card) for the see-through one).
    pub card_color: u32,
    /// Menus, dialogs and tooltips.
    pub raised: u32,
    /// Text inputs, search and code wells.
    pub field: u32,
    /// A hovered row or secondary action.
    pub hover: u32,
    /// A selected row or segment, and the text selection. Always with `text`.
    pub selected: u32,
    pub text: u32,
    pub muted: u32,
    /// The edge of an input or a secondary control.
    pub line: u32,
    /// Passive rules between rows.
    pub divider: u32,
    /// A selected or hovered control on the chrome.
    pub chrome_raised: u32,
    pub chrome_line: u32,
    pub chrome_text: u32,
    pub chrome_muted: u32,
    /// The accent as picked: never changed. Marks, active indicators, progress.
    pub accent: u32,
    /// Text and glyphs on the accent: white, always.
    pub accent_on: u32,
    /// Play's solid fill: the accent, darkened along its hue only as far as white needs (4.5:1).
    pub play_fill: u32,
    /// The accent as text, a link or a tick on the surfaces above (4.5:1).
    pub accent_ink: u32,
    /// The keyboard ring, on every surface (3:1).
    pub focus: u32,
    pub danger: u32,
    pub info: u32,
    pub success: u32,
    pub warning: u32,
    /// How opaque the plates are: see the module's note.
    pub alpha: f32,
    /// How opaque the dialogs' scrim is.
    pub scrim_alpha: f32,
}

/// The colours of a picture's stand-in ([`Palette::tile`]): its fill, and the letter on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tile {
    pub fill: u32,
    pub ink: u32,
}

/// How far a tile's fill stands from the card (a contrast ratio): enough to be a shape of its
/// own, not enough to shout.
const TILE_APART: f32 = 1.4;
/// How much of its hue a tile's fill carries over the card, and its letter over the text.
const TILE_HUE: f32 = 0.24;
const TILE_INK_HUE: f32 = 0.3;

/// The hue `id` always gets, as a turn (0 to 1): FNV-1a over its bytes, then mixed (the last
/// step of MurmurHash3) so that ids a letter apart land far apart.
fn tile_hue(id: &str) -> f32 {
    let mut hash = id.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    hash = (hash ^ hash >> 16).wrapping_mul(0x85eb_ca6b);
    hash = (hash ^ hash >> 13).wrapping_mul(0xc2b2_ae35);
    (hash >> 16) as f32 / 65536.
}

/// The full colour at `turn` of the hue wheel.
fn hue_colour(turn: f32) -> u32 {
    let channel = |shift: f32| {
        let wave = ((turn - shift) * std::f32::consts::TAU).cos();
        ((0.5 + 0.5 * wave) * 255.).round() as u32
    };
    channel(0.) << 16 | channel(1. / 3.) << 8 | channel(2. / 3.)
}

impl Inputs {
    fn of(settings: &Settings, theme: &Theme) -> Inputs {
        Inputs {
            night: theme.night,
            accent: theme.accent_choice.rgb(),
            tint: settings.glass_tint(),
            scenery: settings.scenery() && !theme.photo && !settings.performance(),
        }
    }
}

thread_local! {
    static CURRENT: Cell<(Inputs, Palette)> = Cell::new({
        let settings = Settings::default();
        let theme = crate::theme::from_settings(&settings, gpui::WindowAppearance::Dark);
        let inputs = Inputs::of(&settings, &theme);
        (inputs, inputs.palette())
    });
}

/// The palette of the frame being drawn.
pub(crate) fn palette() -> Palette {
    CURRENT.get().1
}

/// Starts a frame: the palette its screens draw with, computed again only for new inputs.
pub(crate) fn begin(settings: &Settings, theme: &Theme) {
    let inputs = Inputs::of(settings, theme);
    if CURRENT.get().0 != inputs {
        CURRENT.set((inputs, inputs.palette()));
    }
}

/// A test for [`fit`]: `c` reads at `min` on each of `on`.
fn reads(on: &[u32], min: f32) -> impl Fn(u32) -> bool + '_ {
    move |c| on.iter().all(|&s| contrast(c, s) >= min)
}

/// `c`, mixed toward white (`lighter`) or black, which keeps its hue, by the least that makes
/// `reads` hold: found by bisection; `c` itself when it already reads.
fn fit(c: u32, lighter: bool, reads: impl Fn(u32) -> bool) -> u32 {
    let end = if lighter { 0xffffff } else { 0x000000 };
    if reads(c) || !reads(end) {
        return if reads(c) { c } else { end };
    }
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..10 {
        let mid = (lo + hi) / 2.;
        if reads(over(end, mid, c)) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    over(end, hi, c)
}

/// Everything a palette depends on: it is computed again only when one of these changes.
#[derive(Clone, Copy, PartialEq)]
struct Inputs {
    night: Night,
    accent: u32,
    tint: f32,
    scenery: bool,
}

impl Inputs {
    /// The palette of the theme's night (its own colours, or Dawn's), at the player's accent
    /// and Glass, and with or without the scenery behind the plates.
    fn palette(&self) -> Palette {
        let Inputs {
            night: n,
            accent: raw,
            tint,
            scenery,
        } = *self;
        // Light text on dark surfaces fits lighter; dark text on light ones fits darker.
        let lighter = n.dark;
        // The theme's own plates with the player's Glass: white (or the light theme's ink)
        // at `tint`.
        let veil = |plate: u32| {
            over(
                n.ink,
                (tint - sunrise_launcher_theme::glass::TINT).max(0.),
                plate,
            )
        };
        let background = n.page;
        let card_color = veil(n.card);
        let raised = veil(n.card_hi);
        let field = over(n.ink, fill::FIELD, card_color);
        let hover = over(n.ink, fill::ACTIVE, card_color);
        // The selection is the accent washed into the plate, as strong as the text on it allows.
        let selected = (0..14)
            .map(|i| over(raw, 0.30 - 0.02 * i as f32, card_color))
            .find(|&s| contrast(n.ink, s) >= TEXT)
            .unwrap_or(hover);
        let surfaces = [background, card_color, raised, field, hover, selected];
        let text = fit(n.ink, lighter, reads(&surfaces, TEXT));
        let muted = fit(n.muted, lighter, reads(&surfaces, TEXT));
        let line = fit(
            over(n.ink, 0.4, card_color),
            lighter,
            reads(&surfaces[..4], MARK),
        );
        let divider = over(n.stroke, n.rule_alpha, card_color);

        let chrome = veil(n.side);
        let chrome_raised = over(n.ink, fill::ACTIVE, chrome);
        let on_chrome = [chrome, chrome_raised];
        let chrome_text = fit(n.ink, lighter, reads(&on_chrome, TEXT));
        let chrome_muted = fit(n.muted, lighter, reads(&on_chrome, TEXT));
        let chrome_line = fit(over(n.ink, 0.4, chrome), lighter, reads(&on_chrome, MARK));

        // The accent, and what must read in front of or on it.
        let play_fill = fit(raw, false, |c| contrast(0xffffff, c) >= TEXT);
        let accent_ink = fit(raw, lighter, reads(&surfaces[..5], TEXT));
        let everywhere = [surfaces.as_slice(), on_chrome.as_slice()].concat();
        let focus = fit(raw, lighter, reads(&everywhere, MARK));
        let status = |c: u32| fit(c, lighter, reads(&surfaces[..5], TEXT));

        // Opaque without scenery; with it, as see-through as text over the sky's worst band
        // allows.
        let alpha = if scenery {
            let base = tint + (1. - tint) * PLATE_BASE;
            let holds = |a: f32| {
                n.sky.iter().all(|&sky| {
                    [
                        (text, card_color),
                        (muted, card_color),
                        (chrome_text, chrome),
                        (chrome_muted, chrome),
                    ]
                    .iter()
                    .all(|&(ink, plate)| contrast(ink, over(plate, a, sky)) >= TEXT)
                })
            };
            (0..=100)
                .map(|i| i as f32 / 100.)
                .find(|&a| a >= base && holds(a))
                .unwrap_or(1.)
        } else {
            1.
        };

        let scrim_alpha = SCRIM_ALPHAS
            .map(|i| i as f32 / 100.)
            .max_by(|&a, &b| {
                let apart = |al: f32| {
                    let seen = |page| contrast(raised, over(SCRIM, al, page));
                    seen(BRIGHT[0]).min(seen(BRIGHT[1]))
                };
                apart(a).total_cmp(&apart(b))
            })
            .unwrap_or(0.55);

        Palette {
            background,
            chrome,
            card_color,
            raised,
            field,
            hover,
            selected,
            text,
            muted,
            line,
            divider,
            chrome_raised,
            chrome_line,
            chrome_text,
            chrome_muted,
            accent: raw,
            accent_on: 0xffffff,
            play_fill,
            accent_ink,
            focus,
            danger: status(n.danger),
            info: status(if n.dark { INFO_ON_DARK } else { INFO_ON_LIGHT }),
            success: status(n.success),
            warning: status(n.warn),
            alpha,
            scrim_alpha,
        }
    }
}

impl Palette {
    /// The palette of `night` under `accent` with opaque plates: a theme's preview, whatever
    /// theme is in use.
    pub(crate) fn plain(night: Night, accent: u32, tint: f32) -> Palette {
        Inputs {
            night,
            accent,
            tint,
            scenery: false,
        }
        .palette()
    }

    /// The palette `begin` would set for these settings and this theme.
    #[cfg(test)]
    pub(crate) fn of(settings: &Settings, theme: &Theme) -> Palette {
        Inputs::of(settings, theme).palette()
    }

    /// What stands in for the picture of a mod or pack that has none: a quiet colour of its own
    /// from `id`, the same every time, under its first letter. The fill is the card washed with
    /// the id's hue and set [`TILE_APART`] from the card, lighter on a dark theme and darker
    /// on a light one, so every tile of a theme weighs the same; the letter is the text washed
    /// with the same hue, and reads on it (4.5:1).
    pub(crate) fn tile(&self, id: &str) -> Tile {
        use sunrise_launcher_theme::glass::luminance;
        let hue = hue_colour(tile_hue(id));
        let card = self.card_color;
        let lighter = luminance(self.text) > luminance(card);
        // The one luminance every tile of this theme has. A card too bright (or too dark) to
        // step away from in its own direction is stepped away from in the other.
        let (up, down) = (
            (luminance(card) + 0.05) * TILE_APART - 0.05,
            (luminance(card) + 0.05) / TILE_APART - 0.05,
        );
        let level = match lighter {
            true if up <= 1. => up,
            false if down < 0. => up,
            _ => down,
        };
        let washed = over(hue, TILE_HUE, card);
        let brighten = luminance(washed) < level;
        let fill = fit(washed, brighten, |c| (luminance(c) >= level) == brighten);
        let reads = |c: u32| contrast(c, fill) >= TEXT;
        let letter = over(hue, TILE_INK_HUE, self.text);
        let ink = [lighter, !lighter]
            .into_iter()
            .map(|l| fit(letter, l, reads))
            .find(|&c| reads(c))
            .unwrap_or(self.text);
        Tile { fill, ink }
    }

    /// A card, caption or plate: the theme's tint at the plates' opacity.
    pub(crate) fn card(&self) -> Hsla {
        Hsla::from(rgb(self.card_color)).opacity(self.alpha)
    }

    /// The material the sidebar and the bottom panel share.
    pub(crate) fn chrome_plate(&self) -> Hsla {
        Hsla::from(rgb(self.chrome)).opacity(self.alpha)
    }

    /// A text input's fill, and the account chip's: the field colour, part see-through, so
    /// the glass behind it shows.
    pub(crate) fn input_fill(&self) -> Hsla {
        Hsla::from(rgb(self.field)).opacity(0.55)
    }

    /// A resting input's outline: faint. The focus ring, not this line, marks the one in use.
    pub(crate) fn input_edge(&self) -> Hsla {
        Hsla::from(rgb(self.text)).opacity(0.14)
    }

    /// What a dialog dims the launcher with.
    pub(crate) fn scrim(&self) -> Hsla {
        Hsla::from(rgb(SCRIM)).opacity(self.scrim_alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::from_settings;
    use gpui::WindowAppearance;
    use sunrise_launcher::backend::Appearance;
    use sunrise_launcher::themes::{self, Accent};
    use sunrise_launcher_theme::{glass, glass::luminance};

    fn look(id: Option<&str>, accent: Accent, glass: f32, scenery: bool) -> (Settings, Theme) {
        let settings = Settings {
            accent,
            theme: id.map(Into::into),
            glass: Some(glass),
            scenery_on: Some(scenery),
            ..Settings::default()
        };
        let theme = from_settings(&settings, WindowAppearance::Dark);
        (settings, theme)
    }

    /// The classes of look: each theme at its own accent; one theme in every named accent and
    /// in a pale, a dark, a grey and a saturated typed one; the least and most Glass; scenery
    /// off; and the light appearance. `all` is every theme with every accent and setting.
    fn looks(all: bool) -> Vec<(String, Settings, Theme)> {
        let ids = std::iter::once(None).chain(themes::THEMES.iter().map(|t| Some(t.id)));
        let typed = [0xffe39a, 0x101040, 0x808080, 0xff0000].map(Accent::from_rgb);
        let mut looks = Vec::new();
        for id in ids {
            let own = themes::starting_accent(id, &Default::default());
            let accents = if all || id.is_none() {
                [vec![own], Accent::ALL.to_vec(), typed.to_vec()].concat()
            } else {
                vec![own]
            };
            for accent in accents {
                for (glass, scenery) in [(6., true), (0., true), (16., true), (6., false)] {
                    if all || id.is_none() || (glass == 6. && scenery) {
                        let (settings, theme) = look(id, accent, glass, scenery);
                        looks.push((
                            format!("{id:?} {} {glass} {scenery}", accent.name()),
                            settings,
                            theme,
                        ));
                    }
                }
            }
        }
        for accent in Accent::ALL {
            let theme = Theme::for_appearance(Appearance::Light, accent, WindowAppearance::Light);
            looks.push((
                format!("light {}", accent.name()),
                Settings {
                    accent,
                    ..Settings::default()
                },
                theme,
            ));
        }
        looks
    }

    /// Ids as the registry and the packs' folders spell them.
    const IDS: [&str; 12] = [
        "jei",
        "coords_hud",
        "lanterns",
        "sodium",
        "iris",
        "waystones",
        "minimap",
        "splitscreen",
        "replay",
        "Faithful 32x.zip",
        "complementary-reimagined",
        "",
    ];

    /// The channel-wise hue, in radians.
    fn hue(c: u32) -> f32 {
        let [r, g, b] = [16, 8, 0].map(|s| ((c >> s) & 255) as f32);
        (3f32.sqrt() * (g - b)).atan2(2. * r - g - b)
    }

    fn spread(c: u32) -> u32 {
        let ch = [16, 8, 0].map(|s| (c >> s) & 255);
        ch.iter().max().unwrap() - ch.iter().min().unwrap()
    }

    /// Everything the palette promises, for one look.
    fn check(name: &str, settings: &Settings, theme: &Theme) {
        let p = Palette::of(settings, theme);
        let reads = |role: &str, fg: u32, on: &[u32], min: f32| {
            for &s in on {
                let r = contrast(fg, s);
                assert!(r >= min, "{name}: {role} on {s:06x} {r:.2}");
            }
        };
        // The accent is never changed; Play is white on a darkened fit of it, keeping its hue.
        assert_eq!(p.accent, theme.accent_choice.rgb(), "{name}: raw accent");
        assert_eq!(p.accent_on, 0xffffff, "{name}: Play's text is white");
        reads("white on Play", 0xffffff, &[p.play_fill], TEXT);
        if p.play_fill != p.accent {
            assert!(
                luminance(p.play_fill) < luminance(p.accent),
                "{name}: Play is darker"
            );
            if spread(p.accent) > 60 && spread(p.play_fill) > 40 {
                let drift = (hue(p.play_fill) - hue(p.accent)).abs();
                assert!(
                    drift.min(std::f32::consts::TAU - drift) < 0.1,
                    "{name}: hue {drift}"
                );
            }
        }
        let surfaces = [
            p.background,
            p.card_color,
            p.raised,
            p.field,
            p.hover,
            p.selected,
        ];
        reads("text", p.text, &surfaces, TEXT);
        reads("muted", p.muted, &surfaces, TEXT);
        for (role, fg) in [
            ("accent ink", p.accent_ink),
            ("danger", p.danger),
            ("info", p.info),
            ("success", p.success),
            ("warning", p.warning),
        ] {
            reads(role, fg, &surfaces[..5], TEXT);
        }
        reads(
            "focus ring",
            p.focus,
            &[&surfaces[..], &[p.chrome, p.chrome_raised]].concat(),
            MARK,
        );
        reads("edge", p.line, &surfaces[..4], MARK);
        let chrome = [p.chrome, p.chrome_raised];
        reads("chrome text", p.chrome_text, &chrome, TEXT);
        reads("chrome muted", p.chrome_muted, &chrome, TEXT);
        reads("chrome edge", p.chrome_line, &chrome, MARK);

        // Plates: opaque without scenery, else at least the Glass setting, and text on them
        // reads over each band of the theme's sky.
        assert_eq!(
            (p.card().a, p.chrome_plate().a),
            (p.alpha, p.alpha),
            "{name}"
        );
        if !settings.scenery() {
            assert_eq!(p.alpha, 1., "{name}: opaque without scenery");
        }
        let tint = settings.glass_tint();
        assert!(
            p.alpha + f32::EPSILON >= (tint + (1. - tint) * PLATE_BASE).min(1.),
            "{name}"
        );
        for sky in theme.night.sky {
            for (fg, plate) in [
                (p.text, p.card_color),
                (p.muted, p.card_color),
                (p.chrome_text, p.chrome),
                (p.chrome_muted, p.chrome),
            ] {
                reads(
                    "text on a plate over the sky",
                    fg,
                    &[over(plate, p.alpha, sky)],
                    TEXT,
                );
            }
        }
        // A tile without a picture: its letter reads, and every tile stands the same small way
        // off the card (lighter on a dark theme, darker on a light one), in a quiet colour.
        let lighter = luminance(p.text) > luminance(p.card_color);
        for id in IDS {
            let t = p.tile(id);
            reads(
                &format!("the letter of {id}'s tile"),
                t.ink,
                &[t.fill],
                TEXT,
            );
            let apart = contrast(t.fill, p.card_color);
            assert!(
                (TILE_APART - 0.08..=TILE_APART + 0.08).contains(&apart),
                "{name}: {id}'s tile {:06x} is {apart:.2} from the card",
                t.fill
            );
            // Only a card with no room on its own side (a white one under pale text, say)
            // sends its tiles the other way.
            let room = match lighter {
                true => (luminance(p.card_color) + 0.05) * TILE_APART <= 1.05,
                false => (luminance(p.card_color) + 0.05) / TILE_APART >= 0.05,
            };
            assert_eq!(
                luminance(t.fill) > luminance(p.card_color),
                lighter == room,
                "{name}: {id}'s tile {:06x} is on the wrong side of the card",
                t.fill
            );
            assert!(
                spread(t.fill) <= spread(p.card_color) + 64,
                "{name}: {id}'s tile {:06x} is loud",
                t.fill
            );
        }
        // Fields, selections, tooltips and menus are opaque: a bright page behind changes nothing.
        reads("field text", p.text, &[p.field], TEXT);
        reads("placeholder", p.muted, &[p.field], TEXT);
        reads("tooltip", p.text, &[p.raised], TEXT);
        if spread(p.accent) > 30 {
            assert_ne!(p.selected, p.field, "{name}: a selection shows in a field");
        }
        assert_eq!(p.scrim().a, p.scrim_alpha);
        for page in BRIGHT {
            let edge = contrast(p.raised, over(SCRIM, p.scrim_alpha, page));
            assert!(
                edge >= 1.5,
                "{name}: dialog over the scrim over {page:06x} {edge:.2}"
            );
        }
    }

    #[test]
    fn a_sample_of_every_class_of_look_holds_the_palette_contract() {
        for (name, settings, theme) in looks(false) {
            check(&name, &settings, &theme);
        }
    }

    #[test]
    #[ignore = "exhaustive: every theme with every accent and setting"]
    fn every_look_holds_the_palette_contract() {
        for (name, settings, theme) in looks(true) {
            check(&name, &settings, &theme);
        }
    }

    /// A mod without a picture keeps one colour of its own: the same whenever it is asked for,
    /// another mod's is another colour, and the ids' hues go all the way round the wheel.
    #[test]
    fn a_tile_is_one_colour_per_id() {
        let ids = [Some("twilight"), None]
            .into_iter()
            .chain(themes::THEMES.iter().map(|t| Some(t.id)));
        for id in ids {
            let own = themes::starting_accent(id, &Default::default());
            let (settings, theme) = look(id, own, 6., true);
            let p = Palette::of(&settings, &theme);
            let tiles = IDS.map(|mod_id| p.tile(mod_id));
            assert_eq!(tiles, IDS.map(|mod_id| p.tile(mod_id)), "{id:?}: stable");
            for (i, a) in tiles.iter().enumerate() {
                for b in &tiles[i + 1..] {
                    assert_ne!(a.fill, b.fill, "{id:?}: {tiles:06x?}");
                }
            }
            // Coloured, not the grey every tile used to be.
            assert!(
                tiles.iter().all(|t| spread(t.fill) >= 12),
                "{id:?}: {tiles:06x?}"
            );
        }
        assert_eq!(tile_hue("jei"), tile_hue("jei"));
        // Ids a letter apart land far apart, and a thousand ids leave no twelfth of the wheel
        // empty.
        let apart = (tile_hue("mod-a") - tile_hue("mod-b")).abs();
        assert!(apart.min(1. - apart) > 0.02, "{apart}");
        let mut sectors = [0; 12];
        for n in 0..1000 {
            sectors[(tile_hue(&format!("mod-{n}")) * 12.) as usize] += 1;
        }
        assert!(sectors.iter().all(|&n| n >= 40), "{sectors:?}");
    }

    /// Dawn's Play is the site's brand orange as it is, under white; a lighter orange would
    /// not hold the label.
    #[test]
    fn dawns_play_is_the_brand_orange_under_white() {
        for json in [
            "{}",
            r#"{"theme":"dawn"}"#,
            r##"{"accent":"#c2366f"}"##,
            r##"{"theme":"dawn","accent":"#c2366f"}"##,
        ] {
            let settings = Settings::parse(json);
            let theme = from_settings(&settings, WindowAppearance::Dark);
            let p = Palette::of(&settings, &theme);
            assert_eq!(
                (p.accent, p.play_fill, p.accent_on),
                (0xb4552f, 0xb4552f, 0xffffff)
            );
            assert!((contrast(0xffffff, p.play_fill) - 4.905).abs() < 0.01);
        }
        assert!(
            contrast(0xffffff, 0xc4623a) < TEXT,
            "the site's hover orange is too light"
        );
    }

    #[test]
    fn every_builtin_play_uses_its_accent_at_readable_white_contrast() {
        let ids = [themes::TWILIGHT]
            .into_iter()
            .chain(themes::THEMES.iter().map(|t| t.id));
        for id in ids {
            let own = themes::own_accent(Some(id)).rgb();
            let settings = Settings::parse(&format!(r#"{{"theme":"{id}"}}"#));
            let theme = from_settings(&settings, WindowAppearance::Dark);
            let p = Palette::of(&settings, &theme);
            assert_eq!(p.accent, own, "{id}: the theme's accent is unchanged");
            assert_eq!(p.accent_on, 0xffffff, "{id}: white label and icon");
            let actual = contrast(p.accent_on, p.play_fill);
            assert!(actual >= TEXT, "{id}: white contrast is {actual:.3}:1");
            if contrast(0xffffff, own) >= TEXT {
                assert_eq!(p.play_fill, own, "{id}: no fit needed");
            } else {
                // Only just dark enough to read, within the rounding of an RGB step.
                assert!(actual < TEXT + 0.06, "{id}: darkened too far");
                let shade = luminance(p.play_fill) / luminance(own);
                assert!(shade < 1.0, "{id}: fit must darken the fill");
                let drift = (hue(p.play_fill) - hue(own)).abs();
                assert!(drift.min(std::f32::consts::TAU - drift) < 0.02, "{id}");
            }
        }
    }

    /// A player's theme holds the same contract as a built-in one, whatever six colours it
    /// was let in with: dark, light, a pale accent, surfaces that differ.
    #[test]
    fn a_players_theme_holds_the_palette_contract() {
        use sunrise_launcher::theme_files::parse;
        let files = [
            (
                "tidepool",
                r##"card = "#1A3135"
chrome = "#11262B"
text = "#F0F8F4"
muted = "#C3D7D2"
background = "#102328"
accent = "#8EDBCB""##,
            ),
            (
                "paper",
                r##"card = "#ffffff"
chrome = "#e8e4dc"
text = "#1b1b1b"
muted = "#555555"
background = "#f4f1ea"
accent = "#ffd84a""##,
            ),
            (
                "faint inks",
                r##"card = "#202020"
text = "#3a3a3a"
muted = "#303030""##,
            ),
            (
                "apart",
                r##"card = "#3a2a10"
chrome = "#000000"
background = "#101820"
accent = "#101010""##,
            ),
            ("accent only", r##"accent = "#ff66cc""##),
        ];
        for (name, palette) in files {
            let text = format!("version = 1\nname = \"{name}\"\n[palette]\n{palette}\n");
            let roles = parse(&text)
                .unwrap_or_else(|e| panic!("{name}: {e:?}"))
                .roles;
            let accent = Accent::from_rgb(roles.accent);
            for scenery in [true, false] {
                let settings = Settings {
                    accent,
                    scenery_on: Some(scenery),
                    ..Settings::default()
                };
                let theme = Theme::for_appearance(Appearance::Dark, accent, WindowAppearance::Dark)
                    .with_night(roles.night());
                check(
                    &format!("local {name}, scenery {scenery}"),
                    &settings,
                    &theme,
                );
                let p = Palette::of(&settings, &theme);
                assert_eq!(p.accent, roles.accent, "{name}");
                assert_eq!(p.accent_on, 0xffffff, "{name}");
                assert_eq!(p.background, roles.background, "{name}");
            }
        }
    }

    /// A picture behind Home, the player's or their theme's, may be any colour: the plates
    /// over it are opaque.
    #[test]
    fn plates_are_opaque_over_a_picture() {
        let glassy = Settings::default();
        let theme = from_settings(&glassy, WindowAppearance::Dark);
        assert!(Palette::of(&glassy, &theme).alpha < 1.);
        let pictured = Settings {
            background: Some("background-0123456789ab.png".into()),
            ..Settings::default()
        };
        let theme = from_settings(&pictured, WindowAppearance::Dark);
        assert!(theme.photo);
        let p = Palette::of(&pictured, &theme);
        assert_eq!(p.alpha, 1.);
        // Home's header stands on a card over a picture: nothing of the picture comes through
        // it, so what the header says reads as it does on any card.
        assert_eq!(p.card().a, 1.);
        for ink in [p.text, p.muted, p.warning, p.accent_ink] {
            assert!(contrast(ink, p.card_color) >= TEXT, "{ink:06x}");
        }
        // With scenery off there is no picture to guard against, and no glass either.
        let off = Settings {
            scenery_on: Some(false),
            ..pictured
        };
        let theme = from_settings(&off, WindowAppearance::Dark);
        assert!(!theme.photo);
        assert_eq!(Palette::of(&off, &theme).alpha, 1.);
    }

    #[test]
    fn white_text_on_a_pale_accent_darkens_the_fill_only() {
        let (settings, theme) = look(None, Accent::from_rgb(0xffe39a), 6., true);
        let p = Palette::of(&settings, &theme);
        assert_eq!(p.accent, 0xffe39a);
        assert_ne!(p.play_fill, p.accent);
        let (settings, theme) = look(None, Accent::from_rgb(glass::ACCENT), 6., true);
        let p = Palette::of(&settings, &theme);
        assert_eq!(
            (p.play_fill, p.accent_on),
            (p.accent, 0xffffff),
            "a dark enough accent stays"
        );
    }

    /// The theme and the player's accent decide the palette; a frame reads what it began, and
    /// the same inputs are not computed again.
    #[test]
    fn begin_sets_the_frames_palette_from_the_inputs() {
        let (violet, a) = look(None, Accent::default(), 6., true);
        let nord = Settings {
            theme: Some("nord".into()),
            accent: Accent::from_rgb(0x88c0d0),
            ..Settings::default()
        };
        let b = from_settings(&nord, WindowAppearance::Dark);
        let (pa, pb) = (Palette::of(&violet, &a), Palette::of(&nord, &b));
        assert_eq!(
            (pb.background, pb.accent),
            (0x2e3440, 0x88c0d0),
            "Nord's night and frost"
        );
        assert_ne!(pb.play_fill, pb.accent, "pale frost cannot hold white text");
        begin(&nord, &b);
        assert_eq!(palette(), pb);
        let kept = CURRENT.get().1;
        begin(&nord, &b);
        assert_eq!(CURRENT.get().1, kept);
        begin(&violet, &a);
        assert_eq!(palette(), pa);
        assert_eq!(pa.accent, glass::ACCENT);
    }
}
