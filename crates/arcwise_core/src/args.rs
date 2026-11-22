//! The command line: `arcwise [--library DIR] [--map NAME] [--agent NAME] [--side attack|defense]
//! [--lineup ID] [--view clip|stand|aim|lands] [--resume]`.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::browse::View;
use crate::model::{LineupId, Side};

/// What the command line asks for. Every flag is optional.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    /// A library folder to show instead of the ones in the settings.
    pub library: Option<PathBuf>,
    /// The map to show, by name.
    pub map: Option<String>,
    /// The agent to show, by name.
    pub agent: Option<String>,
    /// The side to show.
    pub side: Option<Side>,
    /// The lineup to select.
    pub lineup: Option<LineupId>,
    /// The view of the selected lineup.
    pub view: Option<View>,
    /// Open where Arcwise was, even with opening where it was left turned off: Arcwise passes it
    /// to itself when it reopens to apply a setting or an update.
    pub resume: bool,
}

impl Args {
    /// Reads the arguments after the program's name.
    ///
    /// # Errors
    ///
    /// A message for the user when a flag is unknown, lacks its value, or has a value it doesn't
    /// take.
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, String> {
        let mut parsed = Args::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let flag = arg.to_string_lossy().into_owned();
            let mut value = || args.next().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--library" => parsed.library = Some(value()?.into()),
                "--resume" => parsed.resume = true,
                "--map" => parsed.map = Some(text(&value()?)),
                "--agent" => parsed.agent = Some(text(&value()?)),
                "--side" => {
                    parsed.side = Some(match text(&value()?).as_str() {
                        "attack" => Side::Attack,
                        "defense" => Side::Defense,
                        other => return Err(format!("--side is attack or defense, not {other}")),
                    });
                }
                "--lineup" => {
                    let id = text(&value()?);
                    let id: u32 = id
                        .parse()
                        .map_err(|_| format!("--lineup takes a lineup number, not {id}"))?;
                    parsed.lineup = Some(LineupId(id));
                }
                "--view" => {
                    parsed.view = Some(match text(&value()?).as_str() {
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

fn text(value: &OsStr) -> String {
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
            "--resume",
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
                resume: true,
            },
            "every flag"
        );
    }

    #[test]
    fn nothing_is_required() {
        assert_eq!(parse(&[]).unwrap(), Args::default(), "no flags");
    }

    #[test]
    fn explains_bad_input() {
        assert_eq!(
            parse(&["--map"]).unwrap_err(),
            "--map needs a value",
            "a missing value"
        );
        assert_eq!(
            parse(&["--side", "mid"]).unwrap_err(),
            "--side is attack or defense, not mid",
            "a side that doesn't exist"
        );
        assert_eq!(
            parse(&["--lineup", "a"]).unwrap_err(),
            "--lineup takes a lineup number, not a",
            "a lineup that isn't a number"
        );
        assert_eq!(
            parse(&["-v"]).unwrap_err(),
            "unknown argument -v",
            "an unknown flag"
        );
    }
}
