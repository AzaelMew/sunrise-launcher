use super::*;

#[test]
fn a_data_folder_problem_opens_settings_on_storage() {
    assert!(startup_section(true, false) == Some(Section::Storage));
    assert!(startup_section(false, true) == Some(Section::Storage));
    assert!(startup_section(false, false).is_none());
}

#[test]
fn a_text_box_shows_as_much_as_its_pane_is_wide() {
    assert_eq!(field_chars(1280.), 91);
    assert_eq!(field_chars(900.), 44);
    assert_eq!(field_chars(300.), 21);
}

#[test]
fn a_long_proxy_shows_whole_in_a_wide_window_and_the_settings_view_sizes_its_box() {
    let line = "http://user:password@proxy.example.org:7890/".repeat(2);
    let mut input = line_edit::LineEdit::new(line.clone(), 256, 44);
    fit_field(&mut input, 1280.);
    assert_eq!(input.text(), line);
    assert!(line.chars().count() <= 91);
    assert_eq!(input.shown(), line);
    // The view sizes the proxy box through `fit_field`; a Launcher needs a window to build.
    let source = include_str!("launcher.rs");
    assert!(source.contains("fit_field(&mut self.proxy_input, self.window_width);"));
}

#[test]
fn the_launcher_row_says_what_its_check_found() {
    let now = Some(Instant::now());
    // A failure is shown as one and is retryable (the row keeps its button); it is never
    // passed off as up to date, even right after a check.
    let raw = "No connection: https://api.github.com/repos/sunrise-org/sunrise-launcher/releases?per_page=100 Dns Failed: resolve dns name 'api.github.com:443', and Sunrise's mirror of GitHub couldn't help either (http://127.0.0.1:40319/repos/x answered 500)";
    let (line, bad) = launcher_check_line(Some(raw), now, true);
    assert!(bad);
    for leaked in ["http", "127.0.0.1", "40319", "api.github.com", "500", "Dns"] {
        assert!(!line.contains(leaked), "{leaked} in {line}");
    }
    assert_eq!(
        line,
        "Couldn't check for updates. Check your connection and try again."
    );
    // The detail goes to the log instead.
    let mut logged = String::new();
    log_launcher_check_failure(raw, |l| logged = l.to_string());
    assert!(
        logged.contains(raw) && logged.contains("127.0.0.1:40319"),
        "{logged}"
    );
    assert_eq!(
        launcher_check_line(None, now, true),
        ("up to date · checked just now".into(), false)
    );
    let long_ago = Instant::now().checked_sub(Duration::from_secs(120));
    assert_eq!(
        launcher_check_line(None, long_ago, true),
        ("up to date".into(), false)
    );
    assert_eq!(
        launcher_check_line(None, None, false),
        (String::new(), false)
    );
}
