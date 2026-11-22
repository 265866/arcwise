//! Checks a real library on disk: that it loads and that every lineup's clip is there.
//!
//! Run it with `cargo run -p arcwise_core --example check_library -- <library folder>`.

use std::fmt::Arguments;
use std::path::PathBuf;
use std::process::ExitCode;
use std::{env, io};

use arcwise_core::Library;

fn main() -> io::Result<ExitCode> {
    let Some(root) = env::args_os().nth(1).map(PathBuf::from) else {
        report(format_args!("usage: check_library <library folder>"))?;
        return Ok(ExitCode::from(2));
    };
    let library = match Library::open(&root) {
        Ok(library) => library,
        Err(e) => {
            report(format_args!("{}: {e}", root.display()))?;
            return Ok(ExitCode::FAILURE);
        }
    };
    if library.lineups().is_empty() {
        report(format_args!("{} has no lineups", root.display()))?;
        return Ok(ExitCode::FAILURE);
    }
    let mut missing = 0_usize;
    for lineup in library.lineups() {
        let clip = library.clip(lineup.id);
        if !clip.is_file() {
            report(format_args!(
                "lineup {}: no clip at {}",
                lineup.id,
                clip.display()
            ))?;
            missing += 1;
        }
    }
    report(format_args!(
        "{} lineups, {missing} without a clip",
        library.lineups().len()
    ))?;
    Ok(if missing == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Writes one line to standard error.
fn report(line: Arguments<'_>) -> io::Result<()> {
    use std::io::Write as _;

    writeln!(io::stderr().lock(), "{line}")
}
