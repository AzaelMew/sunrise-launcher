//! Embeds Sunrise's icon (`assets/icons/sunrise.ico`) in the Windows executables, as icon
//! resource 1: Explorer and the task bar show it, and the window takes it as its icon.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Resolve imports before main from System32, never a nearby injector DLL (#13108).
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
        // Unoptimised builds overflow Windows' default 1 MiB main-thread stack while the window
        // builds its first frame (0xC00000FD at start); release builds keep the default.
        if std::env::var("PROFILE").as_deref() == Ok("debug") {
            println!("cargo:rustc-link-arg-bins=/STACK:16777216");
        }
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    // Microsoft sign-in in a local build: the fork's own Azure client ID from an untracked
    // `launcher/.msa-client-id` (never committed), when the variable isn't set. Release builds
    // set SUNRISE_MSA_CLIENT_ID from the repository variable instead. Watched only once it exists
    // (a missing watched file would rerun this script on every build): after adding it, change
    // build.rs or clean once.
    let id_file = format!("{dir}/.msa-client-id");
    if std::path::Path::new(&id_file).is_file() {
        println!("cargo:rerun-if-changed={id_file}");
    }
    println!("cargo:rerun-if-env-changed=SUNRISE_MSA_CLIENT_ID");
    if std::env::var_os("SUNRISE_MSA_CLIENT_ID").is_none() {
        if let Ok(id) = std::fs::read_to_string(&id_file) {
            let id = id.trim();
            if !id.is_empty() {
                println!("cargo:rustc-env=SUNRISE_MSA_CLIENT_ID={id}");
            }
        }
    }
    let icon = "sunrise.ico";
    let ico = format!("{dir}/assets/icons/{icon}");
    println!("cargo:rerun-if-changed={ico}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(&ico);
        if repository == "sunrise-org/sunrise" {
            resource.set("FileDescription", "Sunrise Launcher");
            resource.set("ProductName", "Sunrise Launcher");
            resource.set("InternalName", "sunrise-launcher");
            resource.set("OriginalFilename", "sunrise-launcher.exe");
        }
        if let Err(e) = resource.compile() {
            panic!("embedding {ico}: {e}");
        }
    }
}
