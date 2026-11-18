//! The picture of a mod or a pack, wherever one is listed: Discover, Installed, a project's
//! page, the packs' shelves and the search.
use crate::paint::palette;
use crate::tokens;
use gpui::{div, prelude::*, px, rgb, Div, FontWeight, ImageSource, ObjectFit};

/// The picture of a mod or pack in a square `side` wide: it fills the square edge to edge,
/// cropped rather than boxed, with a button's corners and no outline. Without a picture (it has
/// none, or it has not arrived), the first letter of `name` on the colour `id` always gets
/// ([`Palette::tile`](crate::paint::Palette::tile)).
pub(crate) fn tile(
    picture: Option<impl Into<ImageSource>>,
    id: &str,
    name: &str,
    side: f32,
) -> Div {
    // A square smaller than a button is tall takes the small tiles' corners.
    let corners = px(if side < 32. {
        tokens::R_SMALL
    } else {
        tokens::R_CONTROL
    });
    let square = div().flex_none().size(px(side)).rounded(corners);
    match picture {
        Some(picture) => square.child(
            gpui::img(picture)
                .size_full()
                .rounded(corners)
                .object_fit(ObjectFit::Cover),
        ),
        None => {
            let colours = palette().tile(id);
            square
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(colours.fill))
                .text_size(px(side * 0.45))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(colours.ink))
                .child(crate::join::initial(name))
        }
    }
}
