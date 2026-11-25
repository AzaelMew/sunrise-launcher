//! Saving a look: the skin form, read-backs, unconfirmed changes, and what gets recorded.

use super::*;

#[test]
fn a_skin_is_sent_as_a_well_formed_form_with_its_arm_model() {
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    for slim in [true, false] {
        let skin = png(64, 64);
        let capes::Written::Profile(worn) = skins::upload(&ep, "mc-token", &skin, slim).unwrap()
        else {
            panic!("the services sent the profile back");
        };
        let n = m.lock().unwrap().uploads.len();
        assert_eq!(
            worn.skin,
            Some((
                format!("http://textures.minecraft.net/texture/up{n}"),
                if slim { "slim" } else { "classic" }.into()
            ))
        );
        let m = m.lock().unwrap();
        let upload = m.uploads.last().unwrap();
        assert_eq!(upload.variant, if slim { "slim" } else { "classic" });
        assert_eq!(
            upload.file, skin,
            "the file part is the picture, byte for byte"
        );
        assert_eq!(
            (upload.file_name.as_str(), upload.file_type.as_str()),
            ("skin.png", "image/png")
        );
        assert!(upload.closed, "the closing boundary ends the body");
        let (method, path, _) = m.log.last().unwrap();
        assert_eq!(
            (method.as_str(), path.as_str()),
            ("POST", "/mc/minecraft/profile/skins")
        );
    }
    // Never sent: a picture that isn't a skin, or a shape the services don't take.
    let before = m.lock().unwrap().log.len();
    assert!(skins::upload(&ep, "mc-token", &png(46, 22), false).is_err());
    assert!(skins::upload(&ep, "mc-token", &png(128, 128), false).is_err());
    assert!(skins::upload(&ep, "mc-token", b"not a png", false).is_err());
    assert_eq!(m.lock().unwrap().log.len(), before);
    assert!(skins::upload(&offline(), "mc-token", &png(64, 64), false)
        .unwrap_err()
        .contains("Could not reach"));
}

#[test]
fn an_acknowledgement_without_a_profile_is_read_back() {
    for status in [200, 204] {
        let (m, ep) = mock(|m| m.skin_answer_empty = Some(status));
        let worn = skins::upload(&ep, "mc-token", &png(64, 64), true).unwrap();
        let capes::Written::Profile(worn) = worn else {
            panic!("read back: {worn:?}");
        };
        assert_eq!(
            worn.skin.map(|(_, model)| model).as_deref(),
            Some("slim"),
            "read back from the profile, not taken as no skin"
        );
        let log = &m.lock().unwrap().log;
        let (method, path, _) = log.last().unwrap();
        assert_eq!(
            (method.as_str(), path.as_str()),
            ("GET", "/mc/minecraft/profile")
        );
    }
}

#[test]
fn an_accepted_change_whose_read_back_fails_is_unconfirmed_not_refused() {
    // 204, then the reading fails: an error page, or a body whose `capes` field is malformed.
    for reading in [503, 200] {
        let (_m, ep) = mock(|m| {
            m.skin_answer_empty = Some(204);
            m.cape_answer_empty = Some(204);
            m.profile_answers = [reading, reading].into();
        });
        assert!(matches!(
            skins::upload(&ep, "mc-token", &png(64, 64), false),
            Ok(capes::Written::Unconfirmed(_))
        ));
        assert!(matches!(
            capes::wear(&ep, "mc-token", Some("c1")),
            Ok(capes::Written::Unconfirmed(_))
        ));
    }
}

#[test]
fn a_look_is_recorded_on_its_own_account_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("accounts.json");
    let store = MemoryStore::default();
    let mut other = notch();
    other.uuid = "b09600dff4354d4fb56776eec1e55083".into();
    other.name = "jeb_".into();
    let mut me = notch();
    me.cape = Some("http://textures.minecraft.net/texture/c2".into());
    let accounts = Accounts {
        accounts: vec![me.clone(), other.clone()],
        ..Accounts::default()
    };
    accounts.save_to(&path).unwrap();
    // Meanwhile another window renamed the other account: the Save mustn't undo that.
    let mut renamed = Accounts::load_from(&path);
    renamed.accounts[1].name = "jeb".into();
    renamed.save_to(&path).unwrap();
    Accounts::save_look_to(
        &store,
        &path,
        UUID,
        Some(Some((
            "http://textures.minecraft.net/texture/new".into(),
            "slim".into(),
        ))),
        None,
    )
    .unwrap();
    let after = Accounts::load_from(&path);
    assert_eq!(
        after.accounts[0].skin.as_ref().map(|(u, _)| u.as_str()),
        Some("http://textures.minecraft.net/texture/new")
    );
    assert_eq!(after.accounts[0].cape, me.cape, "the cape left as it was");
    assert_eq!(
        after.accounts[1].name, "jeb",
        "the other account as it is now"
    );
}

#[test]
fn a_save_keeps_the_skin_the_profile_wears_now_not_an_older_one() {
    // accounts.json (and the page, its reading not back yet) still say `abc`; the account
    // wears `now`, changed in another launcher.
    let now = "http://textures.minecraft.net/texture/now";
    let (m, ep) = mock(|m| {
        m.worn = Some(Some("c2".into()));
        m.skin_url = Some(now.into());
    });
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(9));
    bench.pictures.insert(now.into(), skin_png(1));
    let out = bench.save(&m, &ep, &skin_request(skin_png(2), true, None));
    assert_eq!(
        *bench.fetched.borrow(),
        [now, UP1],
        "the picture of the skin worn now, then the one served for the upload"
    );
    let look::Kept::Kept { url, saved: kept } = &out.kept else {
        panic!("kept: {:?}", out.kept);
    };
    assert_eq!(url, now);
    assert!(!kept.slim, "with the arms it was worn with");
    assert_eq!(
        crate::skin_library::png(bench.dir(), &kept.id),
        Some(skin_png(1))
    );
    assert_eq!(out.skin, Some(look::Change::Done));
    assert_eq!(out.cape, None);
    assert_eq!(
        out.profile.as_ref().and_then(|p| p.skin.clone()),
        Some((UP1.into(), "slim".into()))
    );
    assert_eq!(
        *bench.records.borrow(),
        [record(Some(Some((UP1, "slim"))), None)],
        "only the skin recorded: the cape wasn't changed"
    );
    assert_eq!(methods(&m), ["GET", "POST"], "read, then replaced");
}

#[test]
fn a_skin_whose_predecessor_cant_be_kept_is_not_sent() {
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    // The texture server has no picture for `abc`: a failed download, not "nothing to keep".
    let bench = LookBench::new();
    let out = bench.save(&m, &ep, &skin_request(skin_png(2), false, Some(Some("c1"))));
    assert!(matches!(out.kept, look::Kept::Failed(_)), "{:?}", out.kept);
    assert!(matches!(out.skin, Some(look::Change::Failed(_))));
    assert!(!methods(&m).contains(&"POST".to_owned()));
    // The cape is its own change: it still goes through, and only it is recorded.
    assert_eq!(out.cape, Some(look::Change::Done));
    assert_eq!(*bench.records.borrow(), [record(None, Some(Some(C1)))]);
    // A saved-skins folder that can't be written stops it the same way.
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    std::fs::write(bench.dir().join("index.json"), "{ damaged").unwrap();
    let out = bench.save(&m, &ep, &skin_request(skin_png(2), false, None));
    assert!(matches!(out.kept, look::Kept::Failed(_)), "{:?}", out.kept);
    assert!(matches!(out.skin, Some(look::Change::Failed(_))));
    assert_eq!(methods(&m), ["GET"]);
}

#[test]
fn nothing_is_written_until_the_profile_says_whose_the_token_is() {
    // A profile that can't be read, or one of another account: neither the skin nor the cape
    // is sent, for a skin and cape Save as for a cape alone.
    for (why, other_account) in [("unreadable", false), ("another account's", true)] {
        for with_skin in [true, false] {
            let (m, ep) = mock(|m| {
                m.worn = Some(Some("c2".into()));
                if !other_account {
                    m.profile_answers = [503].into();
                }
            });
            let mut bench = LookBench::new();
            bench.pictures.insert(ABC.into(), skin_png(1));
            let mut request = skin_request(skin_png(2), false, Some(Some("c1")));
            if !with_skin {
                request.skin = None;
            }
            if other_account {
                request.uuid = "b09600dff4354d4fb56776eec1e55083".into();
            }
            let out = bench.save(&m, &ep, &request);
            assert_eq!(methods(&m), ["GET"], "{why}, skin {with_skin}");
            assert!(matches!(out.cape, Some(look::Change::Failed(_))), "{why}");
            assert_eq!(
                out.skin.as_ref().is_some_and(|c| !c.accepted()),
                with_skin,
                "{why}: {:?}",
                out.skin
            );
            assert_eq!(out.kept, look::Kept::Nothing);
            assert_eq!(out.profile, None);
            assert!(bench.records.borrow().is_empty());
            assert!(bench.fetched.borrow().is_empty());
        }
    }
}

#[test]
fn an_upload_accepted_but_not_read_back_is_unconfirmed_and_not_recorded() {
    // 204 to the upload, then the reading after it fails: an error page, or malformed capes.
    for reading in [503, 200] {
        let (m, ep) = mock(|m| {
            m.worn = Some(Some("c2".into()));
            m.skin_answer_empty = Some(204);
            m.profile_answers = [0, reading].into();
        });
        let mut bench = LookBench::new();
        bench.pictures.insert(ABC.into(), skin_png(1));
        let out = bench.save(&m, &ep, &skin_request(skin_png(2), false, None));
        assert!(
            matches!(out.skin, Some(look::Change::Unconfirmed(_))),
            "accepted, not refused: {:?}",
            out.skin
        );
        assert!(out.skin.as_ref().unwrap().accepted());
        assert_eq!(
            out.profile.as_ref().and_then(|p| p.skin.clone()),
            Some((ABC.into(), "classic".into())),
            "the latest profile known is the one before, not one claiming the upload"
        );
        assert!(bench.records.borrow().is_empty(), "no URL to record");
        assert_eq!(methods(&m), ["GET", "POST", "GET"]);
    }
}

#[test]
fn a_cape_profile_after_an_unconfirmed_skin_settles_it() {
    let (m, ep) = mock(|m| {
        m.worn = Some(Some("c2".into()));
        m.skin_answer_empty = Some(204);
        m.profile_answers = [0, 503].into();
    });
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    let out = bench.save(&m, &ep, &skin_request(skin_png(2), true, Some(None)));
    assert_eq!(
        out.skin,
        Some(look::Change::Done),
        "the cape's profile shows it"
    );
    assert_eq!(out.cape, Some(look::Change::Done));
    assert_eq!(
        *bench.records.borrow(),
        [record(Some(Some((UP1, "slim"))), Some(None))]
    );
}

#[test]
fn an_accepted_change_the_profile_doesnt_show_is_unconfirmed_not_to_send_again() {
    // The services took the upload and the cape, but the profile after them shows neither.
    let (m, ep) = mock(|m| {
        m.worn = Some(Some("c2".into()));
        m.upload_ignored = true;
        m.cape_ignored = true;
    });
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    let out = bench.save(&m, &ep, &skin_request(skin_png(2), false, Some(Some("c1"))));
    assert!(
        matches!(out.skin, Some(look::Change::Unconfirmed(_))),
        "{:?}",
        out.skin
    );
    assert!(
        matches!(out.cape, Some(look::Change::Unconfirmed(_))),
        "{:?}",
        out.cape
    );
    assert!(bench.records.borrow().is_empty());
    // The same skin again with other arms keeps its URL: shown, done.
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    bench.pictures.insert(UP1.into(), skin_png(1));
    let out = bench.save(&m, &ep, &skin_request(skin_png(1), true, None));
    assert_eq!(out.skin, Some(look::Change::Done));
}

#[test]
fn a_skin_the_texture_server_re_encodes_is_still_the_one_sent() {
    // A clear hat with colour under it: the server's copy has other bytes and none of that
    // colour, but it is the same skin.
    let image = image::RgbaImage::from_fn(64, 64, |x, y| {
        image::Rgba(if (40..48).contains(&x) && y < 8 {
            [200, 100, 50, 0]
        } else {
            [x as u8, y as u8, 9, 255]
        })
    });
    let mut sent = std::io::Cursor::new(Vec::new());
    image.write_to(&mut sent, image::ImageFormat::Png).unwrap();
    let sent = sent.into_inner();
    assert_ne!(served_copy(&sent), sent, "other bytes");
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    let out = bench.save(&m, &ep, &skin_request(sent, false, None));
    assert_eq!(out.skin, Some(look::Change::Done));
}

#[test]
fn a_new_url_is_not_taken_for_the_skin_sent_unless_its_picture_is() {
    // The profile after the upload names a new skin with the arms sent, but the texture server
    // serves another picture there (another launcher's upload won), or none at all.
    for served in [Some(skin_png(7)), None] {
        let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
        let mut bench = LookBench::new();
        bench.pictures.insert(ABC.into(), skin_png(1));
        if let Some(served) = &served {
            bench.pictures.insert(UP1.into(), served.clone());
        } else {
            // What is served there isn't a skin at all.
            bench.pictures.insert(UP1.into(), b"gone".to_vec());
        }
        let out = bench.save(&m, &ep, &skin_request(skin_png(2), false, None));
        assert!(
            matches!(out.skin, Some(look::Change::Unconfirmed(_))),
            "{served:?}: {:?}",
            out.skin
        );
        assert!(bench.records.borrow().is_empty());
        assert_eq!(methods(&m), ["GET", "POST"], "not sent twice");
    }
}

#[test]
fn a_saved_skin_takes_the_arms_it_was_worn_with() {
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    let saved = crate::skin_library::add(bench.dir(), &skin_png(3), "Knight", Some(false)).unwrap();
    let mut request = skin_request(skin_png(3), true, None);
    request.skin.as_mut().unwrap().picture = look::Picture::New {
        png: skin_png(3),
        saved: Some(saved.id.clone()),
    };
    let out = bench.save(&m, &ep, &request);
    assert_eq!((out.skin, out.arms), (Some(look::Change::Done), None));
    let listed = crate::skin_library::list(bench.dir()).unwrap();
    assert!(listed.iter().find(|s| s.id == saved.id).unwrap().slim);
}

#[test]
fn new_arms_keep_the_picture_the_account_wears_when_the_save_starts() {
    let (m, ep) = mock(|m| m.worn = Some(Some("c2".into())));
    let mut bench = LookBench::new();
    bench.pictures.insert(ABC.into(), skin_png(1));
    let out = bench.save(
        &m,
        &ep,
        &arms_request(look::WornPicture::Texture(ABC.into()), true),
    );
    assert_eq!(out.skin, Some(look::Change::Done));
    let upload = &m.lock().unwrap().uploads[0];
    assert_eq!(
        (upload.file.clone(), upload.variant.as_str()),
        (skin_png(1), "slim"),
        "the picture worn, fetched from Mojang, with the new arms"
    );
    let look::Kept::Kept { saved, .. } = &out.kept else {
        panic!("{:?}", out.kept);
    };
    let listed = crate::skin_library::list(bench.dir()).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(
        listed[0].slim && listed[0].id == saved.id,
        "kept, now with the arms it is worn with"
    );
    // The account's default skin: the page's picture of it is the one sent.
    let (m, ep) = mock(|m| {
        m.worn = Some(Some("c2".into()));
        m.no_skin = true;
    });
    let bench = LookBench::new();
    let out = bench.save(
        &m,
        &ep,
        &arms_request(look::WornPicture::Default(skin_png(4)), true),
    );
    assert_eq!(out.skin, Some(look::Change::Done));
    assert_eq!(out.kept, look::Kept::Nothing);
    assert_eq!(m.lock().unwrap().uploads[0].file, skin_png(4));
}

#[test]
fn new_arms_on_a_stale_page_replace_nothing_and_show_the_skin_worn_now() {
    // The page read `old`; another launcher has since put `abc` on. Or the page shows the
    // default skin, and the account wears `abc`; or the other way round.
    let old = "http://textures.minecraft.net/texture/old";
    for (worn, no_skin) in [
        (look::WornPicture::Texture(old.into()), false),
        (look::WornPicture::Default(skin_png(4)), false),
        (look::WornPicture::Texture(ABC.into()), true),
    ] {
        let (m, ep) = mock(|m| {
            m.worn = Some(Some("c2".into()));
            m.no_skin = no_skin;
        });
        let mut bench = LookBench::new();
        bench.pictures.insert(ABC.into(), skin_png(1));
        bench.pictures.insert(old.into(), skin_png(2));
        let out = bench.save(&m, &ep, &arms_request(worn.clone(), true));
        assert!(
            matches!(out.skin, Some(look::Change::Failed(_))),
            "{worn:?}: {:?}",
            out.skin
        );
        assert_eq!(methods(&m), ["GET"], "{worn:?}: nothing uploaded");
        assert_eq!(out.kept, look::Kept::Nothing);
        assert!(bench.fetched.borrow().is_empty(), "nothing archived");
        assert!(crate::skin_library::list(bench.dir()).unwrap().is_empty());
        let shown = out.profile.and_then(|p| p.skin).map(|(url, _)| url);
        assert_eq!(
            shown,
            (!no_skin).then(|| ABC.to_owned()),
            "the page is told what is worn now"
        );
    }
}
