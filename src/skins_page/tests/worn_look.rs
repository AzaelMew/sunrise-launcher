//! What the page shows as worn: one reading of the profile, its arms, the account's default skin.

use super::*;

#[test]
fn the_worn_look_comes_from_one_reading_of_the_profile() {
    let mut skins = Skins::new_for("a");
    let recorded = (SKIN.to_owned(), "slim".to_owned());
    assert_eq!(
        skins.worn(Some(&recorded), false),
        Worn {
            skin: SkinPick::Texture(SKIN.into()),
            slim: true,
            cape: None,
            texture: Some(SKIN.into()),
        }
    );
    read(
        &mut skins,
        profile(
            "http://textures.minecraft.net/texture/new",
            false,
            Some("c1"),
        ),
    );
    let worn = skins.worn(Some(&recorded), false);
    assert!(!worn.slim);
    assert_eq!(worn.cape, Some(CapePick::Minecraft("c1".into())));
    let again = skins.read_asked();
    skins.read_answered("a", again, Err("offline".into()), false);
    assert!(matches!(skins.profile, Some(Ok(_))));
}

#[test]
fn the_worn_skins_tile_uses_the_profiles_arms() {
    let mut skins = Skins::new_for("a");
    read(&mut skins, profile(SKIN, true, None));
    skins.kept.insert(SKIN.into(), KEPT.into());
    let worn = worn(&skins);
    assert_eq!(worn.skin, SkinPick::Saved(KEPT.into()));
    skins.edit.skin = Some(worn.skin.clone());
    skins.edit.slim = Some(worn.slim);
    skins.settle(&worn);
    assert!(skins.edit.is_empty());
    // Removed from the saved skins, it is the skin at its URL again.
    skins.removed(KEPT);
    assert_eq!(
        super::tests::worn(&skins).skin,
        SkinPick::Texture(SKIN.into())
    );
}

#[test]
fn the_account_default_skin_comes_with_the_arms_shown() {
    let resources = tempfile::tempdir().unwrap();
    // An id the game gives a slim default skin, then the same skin with wide arms.
    let uuid = (0..64u32)
        .map(|n| format!("{n:032x}"))
        .find(|uuid| {
            sunrise_launcher::heads::default_skin_file(resources.path(), uuid)
                .is_some_and(|(_, slim)| slim)
        })
        .unwrap();
    let (slim_file, _) =
        sunrise_launcher::heads::default_skin_file(resources.path(), &uuid).unwrap();
    let name = slim_file.file_stem().unwrap().to_str().unwrap().to_owned();
    assert_eq!(
        account_default_path(resources.path(), &uuid, true),
        Some(slim_file)
    );
    assert_eq!(
        account_default_path(resources.path(), &uuid, false),
        Some(default_skin_path(resources.path(), &name, false)),
        "the wide picture of the same skin"
    );
}
