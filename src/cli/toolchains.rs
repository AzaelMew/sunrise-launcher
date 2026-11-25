//! `java …` and `rust …`: the Java runtimes, and the Rust toolchain native builds use.
use super::*;

#[derive(Subcommand)]
pub(super) enum JavaCmd {
    /// The Java runtimes Sunrise downloaded from Mojang and the ones installed on this computer
    List,
    /// Download Mojang's Java runtime: a major (8, 16, 17, 21, 25) or a component name from a
    /// version's JSON (java-runtime-delta)
    Install { java: String },
}

#[derive(Subcommand)]
pub(super) enum RustCmd {
    /// Whether it is installed, its version and whether this computer can link with it
    Status,
    /// Download the pinned toolchain from static.rust-lang.org (once), checked by SHA-256
    Install,
    /// Delete the toolchain and the native builds' Cargo home
    Remove,
}

pub(super) fn java(action: JavaCmd, as_json: bool) -> Result<(), String> {
    use sunrise_launcher::java;
    match action {
        JavaCmd::List => {
            let mut all = java::mojang_runtimes(&java::runtimes_dir());
            all.extend(java::installed());
            if as_json {
                json(serde_json::json!(all));
            } else if all.is_empty() {
                println!("No Java yet. Sunrise downloads Mojang's when a profile needs it.");
            } else {
                for r in &all {
                    let from = match r.source {
                        java::Source::Mojang => "Mojang's, downloaded by Sunrise",
                        java::Source::Installed => "installed",
                    };
                    println!("Java {}  ({from})\n  {}", r.version, r.home.display());
                }
            }
        }
        JavaCmd::Install { java: which } => {
            let need = java::Need::named(&which)?;
            let runtime = java::install(&need, &|line| eprintln!("{line}"))?;
            if as_json {
                json(serde_json::json!(runtime));
            } else {
                println!(
                    "Java {} is ready: {}",
                    runtime.version,
                    runtime.home.display()
                );
            }
        }
    }
    Ok(())
}

pub(super) fn rust(action: RustCmd, as_json: bool) -> Result<(), String> {
    use sunrise_launcher::rust_toolchain as rust;
    match action {
        RustCmd::Status => {
            let installed = rust::installed();
            let version = installed.as_ref().map(|t| t.version()).transpose()?;
            let linker = rust::linker().err();
            if as_json {
                json(serde_json::json!({
                    "pinned": rust::VERSION,
                    "host": rust::host().map(|h| h.target),
                    "installed": installed.as_ref().map(|t| t.dir.display().to_string()),
                    "version": version,
                    "linker": linker,
                }));
            } else {
                match (&installed, &version) {
                    (Some(t), Some(v)) => println!("{v} in {}", t.dir.display()),
                    _ => println!(
                        "Rust {} is not installed (`sunrise-launcher rust install`).",
                        rust::VERSION
                    ),
                }
                println!(
                    "{}",
                    linker.unwrap_or_else(|| "This computer can link the game.".into())
                );
            }
        }
        RustCmd::Install => {
            let shown = std::cell::Cell::new(0);
            let toolchain = rust::install(|f| {
                let percent = (f * 100.0) as u32;
                if percent >= shown.get() + 10 {
                    shown.set(percent);
                    eprintln!("{percent}%");
                }
            })?;
            println!("{} in {}", toolchain.version()?, toolchain.dir.display());
        }
        RustCmd::Remove => {
            rust::remove()?;
            println!("Removed the launcher's Rust toolchain and Cargo home.");
        }
    }
    Ok(())
}
