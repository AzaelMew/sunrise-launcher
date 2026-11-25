//! `completions`, `links` and `benchmark`: the shell's completions, the project's links, and timing a start.
use super::*;

pub(super) fn completions(shell: clap_complete::Shell) {
    clap_complete::generate(
        shell,
        &mut super::command(),
        "sunrise-launcher",
        &mut std::io::stdout(),
    );
}

pub(super) fn benchmark(repeats: usize, as_json: bool) -> Result<(), Error> {
    let path = crate::perf::benchmark(repeats.max(1))?;
    if as_json {
        json(serde_json::json!({ "report": path }));
    } else {
        println!("{}", path.display());
    }
    Ok(())
}

pub(super) fn links(as_json: bool) {
    let links = backend::links();
    if as_json {
        json(serde_json::to_value(&links).unwrap());
    } else {
        for (name, url) in [
            ("Website", &links.website),
            ("Roadmap", &links.roadmap),
            ("Documentation", &links.docs),
            ("Discord", &links.discord),
        ] {
            println!("{name:<8} {}", if url.is_empty() { "(none)" } else { url });
        }
    }
}
