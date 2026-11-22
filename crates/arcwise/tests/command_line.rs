//! Tests for the command line of `arcwise`. Every command line but `--version` opens the window,
//! even one that can't be read, which the window then shows, so only `--version` runs here.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;
use std::{env, fs, process, thread};

/// How long `arcwise --version` may take. A build that started the app instead would wait for its
/// window to close; the test fails then rather than hang.
const PATIENCE: Duration = Duration::from_secs(30);

/// Runs the built `arcwise` with `arguments`, every folder it could read or write under `home`,
/// so a build that started the app by mistake touches nothing of the user's.
fn run(arguments: &[&str], home: &Path) -> Output {
    let binary = env!("CARGO_BIN_EXE_arcwise");
    let child = Command::new(binary)
        .args(arguments)
        .env("APPDATA", home.join("Roaming"))
        .env("LOCALAPPDATA", home.join("Local"))
        .env("USERPROFILE", home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary should start");
    let pid = child.id();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || done.send(child.wait_with_output()));
    if let Ok(output) = finished.recv_timeout(PATIENCE) {
        output.expect("the binary's output should be read")
    } else {
        let killed = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .output();
        panic!(
            "`arcwise {arguments:?}` was still running after {PATIENCE:?}, so it was killed: {killed:?}"
        );
    }
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
    let settings = appdata.join("Roaming").join("Arcwise");
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
