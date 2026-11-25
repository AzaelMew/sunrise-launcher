//! The website's cat, hiding behind Play: now and then it peeks out, or walks out into the empty
//! part of the play bar and stays a while, or climbs up and sleeps on Play. In the bar a click
//! pets it, awake or dozing. On Play it can't be petted: it is a picture with no handler and no
//! hitbox of its own, so a click there goes to whatever is under it, as if no cat were there.
//!
//! Its look and its ways are the Sunrise site's, ported from sunrise-org/sunny
//! (`frontend/site/art/`: `cat.ts` for the poses and coats, `cat-actions.ts` for what it does and
//! how it takes being petted, `core.ts` for the pixel helpers). The site's cat breathes all the
//! time; this one holds still between actions, so a resting cat costs no frames.
mod actions;
mod art;
mod behaviour;
mod drawing;
mod outing;
#[cfg(test)]
mod tests;

use super::*;
use gpui::RenderImage;
use std::sync::Arc;
use std::time::{Duration, Instant};

use actions::*;
use art::*;
use behaviour::*;
use drawing::*;
pub(crate) use outing::*;

impl Launcher {
    /// The empty stretch of the play bar between its text and Play, and the cat when it is out.
    /// The cat is drawn inside this stretch and cut at its edges, under Play, so it comes out
    /// from behind the button and can never lie over text or a control. `beside` says Play is
    /// what follows; with other buttons in between the cat stays in.
    ///
    /// The second element is the cat asleep on Play, for Play's own box: it is drawn only above
    /// the button's top edge and takes no clicks. `free` says Play is idle and the bar has
    /// nothing to say; the cat climbs up only then, and drops off the moment that ends.
    pub(super) fn cat(
        &mut self,
        beside: bool,
        free: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Div, Option<Div>) {
        let measured = self.ui.cat.lane.clone();
        let lane = div()
            .flex_1()
            .min_w_0()
            .h(px(H as f32 * SIZE))
            .relative()
            .overflow_hidden()
            // Over the bar's gap and a little under Play.
            .when(beside, |d| d.mr(px(-(14. + UNDER))))
            .child(
                gpui::canvas(
                    move |bounds, _, _| measured.set(f32::from(bounds.size.width)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            );
        // Switched off, or the player asked for less motion: the cat stays away. Nothing of it
        // is kept and no wake is scheduled, so an idle window draws nothing for it.
        if !cat_out(self.settings.show_cat(), crate::motion::still()) {
            if let Some(image) = self.ui.cat.switch_off() {
                let _ = window.drop_image(image);
            }
            return (lane, None);
        }
        let roomy = beside && self.window_width >= ROOM.0 && self.window_height >= ROOM.1;
        let room = if roomy {
            self.ui.cat.lane.get() - UNDER - 6.
        } else {
            0.
        };
        let free = free && roomy;
        // A window in the background keeps its cat still: nothing is scheduled until it is back.
        let awake = window.is_window_active();
        if awake {
            if let Some(seen) = self.ui.cat.seen {
                self.ui.cat.clock += seen.elapsed().as_secs_f32() * 1000.;
            }
        }
        self.ui.cat.seen = awake.then(Instant::now);
        let now = self.ui.cat.clock;
        let outing = self.ui.cat.cat.get_or_insert_with(|| {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(4, |d| d.as_millis() as u64);
            // One of the site's two cats, a different one from launch to launch.
            let ways = if seed.is_multiple_of(2) {
                &SIAMESE_WAYS
            } else {
                &TABBY_WAYS
            };
            let mut outing = Outing::new(ways, seed, 0.);
            outing.scene = Scene::Away {
                until: outing.cat.random.span([15000., 40000.]),
            };
            outing
        });
        if awake {
            if room <= 0.
                && matches!(
                    outing.scene,
                    Scene::Peek { .. } | Scene::Walk { .. } | Scene::Visit { .. }
                )
            {
                outing.hide(now);
            }
            outing.step(now, room, free);
        }
        let after = outing.next_change(now);
        let coat = outing.cat.ways.coat;
        let view = outing
            .view(now)
            .filter(|view| room > 0. || view.perch.is_some());
        if awake {
            self.cat_wake(after, cx);
        }
        let Some(view) = view else {
            return (lane, None);
        };
        let scale = (SIZE * self.scale).round().max(1.) as usize;
        let image = match &self.ui.cat.shown {
            Some((drawn, at, left, image))
                if *drawn == view.pose && *at == scale && *left == view.left =>
            {
                image.clone()
            }
            _ => {
                let small = picture(coat, &view.pose);
                let mut big = vec![0u8; W * scale * H * scale * 4];
                for (y, row) in big.chunks_exact_mut(W * scale * 4).enumerate() {
                    for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                        // The cat is drawn facing right; facing left is its mirror image.
                        let x = if view.left {
                            W - 1 - x / scale
                        } else {
                            x / scale
                        };
                        let from = (y / scale * W + x) * 4;
                        px.copy_from_slice(&small.0[from..from + 4]);
                    }
                }
                let frame = image::RgbaImage::from_raw((W * scale) as u32, (H * scale) as u32, big)
                    .expect("the buffer is the picture's size");
                let image = Arc::new(RenderImage::new([image::Frame::new(frame)]));
                if let Some((_, _, _, old)) =
                    self.ui
                        .cat
                        .shown
                        .replace((view.pose, scale, view.left, image.clone()))
                {
                    let _ = window.drop_image(old);
                }
                image
            }
        };
        let tall = H as f32 * SIZE;
        if let Some(up) = view.perch {
            // A window above Play's top edge, as tall as the cat: it climbs into it from below,
            // its feet on the edge, and nothing of it is ever drawn over the button. Keep it
            // without an id, a listener or `occlude`: any of those would take a click from the
            // page or the bar under it.
            let feet = (H as f32 - GY - 1.) * SIZE;
            let down = ((1. - up) * (tall - feet) / SIZE).round() * SIZE;
            let perch = div()
                .absolute()
                .top(px(feet - tall))
                .right(px(18.))
                .w(px(WIDTH))
                .h(px(tall - feet))
                .overflow_hidden()
                .child(
                    gpui::img(image)
                        .absolute()
                        .top(px(down))
                        .left(px(0.))
                        .w(px(WIDTH))
                        .h(px(tall)),
                );
            return (lane, Some(perch));
        }
        // Whole cat pixels, so the picture stays crisp as it moves.
        let out = (view.out / SIZE).round() * SIZE;
        let cat = div()
            .id("alt-cat")
            .absolute()
            .top(px(0.))
            .right(px(out + UNDER - WIDTH))
            .child(gpui::img(image).w(px(WIDTH)).h(px(tall)));
        let lane = lane.child(if view.pettable {
            cat.cursor_pointer()
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    let now = this.ui.cat.clock;
                    if let Some(outing) = &mut this.ui.cat.cat {
                        outing.cat.pet(now);
                    }
                    cx.notify();
                }))
        } else {
            cat
        });
        (lane, None)
    }

    /// Play was pressed: a cat asleep on it hops off.
    pub(super) fn cat_off_play(&mut self) {
        let now = self.ui.cat.clock;
        if let Some(outing) = &mut self.ui.cat.cat {
            outing.leave_play(now);
        }
    }

    /// Draws the launcher again `after` this, unless a wake is already due by then.
    fn cat_wake(&mut self, after: Duration, cx: &mut Context<Self>) {
        let at = Instant::now() + after;
        if self
            .ui
            .cat
            .wake
            .is_some_and(|due| due <= at + Duration::from_millis(5))
        {
            return;
        }
        self.ui.cat.wake = Some(at);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(after).await;
            let _ = this.update(cx, |this, cx| {
                if this.ui.cat.wake.take_if(|due| *due == at).is_some()
                    && cat_out(
                        this.settings.show_cat(),
                        this.settings.reduced_graphics() || crate::motion::still(),
                    )
                {
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
