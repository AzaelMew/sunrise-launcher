//! Skin and cape pictures as the Skins tab's player model wears them: decoded, completed and laid
//! out the way the game does it. The model itself is drawn by `sunrise_launcher_scenery::model`.

pub use sunrise_launcher_scenery::model::{render, Pose, Texture};

/// The PNG `png` decoded, refused when its header says it is wider than `max_w` or taller than
/// `max_h`: the size is checked before any texel is allocated, so a small file that unpacks to
/// an enormous picture costs nothing.
pub fn decode(png: &[u8], max_w: u32, max_h: u32) -> Option<image::RgbaImage> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(png), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_w);
    limits.max_image_height = Some(max_h);
    limits.max_alloc = Some(16 << 20);
    reader.limits(limits);
    Some(reader.decode().ok()?.to_rgba8())
}

/// A skin's texels as its file has them, uncompleted: 64 by 64 or 64 by 32. What tells two
/// skins apart ([`crate::skin_library::id_of`]), since completing makes texels the file left
/// clear opaque.
pub fn raw_skin(png: &[u8]) -> Option<Texture> {
    let image = decode(png, 64, 64)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w != 64 || (h != 64 && h != 32) {
        return None;
    }
    Some(Texture {
        texels: image.pixels().map(|p| p.0).collect(),
        w,
        h,
    })
}

/// A skin picture as the model wears it, completed as vanilla's
/// `SkinTextureDownloader.processLegacySkin` does: 64 by 64, or an old 64 by 32 skin given its
/// left arm and leg. None for any other shape (the only two the services take) or something
/// that isn't a PNG.
pub fn skin(png: &[u8]) -> Option<Texture> {
    let image = decode(png, 64, 64)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w != 64 || (h != 64 && h != 32) {
        return None;
    }
    let legacy = h == 32;
    let mut texels = vec![[0; 4]; 64 * 64];
    for (i, p) in image.pixels().enumerate() {
        texels[i] = p.0;
    }
    let at = |x: i64, y: i64| (y * 64 + x) as usize;
    if legacy {
        // (from x, from y, to x offset, to y offset, width, height): the right arm and leg,
        // mirrored into the left ones' places.
        const RECTS: [(i64, i64, i64, i64, i64, i64); 12] = [
            (4, 16, 16, 32, 4, 4),
            (8, 16, 16, 32, 4, 4),
            (0, 20, 24, 32, 4, 12),
            (4, 20, 16, 32, 4, 12),
            (8, 20, 8, 32, 4, 12),
            (12, 20, 16, 32, 4, 12),
            (44, 16, -8, 32, 4, 4),
            (48, 16, -8, 32, 4, 4),
            (40, 20, 0, 32, 4, 12),
            (44, 20, -8, 32, 4, 12),
            (48, 20, -16, 32, 4, 12),
            (52, 20, -8, 32, 4, 12),
        ];
        for (sx, sy, ox, oy, rw, rh) in RECTS {
            for y in 0..rh {
                for x in 0..rw {
                    texels[at(sx + ox + rw - 1 - x, sy + oy + y)] = texels[at(sx + x, sy + y)];
                }
            }
        }
    }
    let area = |x0: usize, y0: usize, x1: usize, y1: usize| {
        (y0..y1).flat_map(move |y| (x0..x1).map(move |x| y * 64 + x))
    };
    // In vanilla's order: the head's base opaque (`setNoAlpha`); for an old skin, a hat region
    // with no clear texel at all is filler, not a hat, and goes clear (`doNotchTransparencyHack`,
    // which reads the whole 32..64 by 0..32 region before the lower base is made opaque); then
    // the body, arms and legs' base opaque, on every skin.
    for i in area(0, 0, 32, 16) {
        texels[i][3] = 255;
    }
    if legacy && area(32, 0, 64, 32).all(|i| texels[i][3] >= 128) {
        for i in area(32, 0, 64, 32) {
            texels[i][3] = 0;
        }
    }
    for i in area(0, 16, 64, 32).chain(area(16, 48, 48, 64)) {
        texels[i][3] = 255;
    }
    Some(Texture {
        texels,
        w: 64,
        h: 64,
    })
}

/// A cape picture on the smallest 64·2ⁿ by 32·2ⁿ canvas it fits: Mojang's 64 by 32 capes, and
/// the 46 by 22, 92 by 44 and old 22 by 17 layouts other capes come in, all keep
/// the 64 by 32 layout's coordinates.
pub fn cape(png: &[u8]) -> Option<Texture> {
    let image = decode(png, 2048, 1024)?;
    let (w, h) = (image.width() as usize, image.height() as usize);
    if w == 0 || h == 0 || w > 2048 || h > 1024 {
        return None;
    }
    let mut k = 1;
    while 64 * k < w || 32 * k < h {
        k *= 2;
    }
    // The outer face must be inside the picture.
    if 11 * k > w || 17 * k > h {
        return None;
    }
    let (cw, ch) = (64 * k, 32 * k);
    let mut texels = vec![[0; 4]; cw * ch];
    for (x, y, p) in image.enumerate_pixels() {
        texels[y as usize * cw + x as usize] = p.0;
    }
    Some(Texture {
        texels,
        w: cw,
        h: ch,
    })
}

/// The cape's outer face (10 by 16 of the 64 by 32 layout, at 1, 1), as a player behind it sees
/// it: what a cape's tile shows.
pub fn cape_front(cape: &Texture) -> Texture {
    let k = cape.w / 64;
    let (w, h) = (10 * k, 16 * k);
    let texels = (k..k + h)
        .flat_map(|y| (k..k + w).map(move |x| (x, y)))
        .map(|(x, y)| cape.texel(x, y))
        .collect();
    Texture { texels, w, h }
}

/// Whether the skin `png` is made for slim arms: its right arm's fourth column of front texels
/// is clear, as slim skins leave it. Read from the file itself: the model's copy makes that
/// column opaque, as the game does.
pub fn looks_slim(png: &[u8]) -> bool {
    decode(png, 64, 64).is_some_and(|image| {
        image.width() == 64 && (20..32).all(|y| image.get_pixel(54, y).0[3] == 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let image = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba(f(x, y)));
        let mut out = std::io::Cursor::new(Vec::new());
        image.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    /// A skin whose every base texel says which part it belongs to: red for the head, green for
    /// the body, blue for the arms and legs; its outer layer clear.
    fn marked() -> Texture {
        skin(&png(64, 64, |x, y| {
            let outer =
                y < 16 && x >= 32 || (32..48).contains(&y) || y >= 48 && !(16..48).contains(&x);
            if outer {
                [0, 0, 0, 0]
            } else if y < 16 {
                [255, 0, 0, 255]
            } else if (16..32).contains(&y) && (16..40).contains(&x) {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        }))
        .unwrap()
    }

    fn colour_at(out: &[[u8; 4]], w: usize, x: usize, y: usize) -> [u8; 4] {
        out[y * w + x]
    }

    #[test]
    fn the_model_stands_head_up_and_centred() {
        let skin = marked();
        let pose = Pose {
            skin: &skin,
            slim: false,
            cape: None,
            yaw: 0.,
            pitch: 0.,
        };
        let (w, h) = (120, 190);
        let out = render(&pose, w, h);
        let head = colour_at(&out, w, w / 2, h / 2 - 60);
        let body = colour_at(&out, w, w / 2, h / 2 - 15);
        assert!(
            head[0] > 100 && head[1] == 0,
            "the head is up top: {head:?}"
        );
        assert!(body[1] > 100 && body[0] == 0, "the body under it: {body:?}");
        assert_eq!(colour_at(&out, w, 2, 2)[3], 0, "clear around the model");
    }

    #[test]
    fn a_turned_model_shows_its_back_and_the_cape_on_it() {
        let skin = marked();
        let cape = cape(&png(64, 32, |_, _| [250, 250, 0, 255])).unwrap();
        let (w, h) = (120, 190);
        let pose = |yaw: f32, cape: Option<&Texture>| {
            render(
                &Pose {
                    skin: &skin,
                    slim: false,
                    cape,
                    yaw,
                    pitch: 0.,
                },
                w,
                h,
            )
        };
        let front = pose(0., Some(&cape));
        let at_back = colour_at(&front, w, w / 2, h / 2 - 15);
        assert!(
            at_back[1] > 100 && at_back[2] == 0,
            "the body hides the cape from the front"
        );
        let back = pose(std::f32::consts::PI, Some(&cape));
        let caped = colour_at(&back, w, w / 2, h / 2 - 15);
        assert!(
            caped[0] > 100 && caped[1] > 100,
            "the cape covers the back: {caped:?}"
        );
        let bare = pose(std::f32::consts::PI, None);
        let body = colour_at(&bare, w, w / 2, h / 2 - 15);
        assert!(
            body[1] > 100 && body[0] == 0,
            "no cape, the body's back: {body:?}"
        );
    }

    #[test]
    fn legacy_skins_get_their_left_limbs_and_others_are_refused() {
        let legacy = skin(&png(64, 32, |x, y| [x as u8, y as u8, 9, 255])).unwrap();
        assert_eq!((legacy.w, legacy.h), (64, 64));
        // The left leg's front (20, 52) mirrors the right leg's front (4..8, 20..32).
        assert_eq!(legacy.texel(20, 52), [7, 20, 9, 255]);
        assert!(skin(&png(64, 48, |_, _| [0; 4])).is_none());
        assert!(skin(b"not a png").is_none());
        // Only the two shapes the services take: no HD skins saved that couldn't be worn.
        assert!(skin(&png(128, 128, |_, _| [1, 2, 3, 255])).is_none());
        assert!(skin(&png(128, 64, |_, _| [1, 2, 3, 255])).is_none());
    }

    /// A PNG whose header claims `w` by `h` texels, with no picture data behind it.
    fn header_only(w: u32, h: u32) -> Vec<u8> {
        let mut real = png(1, 1, |_, _| [0; 4]);
        // IHDR's width and height sit at bytes 16..24; its CRC (over 12..29, at 29..33) is redone.
        real[16..20].copy_from_slice(&w.to_be_bytes());
        real[20..24].copy_from_slice(&h.to_be_bytes());
        let crc = crc32(&real[12..29]);
        real[29..33].copy_from_slice(&crc.to_be_bytes());
        real
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for b in bytes {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn an_oversized_header_is_refused_before_decoding() {
        let huge = header_only(30_000, 30_000);
        assert!(huge.len() < 100, "a tiny file");
        assert!(decode(&huge, 2048, 1024).is_none());
        assert!(skin(&huge).is_none());
        assert!(cape(&huge).is_none());
    }

    #[test]
    fn a_real_picture_one_texel_over_a_limit_is_refused_by_it() {
        let clear = |_, _| [0; 4];
        // Whole, decodable PNGs a few kilobytes big once decoded, far below the allocation
        // limit: only the width and height check can turn them down.
        for (w, h, max_w, max_h) in [
            (65, 64, 64, 64),
            (64, 65, 64, 64),
            (2049, 1, 2048, 1024),
            (1, 1025, 2048, 1024),
        ] {
            let over = png(w, h, clear);
            assert!(
                decode(&over, w, h).is_some(),
                "{w}x{h} decodes within its own size"
            );
            assert!(
                decode(&over, max_w, max_h).is_none(),
                "{w}x{h} over the limit"
            );
        }
        // At the limits, they pass.
        assert!(decode(&png(64, 64, clear), 64, 64).is_some());
        assert!(decode(&png(2048, 1, clear), 2048, 1024).is_some());
        assert!(decode(&png(1, 1024, clear), 2048, 1024).is_some());
        // The skin and cape readers use those limits.
        assert!(skin(&png(65, 64, clear)).is_none());
        assert!(skin(&png(64, 65, clear)).is_none());
        assert!(cape(&png(2049, 32, clear)).is_none());
        assert!(cape(&png(64, 1025, clear)).is_none());
    }

    #[test]
    fn a_modern_skins_base_is_opaque_and_its_overlay_kept() {
        // Clear everywhere: the base layer comes out opaque, the outer layer stays clear.
        let clear = skin(&png(64, 64, |_, _| [5, 6, 7, 0])).unwrap();
        assert_eq!(clear.texel(8, 8)[3], 255, "head base");
        assert_eq!(clear.texel(20, 20)[3], 255, "body base");
        assert_eq!(clear.texel(20, 52)[3], 255, "left leg base");
        assert_eq!(clear.texel(40, 8)[3], 0, "hat");
        assert_eq!(clear.texel(20, 36)[3], 0, "jacket");
        // Never the old-skin hat hack on a modern skin: an opaque hat stays.
        let hatted = skin(&png(64, 64, |_, _| [5, 6, 7, 255])).unwrap();
        assert_eq!(hatted.texel(40, 8)[3], 255);
    }

    #[test]
    fn an_old_skin_keeps_its_hat_when_any_of_the_region_is_clear() {
        // Opaque everywhere: the hat is filler and goes clear, its colour kept.
        let filler = skin(&png(64, 32, |_, _| [5, 6, 7, 255])).unwrap();
        assert_eq!(filler.texel(40, 8), [5, 6, 7, 0]);
        assert_eq!(filler.texel(44, 20)[3], 255, "the arm made opaque after");
        // One clear texel below the hat row, inside the checked region: the hat is real.
        let real = skin(&png(64, 32, |x, y| {
            if (x, y) == (60, 20) {
                [0; 4]
            } else {
                [5, 6, 7, 255]
            }
        }))
        .unwrap();
        assert_eq!(real.texel(40, 8)[3], 255, "the hat kept");
    }

    #[test]
    fn capes_keep_their_layout_on_their_canvas() {
        for (w, h, k) in [
            (64, 32, 1),
            (46, 22, 1),
            (22, 17, 1),
            (92, 44, 2),
            (128, 64, 2),
        ] {
            let laid = cape(&png(w, h, |x, y| [x as u8, y as u8, 7, 255])).unwrap();
            assert_eq!((laid.w, laid.h), (64 * k, 32 * k), "{w}x{h}");
            let front = cape_front(&laid);
            assert_eq!((front.w, front.h), (10 * k, 16 * k));
            assert_eq!(front.texels[0], [k as u8, k as u8, 7, 255], "{w}x{h}");
        }
        assert!(cape(&png(8, 8, |_, _| [0; 4])).is_none());
    }

    #[test]
    fn slim_skins_are_told_by_their_clear_arm_column() {
        assert!(!looks_slim(&png(64, 64, |_, _| [9, 9, 9, 255])));
        let slim = png(64, 64, |x, y| {
            if x == 54 && (20..32).contains(&y) {
                [0; 4]
            } else {
                [9, 9, 9, 255]
            }
        });
        assert!(looks_slim(&slim));
    }
}
