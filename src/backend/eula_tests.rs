use super::*;

fn agreed_with(name: &str, text: &str) -> bool {
    let dir = std::env::temp_dir().join(format!("sunrise-eula-{name}-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("eula.txt"), text).unwrap();
    let agreed = eula_agreed(&dir);
    fs::remove_dir_all(dir).unwrap();
    agreed
}

#[test]
fn eula_txt_is_read_as_a_properties_file() {
    assert!(agreed_with("exact", "# comment\neula=true\n"));
    assert!(agreed_with("spaced", "eula = true\n"));
    assert!(agreed_with("colon", "eula:true\n"));
    assert!(agreed_with("blank", "eula true\n"));
    assert!(agreed_with("upper", "eula=TRUE\r\n"));
    assert!(agreed_with("indented", "  eula=true\n"));
    assert!(agreed_with("last wins", "eula=false\neula=true\n"));
    // The value keeps its trailing blank, which is not "true" to the server.
    assert!(!agreed_with("trailing", "eula=true \n"));
    assert!(!agreed_with("false", "eula=false\n"));
    assert!(!agreed_with("last false", "eula=true\neula=false\n"));
    assert!(!agreed_with("commented", "#eula=true\n!eula=true\n"));
    assert!(!agreed_with("other key", "eulas=true\n"));
    assert!(agreed_with("lone cr", "eula=false\reula=true\r"));
    assert!(agreed_with("joined", "eula=tr\\\n    ue\n"));
    assert!(agreed_with("joined key", "eu\\\nla=true\n"));
    assert!(agreed_with("joined crlf", "eula=\\\r\n  true\r\n"));
    // An even run of backslashes is an escaped backslash, not a continuation.
    assert!(!agreed_with("escaped", "eula=true\\\\\nfalse\n"));
    assert!(!agreed_with("joined false", "eula=true\\\nx\n"));
    assert!(!agreed_with("empty", ""));
    assert!(!eula_agreed(Path::new("/no/such/sunrise-folder")));
}
