/// Fail before capability-dependent fixtures silently choose the legacy launch path.
pub(crate) fn assert_holder_capability() {
    sunrise_launcher_smod::holder::retain_capability();
    assert!(
        sunrise_launcher_smod::holder::capable(&std::env::current_exe().unwrap()).unwrap(),
        "capability marker missing from this binary (release optimisation?)"
    );
}

/// Writes a zip at `path` holding `files`, each a name and its bytes, making its folder first.
pub(crate) fn zip_file(path: &std::path::Path, files: &[(&str, &[u8])]) {
    use std::io::Write;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn current_binary_retains_holder_capability() {
    assert_holder_capability();
}
