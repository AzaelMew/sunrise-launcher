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

impl Inputs {
    fn of(settings: &Settings, theme: &Theme) -> Inputs {
        Inputs {
            night: theme.night,
            accent: theme.accent_choice.rgb(),
            tint: settings.glass_tint(),
            scenery: settings.scenery() && settings.background.is_none() && !settings.performance(),
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
    /// The palette `begin` would set for these settings and this theme.
    #[cfg(test)]
    pub(crate) fn of(settings: &Settings, theme: &Theme) -> Palette {
        Inputs::of(settings, theme).palette()
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
