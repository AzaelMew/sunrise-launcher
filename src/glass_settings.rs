//! The Settings page in the glass look: the mockup's `SettingsView`. A list of the sections on
//! the left, and the sections as glass cards in a pane that scrolls on its own under the title;
//! the list follows the scroll and takes the pane to a section. The rows inside are the mockup's
//! `SettingRow`s ([`glass_ui::flat_rows`]).
//!
//! Not in the mockup, kept in its patterns: the appearance, the Home background, the data and
//! logs folders and the applications menu (About's rows), the AI clients and the hidden developer options. The mockup's About tiles for Sunrise and
//! the launcher open Updates here, beside the switches that decide how updates install. The mockup's Native mods switch is the
//! native mods' build profile here: they are always on.





/// Settings' pane draws only the sections near its view in full: laying out every card of every
/// section was most of an open Settings frame's work (~900 layout nodes, ~1,500 text
/// measurements). The others are the space they took when last drawn, so the scrollbar, the
/// sidebar's jumps and its marked section stay where they were. A section not yet measured at
/// this width is drawn out of view once, one a frame, until all are; meanwhile it takes
/// [`Culled::ESTIMATE`].
#[derive(Clone, Copy, Default)]
pub(super) struct Culled {
    /// The pane's width the sections were measured at.
    width: f32,
    /// One bit per section: measured at that width, drawn in full on the last frame, all.
    measured: u32,
    drawn: u32,
    all: u32,
}



/// The scroll spy: which section is being read, given each section's top from the pane's top
/// (`tops`, in order), the pane's `height`, how far it is `scrolled` and how far it can be (`max`).
/// The last section whose top passed a reading line, which sits at 30 % of the pane (the mockup's)
/// and slides down to the pane's bottom over the last 70 % of a pane of scroll: the short last
/// sections, which never reach 30 %, each come in turn as the end nears, instead of the mockup's
/// jump from wherever it was to the last one at the very end.
#[cfg_attr(not(windows), allow(dead_code))] // the Windows setup window
fn spy(tops: &[f32], height: f32, scrolled: f32, max: f32) -> usize {
    let rest = height * 0.3;
    let travel = height - rest;
    let tail = travel.min(max);
    let line = if tail > 0. {
        rest + travel * ((scrolled - (max - tail)) / tail).clamp(0., 1.)
    } else {
        rest
    };
    tops.iter().rposition(|&top| top <= line + 0.5).unwrap_or(0)
}




#[cfg(test)]
mod tests {
    use super::spy;

    /// A 600 px pane over the eight settings cards: tall ones, then About, too short for its
    /// top ever to reach 30 % of the pane.
    const TOPS: [f32; 8] = [0., 420., 700., 1000., 1300., 1600., 1900., 2200.];
    const HEIGHT: f32 = 600.;
    const MAX: f32 = 2400. - HEIGHT;

    fn at(scrolled: f32) -> usize {
        let tops: Vec<f32> = TOPS.iter().map(|t| t - scrolled).collect();
        spy(&tops, HEIGHT, scrolled, MAX)
    }

    #[test]
    fn the_list_follows_the_section_read_at_30_percent() {
        assert_eq!(at(0.), 0);
        assert_eq!(
            at(300.),
            1,
            "Updates' top is 120 px down: in the upper 30 %"
        );
        assert_eq!(at(600.), 2);
        assert_eq!(
            at(1000.),
            3,
            "Network's top at the pane's top, Game's 300 px down"
        );
        assert_eq!(at(1150.), 4, "Game's top 150 px down: in the upper 30 %");
    }

    #[test]
    fn near_the_end_the_short_last_section_is_marked_for_a_stretch() {
        let seen: Vec<usize> = (0..=MAX as usize)
            .step_by(10)
            .map(|s| at(s as f32))
            .collect();
        // Every section is marked on the way down, in order, never skipped.
        let mut order = seen.clone();
        order.dedup();
        assert_eq!(order, (0..8).collect::<Vec<_>>());
        // About is marked for a stretch of scroll before the end, not for one step.
        let about = seen.iter().filter(|&&s| s == 7).count();
        assert!(about >= 8, "About marked for {about} steps of 10 px");
        assert_eq!(at(MAX), 7);
    }

    #[test]
    fn a_pane_that_does_not_scroll_marks_the_first() {
        assert_eq!(spy(&[0., 200.], 600., 0., 0.), 0);
        assert_eq!(spy(&[], 600., 0., 0.), 0);
    }
}


