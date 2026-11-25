use super::data::target_label;

#[test]
fn a_target_label_names_the_build_only_when_there_is_one() {
    assert_eq!(target_label("Default", None), "Default");
    assert!(target_label("Default", Some("1.21.5")).contains("1.21.5"));
}
