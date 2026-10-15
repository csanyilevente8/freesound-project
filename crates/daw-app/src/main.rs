//! `daw` binary entry point.
//!
//! Two front ends share this binary while the GUI (ADR-0002) is still pending:
//!
//! * **Subcommand CLI** (preferred for manual sessions): `daw <command> ...`
//!   operates on a project JSON file — `new`, `import`, `add-track`,
//!   `add-clip`, `add-effect`, `automate`, `set-volume`, `show`, `export`.
//!   Run `daw help` for the full list.
//! * **Legacy demo flags** (kept for quick smoke tests):
//!   `daw --save <path.json>` and `daw --export <path.wav>`.
//!
//! With no arguments it prints a new empty project summary and the help text.

use std::process::ExitCode;

use daw_app::AppState;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Legacy demo flags (checked first so existing scripts keep working).
    if let Some(pos) = args.iter().position(|a| a == "--export") {
        return legacy_export(args.get(pos + 1));
    }
    if args.iter().any(|a| a == "--save") {
        return legacy_new_and_save(&args);
    }

    // No args: show an empty project + help.
    if args.is_empty() {
        let app = AppState::with_new_project("Untitled");
        print!("{}", app.describe());
        println!("\n{}", daw_app::cli::run(&["help".to_string()]).unwrap());
        return ExitCode::SUCCESS;
    }

    // Subcommand CLI.
    match daw_app::cli::run(&args) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn legacy_export(path: Option<&String>) -> ExitCode {
    let Some(path) = path else {
        eprintln!("error: --export requires a path argument");
        return ExitCode::FAILURE;
    };
    let (app, source) = AppState::demo_project("Demo Tone");
    print!("{}", app.describe());
    match daw_render::export_wav(
        &app.project,
        &source,
        path,
        &daw_render::ExportOptions::default(),
    ) {
        Ok(()) => {
            println!("\nExported demo project to {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: export failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn legacy_new_and_save(args: &[String]) -> ExitCode {
    let app = AppState::with_new_project("Untitled");
    print!("{}", app.describe());
    let pos = args.iter().position(|a| a == "--save").unwrap();
    let Some(path) = args.get(pos + 1) else {
        eprintln!("error: --save requires a path argument");
        return ExitCode::FAILURE;
    };
    if let Err(e) = daw_project::save(&app.project, path) {
        eprintln!("error: failed to save project: {e}");
        return ExitCode::FAILURE;
    }
    println!("\nSaved project to {path}");
    match daw_project::load(path) {
        Ok(reloaded) if reloaded == app.project => {
            println!("Reloaded and verified round-trip OK.");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!("error: reloaded project differs from original");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: failed to reload project: {e}");
            ExitCode::FAILURE
        }
    }
}
