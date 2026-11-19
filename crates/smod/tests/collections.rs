//! Reading `collections` and planning installs from a collection's chosen members.
use serde_json::{Value, json};
use sunrise_launcher_smod::{
    index::{
        Index,
        resolve::{self, InstallPlan, Request, RequestResult},
    },
    manifest::Side,
};

fn version(number: &str, dependencies: Value) -> Value {
    json!({
        "version": number, "api": "0.2", "game": "26.3", "dependencies": dependencies,
        "file": {
            "name": "m.smod", "url": "https://example.test/m", "size": 1,
            "sha256": "0000000000000000000000000000000000000000000000000000000000000001",
        },
    })
}

fn project(id: &str, versions: Vec<Value>) -> Value {
    json!({
        "id": id, "kind": "mod", "name": id, "summary": "A fixture.", "license": "MIT",
        "source": "https://example.test/mods", "versions": versions,
    })
}

fn collection(id: &str, members: Value) -> Value {
    json!({
        "id": id, "name": "Sunrise Creatures", "summary": "Animals.", "description": "Markdown.",
        "owners": ["EAnathos"], "icon": "https://example.test/icon.png", "members": members,
    })
}

/// `creature` is a library the members need, `vulture` needs `butterflies` (a member), and
/// `strict` needs a `creature` newer than any listed.
fn index(collections: Value) -> Index {
    let projects = vec![
        project("creature", vec![version("0.1.0", json!([]))]),
        project(
            "butterflies",
            vec![version(
                "0.1.0",
                json!([{"id": "creature", "version": "^0.1"}]),
            )],
        ),
        project(
            "vulture",
            vec![version(
                "0.1.0",
                json!([{"id": "butterflies", "version": "^0.1"}]),
            )],
        ),
        project(
            "strict",
            vec![version(
                "1.0.0",
                json!([{"id": "creature", "version": "^2"}]),
            )],
        ),
    ];
    let mut text = json!({"schema": 1, "projects": projects});
    if !collections.is_null() {
        text["collections"] = collections;
    }
    Index::parse(&text.to_string()).unwrap()
}

fn plan(index: &Index, requests: &[Request]) -> InstallPlan {
    resolve::install(index, requests, &[], "26.3", Side::Both, |_, _| true)
}

fn ids(rows: &[resolve::ModVersion]) -> Vec<&str> {
    rows.iter().map(|row| row.id.as_str()).collect()
}

#[test]
fn an_index_without_the_field_has_no_collections() {
    let index = index(Value::Null);
    assert!(index.collections.is_empty());
    let text =
        json!({"schema": 1, "projects": [project("creature", vec![version("0.1.0", json!([]))])]});
    assert!(
        Index::parse(&text.to_string())
            .unwrap()
            .collections
            .is_empty()
    );
}

#[test]
fn a_malformed_row_is_skipped_without_dropping_the_rest() {
    let members = json!([{"id": "creature"}, {"id": "butterflies"}]);
    let index = index(json!([
        {"id": "broken", "name": 7},
        "not even an object",
        collection("good", members.clone()),
        collection("creature", members),
    ]));
    assert_eq!(index.collections.len(), 1);
    assert_eq!(index.collections[0].id, "good");
    assert_eq!(index.projects.len(), 4);
    let left: Vec<_> = index.left_out.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(left, ["broken", "collections[1]", "creature"]);
    assert!(index.left_out[0].reason.starts_with("invalid collection"));
    assert!(index.left_out[2].reason.contains("already a project id"));
}

#[test]
fn a_collection_naming_an_unlisted_member_is_left_out() {
    let index = index(json!([collection(
        "ghosts",
        json!([{"id": "creature"}, {"id": "nobody"}])
    )]));
    assert!(index.collections.is_empty());
    assert!(
        index.left_out[0]
            .reason
            .contains("member nobody is not a listed mod")
    );
}

#[test]
fn default_false_starts_a_member_unticked() {
    let index = index(json!([collection(
        "c",
        json!([{"id": "creature"}, {"id": "butterflies", "default": true}, {"id": "vulture", "default": false}])
    )]));
    let c = index.collection("c").unwrap();
    assert_eq!(
        c.default_members().collect::<Vec<_>>(),
        ["creature", "butterflies"]
    );
    assert!(!c.members[2].default);
    // Members keep the registry's order, and the default is not written back when true.
    let written = serde_json::to_value(&index).unwrap();
    assert_eq!(
        written["collections"][0]["members"][0],
        json!({"id": "creature"})
    );
    assert_eq!(
        written["collections"][0]["members"][2]["default"],
        json!(false)
    );
}

#[test]
fn a_dependency_that_is_not_a_member_is_selected_and_names_who_needs_it() {
    let index = index(json!([collection(
        "c",
        json!([{"id": "butterflies"}, {"id": "vulture"}])
    )]));
    let c = index.collection("c").unwrap();
    let plan = plan(&index, &c.requests(&["butterflies", "vulture"]));
    assert_eq!(ids(&plan.install), ["butterflies", "creature", "vulture"]);
    assert_eq!(plan.required_by["creature"], ["butterflies", "vulture"]);
    assert!(!plan.requested.contains_key("creature"));
}

#[test]
fn a_member_needed_by_another_chosen_member_stays_selected() {
    let index = index(json!([collection(
        "c",
        json!([{"id": "butterflies"}, {"id": "vulture"}])
    )]));
    let c = index.collection("c").unwrap();
    let plan = plan(&index, &c.requests(&["butterflies", "vulture"]));
    assert_eq!(
        plan.requested["butterflies"],
        RequestResult::Installable {
            version: "0.1.0".into()
        }
    );
    assert_eq!(plan.required_by["butterflies"], ["vulture"]);
    // Untick the member that needs it, and nothing says it is needed by anyone.
    let alone = self::plan(&index, &c.requests(&["butterflies"]));
    assert!(!alone.required_by.contains_key("butterflies"));
}

#[test]
fn only_members_are_requested() {
    let index = index(json!([collection(
        "c",
        json!([{"id": "butterflies"}, {"id": "vulture"}])
    )]));
    let c = index.collection("c").unwrap();
    let requests = c.requests(&["vulture", "creature"]);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].id, "vulture");
}

#[test]
fn an_unsatisfiable_range_is_refused_with_a_reason() {
    let index = index(Value::Null);
    let plan = plan(
        &index,
        &[Request {
            id: "strict".into(),
            version: None,
        }],
    );
    let RequestResult::Refused { reason } = &plan.requested["strict"] else {
        panic!("{plan:?}");
    };
    assert!(reason.contains("creature"), "{reason}");
    assert!(plan.install.is_empty());
}
