//! `arcwise`: A desktop lineup browser for Valorant.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::ffi::OsString;
use std::process::ExitCode;
use std::{env, io};

use arcwise_core::Args;

fn main() -> io::Result<ExitCode> {
    use std::io::Write as _;

    let arguments: Vec<OsString> = env::args_os().skip(1).collect();
    if let [flag] = arguments.as_slice()
        && flag == "--version"
    {
        let line = arcwise_core::version_line();
        writeln!(io::stdout().lock(), "{line}")?;
        return Ok(ExitCode::SUCCESS);
    }
    arcwise_ui::run(Args::parse(arguments));
    Ok(ExitCode::SUCCESS)
}
