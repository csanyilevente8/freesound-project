//! `daw-gui` — the graphical prototype entry point (ADR-0002).
//!
//! Built only with `--features gui`. Loads a project (from an argument or
//! starts empty) and opens the egui window.
//!
//!   cargo run -p daw-app --features gui --bin daw-gui -- [project.json]

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Optional first arg: a project file to open.
    let (project, path) = match args.first() {
        Some(p) => match daw_project::load(p) {
            Ok(proj) => (proj, Some(p.clone())),
            Err(e) => {
                eprintln!("error: failed to load {p}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => (
            daw_core::project::Project::new("Untitled", daw_core::time::SampleRate::default()),
            None,
        ),
    };

    match daw_app::gui::run(project, path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
