//! The command line: `arcwise [--library DIR] [--map NAME] [--agent NAME] [--side attack|defense]
//! [--lineup ID] [--view clip|stand|aim|lands]`.

use std::ffi::OsString;
use std::path::PathBuf;

use arcwise_core::{LineupId, Side, View};

#[derive(Debug, Default, PartialEq)]
pub struct Args {
    pub library: Option<PathBuf>,
    pub map: Option<String>,
    pub agent: Option<String>,
    pub side: Option<Side>,
    pub lineup: Option<LineupId>,
    pub view: Option<View>,
}

impl Args {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
        let mut parsed = Args::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let flag = arg.to_string_lossy().into_owned();
            let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--library" => parsed.library = Some(value()?.into()),
                "--map" => parsed.map = Some(text(value()?)),
                "--agent" => parsed.agent = Some(text(value()?)),
                "--side" => {
                    parsed.side = Some(match text(value()?).as_str() {
                        "attack" => Side::Attack,
                        "defense" => Side::Defense,
                        other => return Err(format!("--side is attack or defense, not {other}")),
                    });
                }
                "--lineup" => {
                    let id = text(value()?);
                    let id = id
                        .parse()
                        .map_err(|_| format!("--lineup takes a lineup number, not {id}"))?;
                    parsed.lineup = Some(LineupId(id));
                }
                "--view" => {
                    parsed.view = Some(match text(value()?).as_str() {
                        "clip" => View::Clip,
                        "stand" => View::Stand,
                        "aim" => View::Aim,
                        "lands" => View::Lands,
                        other => {
                            return Err(format!(
                                "--view is clip, stand, aim or lands, not {other}"
                            ));
                        }
                    });
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(parsed)
    }
}

fn text(value: OsString) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_every_flag() {
        let args = parse(&[
            "--library",
            r"D:\lineups",
            "--map",
            "Ascent",
            "--agent",
            "Sova",
            "--side",
            "defense",
            "--lineup",
            "211",
            "--view",
            "aim",
        ])
        .unwrap();
        assert_eq!(
            args,
            Args {
                library: Some(PathBuf::from(r"D:\lineups")),
                map: Some("Ascent".into()),
                agent: Some("Sova".into()),
                side: Some(Side::Defense),
                lineup: Some(LineupId(211)),
                view: Some(View::Aim),
            }
        );
    }

    #[test]
    fn nothing_is_required() {
        assert_eq!(parse(&[]).unwrap(), Args::default());
    }

    #[test]
    fn explains_bad_input() {
        assert_eq!(parse(&["--map"]).unwrap_err(), "--map needs a value");
        assert_eq!(
            parse(&["--side", "mid"]).unwrap_err(),
            "--side is attack or defense, not mid"
        );
        assert_eq!(
            parse(&["--lineup", "a"]).unwrap_err(),
            "--lineup takes a lineup number, not a"
        );
        assert_eq!(parse(&["-v"]).unwrap_err(), "unknown argument -v");
    }
}
