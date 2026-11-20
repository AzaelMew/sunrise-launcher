//! Game internals are declared at the manifest root, not in the native table.
use sunrise_launcher_smod::{Manifest, manifest::Internal};

const HEADER: &str =
    "id='owner'\nname='Owner'\nversion='1.0.0'\napi='0.2'\ncapabilities=['registries']\n";
const NATIVE: &str = "[native]\ngame='26.3'\nsunrise='aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\nmixins='bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'\n";

#[test]
fn reads_root_records_in_both_toml_forms() {
    let expected = vec![
        Internal {
            path: "Round::listening".into(),
            kind: Some("field".into()),
            hash: Some("field-hash".into()),
        },
        Internal {
            path: "Round::listening".into(),
            kind: Some("method".into()),
            hash: Some("method-hash".into()),
        },
        Internal {
            path: "Game".into(),
            kind: None,
            hash: Some("game-hash".into()),
        },
    ];
    for declarations in [
        "internals=[{path='Round::listening',kind='field',hash='field-hash'},{path='Round::listening',kind='method',hash='method-hash'},{path='Game',hash='game-hash'}]\n",
        "[[internals]]\npath='Round::listening'\nkind='field'\nhash='field-hash'\n[[internals]]\npath='Round::listening'\nkind='method'\nhash='method-hash'\n[[internals]]\npath='Game'\nhash='game-hash'\n",
    ] {
        let manifest = Manifest::parse(&format!("{HEADER}{declarations}{NATIVE}")).unwrap();
        assert_eq!(manifest.internals, expected);
    }
}

#[test]
fn native_table_declarations_are_unknown_fields_not_an_alternate_reader() {
    for declarations in [
        "internals=[{path='Game',hash='h'}]\n",
        "[[native.internals]]\npath='Game'\nhash='h'\n",
    ] {
        let error = Manifest::parse(&format!("{HEADER}{NATIVE}{declarations}")).unwrap_err();
        assert!(error.contains("unknown field `internals`"), "{error}");
    }
}

#[test]
fn root_records_require_a_native_mod_and_unique_nonempty_entries() {
    for declarations in [
        "[{path='',hash='h'}]",
        "[{path='Game',hash=''}]",
        "[{path='Game',kind='',hash='h'}]",
        "[{path='Game',hash='a'},{path='Game',hash='b'}]",
        "[{path='Game::field',kind='field',hash='a'},{path='Game::field',kind='field',hash='b'}]",
    ] {
        let error =
            Manifest::parse(&format!("{HEADER}internals={declarations}\n{NATIVE}")).unwrap_err();
        assert!(
            error
                .contains("empty internal path, kind or hash, or duplicate internal path and kind"),
            "{error}"
        );
    }
    let error =
        Manifest::parse(&format!("{HEADER}internals=[{{path='Game',hash='h'}}]\n")).unwrap_err();
    assert!(error.contains("internals need a [native] mod"), "{error}");
}

#[test]
fn native_modules_stay_native() {
    let manifest = Manifest::parse(&format!("{HEADER}{NATIVE}[[native.module]]\nid='support'\nfile='src/game.rs'\nname='support'\nsource='src/support.rs'\n")).unwrap();
    assert_eq!(manifest.native.unwrap().module[0].id, "support");
}
