//! Owned capes: listing, wearing one or none, and their tokens and pictures.

use super::*;

#[test]
fn the_owned_capes_are_listed_and_one_or_none_is_worn() {
    let (m, ep) = mock(|_| {});
    let names = |capes: &[capes::Cape]| -> Vec<(String, bool)> {
        capes.iter().map(|c| (c.alias.clone(), c.active)).collect()
    };
    let listed = capes::read(&ep, "mc-token").unwrap();
    assert_eq!(
        names(&listed.capes),
        [("Migrator".into(), false), ("Vanilla".into(), true)]
    );
    assert_eq!(
        listed.capes[0].url,
        "http://textures.minecraft.net/texture/c1"
    );
    assert_eq!(
        listed.skin,
        Some((
            "http://textures.minecraft.net/texture/abc".into(),
            "slim".into()
        )),
        "the skin from the same reading"
    );
    let profile = |w: Result<capes::Written, String>| match w.unwrap() {
        capes::Written::Profile(p) => p,
        other => panic!("not confirmed: {other:?}"),
    };
    let worn = profile(capes::wear(&ep, "mc-token", Some("c1")));
    assert_eq!(
        names(&worn.capes),
        [("Migrator".into(), true), ("Vanilla".into(), false)]
    );
    let none = profile(capes::wear(&ep, "mc-token", None));
    assert!(none.capes.iter().all(|c| !c.active));
    assert_eq!(
        capes::wear(&ep, "mc-token", Some("not-owned")),
        Err("This account doesn't own that cape.".into())
    );
    let log = &m.lock().unwrap().log;
    assert!(log.iter().any(|(method, path, body)| method == "PUT"
        && path == "/mc/minecraft/profile/capes/active"
        && body.contains("\"capeId\":\"c1\"")));
    assert!(log
        .iter()
        .any(|(method, path, _)| method == "DELETE" && path.ends_with("/capes/active")));
}

#[test]
fn a_cape_change_with_no_network_says_so() {
    let refused = capes::wear(&offline(), "mc-token", None).unwrap_err();
    assert!(refused.contains("Could not reach the Minecraft services"));
}

#[test]
fn a_profile_with_a_cape_pictured_elsewhere_is_unreadable() {
    let cape =
        |id: &str, url: &str| json!({"id": id, "state": "INACTIVE", "url": url, "alias": id});
    let profile = |capes: Value| json!({"id": UUID, "skins": [], "capes": capes});
    let kept = capes::Profile::of(&profile(json!([cape(
        "a",
        "http://textures.minecraft.net/texture/abc123"
    )])))
    .unwrap();
    assert_eq!(kept.capes[0].id, "a");
    // Not a shorter list of capes: a profile the launcher can't take as what the account owns.
    for elsewhere in [
        "http://example.com/texture/abc123",
        "http://textures.minecraft.net/texture/../x",
    ] {
        assert_eq!(
            capes::Profile::of(&profile(json!([
                cape("a", "http://textures.minecraft.net/texture/abc123"),
                cape("b", elsewhere)
            ]))),
            None,
            "{elsewhere}"
        );
    }
}

#[test]
fn a_token_for_a_cape_change_is_renewed_and_kept_when_it_runs_out() {
    let (_m, ep) = mock(|_| {});
    let store = MemoryStore::default();
    let fresh = secrets("mc-token-kept", true);
    signed_in(&store, &fresh);
    assert_eq!(
        token_for(&ep, Some("test-client"), &store, &notch()).as_deref(),
        Ok("mc-token-kept"),
        "a token with time left is used as it is"
    );
    let old = Secrets {
        minecraft_expires_at: now() + 60,
        ..secrets("mc-token-old", true)
    };
    store
        .set(UUID, &serde_json::to_string(&old).unwrap())
        .unwrap();
    assert_eq!(
        token_for(&ep, Some("test-client"), &store, &notch()).as_deref(),
        Ok("mc-token")
    );
    assert_eq!(kept(&store).minecraft_token, "mc-token");
    store.delete(UUID).unwrap();
    assert!(token_for(&ep, Some("test-client"), &store, &notch())
        .unwrap_err()
        .contains("no longer signed in"));
}
