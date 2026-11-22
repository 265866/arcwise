//! Tests for the command line of `arcwise`. Every command line but `--version` opens the window,
//! even one that can't be read, which the window then shows, so only `--version` runs here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::{env, fs, process};

/// Runs the built `arcwise` with `arguments`, its settings folder under `appdata`.
fn run(arguments: &[&str], appdata: &Path) -> Output {
    let binary = env!("CARGO_BIN_EXE_arcwise");
    Command::new(binary)
        .args(arguments)
        .env("APPDATA", appdata)
        .output()
        .expect("the binary should start")
}

/// A fresh folder for one test, named for it and for this run.
fn scratch(name: &str) -> PathBuf {
    let dir = env::temp_dir().join(format!("arcwise-cli-{name}-{}", process::id()));
    fs::create_dir_all(&dir).expect("the scratch folder should be created");
    dir
}

#[test]
fn version_prints_the_version_line_and_succeeds() {
    let appdata = scratch("version");
    let output = run(&["--version"], &appdata);
    assert!(
        output.status.success(),
        "`--version` should exit 0: {output:?}"
    );
    let name = "arcwise";
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("{name} {}\n", env!("CARGO_PKG_VERSION")),
        "`--version` should print the name and the version"
    );
    fs::remove_dir_all(&appdata).expect("the scratch folder should be removed");
}

#[test]
fn version_does_not_start_the_app() {
    let appdata = scratch("no-app");
    let settings = appdata.join("Arcwise");
    fs::create_dir_all(&settings).expect("the settings folder should be created");
    fs::write(settings.join("settings.json"), "not json").expect("the settings should be written");
    let output = run(&["--version"], &appdata);
    assert!(
        output.status.success(),
        "`--version` should exit 0: {output:?}"
    );
    assert!(
        output.stderr.is_empty(),
        "`--version` should not load the settings, which would report this broken file: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::remove_dir_all(&appdata).expect("the scratch folder should be removed");
}
