//! Finding a JDK or a Java runtime of a major version, as sunrise-prepare finds the one its exporters run on (a copy of its `java.rs` discovery).

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Jdk {
    pub javac: PathBuf,
    pub java: PathBuf,
}

impl Jdk {
    /// The JDK's home, the folder above `bin` (what `JAVA_HOME` names).
    pub fn home(&self) -> PathBuf {
        self.java
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }
}

/// The variables a JVM reads extra options from; a probe ignores them (a player's global collector choice, #10067).
const JVM_OPTION_VARS: [&str; 3] = ["JAVA_TOOL_OPTIONS", "_JAVA_OPTIONS", "JDK_JAVA_OPTIONS"];

/// A JDK probe: no terminal on Windows, and none of the player's environment options for Java.
pub(crate) fn command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for var in JVM_OPTION_VARS {
        command.env_remove(var);
    }
    #[cfg(windows)]
    let command = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = command;
        command.creation_flags(CREATE_NO_WINDOW);
        command
    };
    command
}

fn executable(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    })
}

/// `fs::canonicalize`, without the `\\?\` prefix it gives a Windows drive path: the
/// home goes into `JAVA_HOME` and the PATH of preparers, whose lookups don't take it.
fn real_path(path: PathBuf) -> PathBuf {
    let real = fs::canonicalize(&path).unwrap_or(path);
    if cfg!(windows) {
        without_verbatim(real)
    } else {
        real
    }
}

fn without_verbatim(path: PathBuf) -> PathBuf {
    match path.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path,
    }
}

/// Resolve compiler symlinks before picking its sibling runtime. The first `java` on the
/// PATH may be an older, runtime-only install; it must not be paired with a newer `javac`.
fn path_homes(path: &OsStr, tool: &str) -> Vec<PathBuf> {
    std::env::split_paths(path)
        .map(|dir| executable(&dir, tool))
        .filter(|javac| javac.is_file())
        .map(real_path)
        .filter_map(|javac| {
            let bin = javac.parent()?;
            // macOS's system tools are dispatchers, not a JDK home: JAVA_HOME=/usr
            // makes them call themselves. /usr/libexec/java_home finds the actual JDK.
            if bin.file_name()? != "bin"
                || (cfg!(target_os = "macos") && bin == Path::new("/usr/bin"))
            {
                return None;
            }
            Some(bin.parent()?.to_path_buf())
        })
        .collect()
}

/// The major version `javac -version` or `java -version` reports, including Java 8's `1.8`.
fn tool_major(program: &Path, name: &str) -> Result<u32, String> {
    let output = command(program)
        .arg("-version")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    // macOS ships /usr/bin/java and /usr/bin/javac as placeholders.
    if text.contains("Unable to locate a Java Runtime") {
        return Err(format!(
            "{} is macOS's placeholder (no Java installed)",
            program.display()
        ));
    }
    if !output.status.success() {
        return Err(format!(
            "{} -version failed ({}): {}",
            program.display(),
            output.status,
            text.trim()
        ));
    }
    let marker = if name == "javac" { "javac" } else { "version" };
    let version = text
        .split_whitespace()
        .skip_while(|w| *w != marker)
        .nth(1)
        .ok_or_else(|| {
            format!(
                "{} -version printed no version: {}",
                program.display(),
                text.trim()
            )
        })?;
    let mut parts = version.trim_matches('"').split(['.', '-', '+', '_']);
    let first: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    Ok(if first == 1 {
        parts.next().and_then(|p| p.parse().ok()).unwrap_or(0)
    } else {
        first
    })
}

/// The folders JDK installers and tools put JDKs in, each JDK one folder below (#666: a
/// player installed Java 25 but JAVA_HOME and the PATH still named an older one).
fn jdk_parents() -> Vec<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = var(if cfg!(windows) { "USERPROFILE" } else { "HOME" });
    let mut parents = Vec::new();
    if cfg!(windows) {
        let vendors = [
            "Eclipse Adoptium",
            "Java",
            "Microsoft",
            "Zulu",
            "BellSoft",
            "Amazon Corretto",
            "Semeru",
            "OpenJDK",
        ];
        for base in ["ProgramFiles", "ProgramW6432"].into_iter().filter_map(var) {
            parents.extend(vendors.iter().map(|v| base.join(v)));
        }
    } else if cfg!(target_os = "macos") {
        parents.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        parents.extend(
            home.iter()
                .map(|h| h.join("Library/Java/JavaVirtualMachines")),
        );
    } else {
        parents.extend(["/usr/lib/jvm", "/usr/java", "/usr/local/lib/jvm"].map(PathBuf::from));
    }
    parents.extend(
        home.iter()
            .flat_map(|h| [h.join(".jdks"), h.join(".sdkman/candidates/java")]),
    );
    parents
}

/// The JDKs (folders with `bin/javac`) one level below `parents`; a macOS bundle's home is
/// its `Contents/Home`. Those named for `major` come first, then the others newest name first.
fn installed_jdks(parents: &[PathBuf], major: i64, tool: &str) -> Vec<PathBuf> {
    let mut homes: Vec<PathBuf> = parents
        .iter()
        .filter_map(|parent| fs::read_dir(parent).ok())
        .flatten()
        .flatten()
        .map(|entry| {
            let bundle = entry.path().join("Contents/Home");
            if bundle.is_dir() {
                bundle
            } else {
                entry.path()
            }
        })
        .filter(|home| executable(&home.join("bin"), tool).is_file())
        .collect();
    let named = |home: &Path| {
        home.iter().any(|part| {
            part.to_string_lossy()
                .split(|c: char| !c.is_ascii_digit())
                .any(|n| n == major.to_string())
        })
    };
    homes.sort_by(|a, b| named(b).cmp(&named(a)).then_with(|| b.cmp(a)));
    homes.dedup();
    homes
}

/// Finds a JDK whose compiler and runtime are both at least `major`, the version
/// research-lock.json pins: `JAVA_HOME`, else JDKs on the PATH, else, on macOS, the JDK
/// `/usr/libexec/java_home -v <major>` reports, else the homes in `more` (the launcher's pick,
/// Minecraft's own runtimes), else the folders installers use (`jdk_parents`).
pub fn find(major: i64, more: &[PathBuf]) -> Result<Jdk, String> {
    find_tools(major, more, true)
}

/// Finds a compatible runtime; precompiled exporters do not need javac.
pub fn find_runtime(major: i64, more: &[PathBuf]) -> Result<Jdk, String> {
    find_tools(major, more, false)
}

fn find_tools(major: i64, more: &[PathBuf], compiler: bool) -> Result<Jdk, String> {
    let tool = if compiler { "javac" } else { "java" };
    let need = format!(
        "Java {major} is needed ({tool}, e.g. Eclipse Temurin {major}). \
         Install it, or set JAVA_HOME to it, and try again."
    );
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("JAVA_HOME").filter(|h| !h.is_empty()) {
        let bin = Path::new(&home).join("bin");
        candidates.push((
            executable(&bin, "javac"),
            executable(&bin, "java"),
            "JAVA_HOME",
        ));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for home in path_homes(&path, tool) {
            let bin = home.join("bin");
            candidates.push((executable(&bin, "javac"), executable(&bin, "java"), "PATH"));
        }
    }
    if cfg!(target_os = "macos") {
        if let Ok(out) = command("/usr/libexec/java_home")
            .args(["-v", &major.to_string()])
            .stderr(Stdio::null())
            .output()
        {
            let home = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !home.is_empty() {
                let bin = Path::new(&home).join("bin");
                candidates.push((
                    executable(&bin, "javac"),
                    executable(&bin, "java"),
                    "/usr/libexec/java_home",
                ));
            }
        }
    }
    for home in more {
        let bin = home.join("bin");
        candidates.push((
            executable(&bin, "javac"),
            executable(&bin, "java"),
            "Minecraft's Java",
        ));
    }
    for home in installed_jdks(&jdk_parents(), major, tool) {
        let bin = home.join("bin");
        candidates.push((
            executable(&bin, "javac"),
            executable(&bin, "java"),
            "an installed JDK",
        ));
    }
    let mut problems = Vec::new();
    for (javac, java, from) in candidates {
        if (compiler && !javac.is_file()) || !java.is_file() {
            problems.push(format!(
                "{from} ({}) has no compatible Java tools",
                javac.parent().unwrap().display()
            ));
            continue;
        }
        let checked = [(&javac, "javac"), (&java, "java")]
            .into_iter()
            .filter(|(_, name)| compiler || *name == "java")
            .try_for_each(|(program, name)| {
                let found = tool_major(program, name)?;
                if i64::from(found) >= major {
                    Ok(())
                } else {
                    Err(format!("{name} {found} ({})", program.display()))
                }
            });
        match checked {
            // Absolute: the exporters run in another working directory, where a relative
            // `JAVA_HOME`, `--java-home` or `PATH` entry would name nothing.
            Ok(()) => {
                return Ok(Jdk {
                    javac: std::path::absolute(&javac).unwrap_or(javac),
                    java: std::path::absolute(&java).unwrap_or(java),
                });
            }
            Err(e) => problems.push(format!("{from}: {e}")),
        }
    }
    if problems.is_empty() {
        Err(format!("{need} No {tool} was found."))
    } else {
        Err(format!("{need} Found: {}.", problems.join("; ")))
    }
}
