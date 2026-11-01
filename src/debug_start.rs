//! Starting the lap part way round, to look at the scenery there without
//! riding to it: the game started with `--from 3700` (or the environment's
//! `MOTORUST_FROM=3700`) puts the whole grid 3700 m along the lap instead
//! of behind the finish line, in whatever track and mode are chosen. The
//! formation is the one it always is: the pole at the distance asked, the
//! rows behind it.
//!
//! Only for development. The lap and the times are counted from the finish
//! line as ever, so the first lap is short and its time means nothing; the
//! benchmark ignores this altogether.

use std::sync::OnceLock;

/// The flag, and the environment's name for it.
const FLAG: &str = "--from";
const VAR: &str = "MOTORUST_FROM";

/// How far along the lap the grid stands (m), if the game was asked for
/// it: read once, from the command line or the environment.
pub fn along() -> Option<f32> {
    static ALONG: OnceLock<Option<f32>> = OnceLock::new();
    *ALONG.get_or_init(|| asked(std::env::args(), std::env::var(VAR).ok()))
}

/// What was asked for: `--from <metres>` or `--from=<metres>` among the
/// arguments, or else the environment's. Nothing, where it is missing or
/// is not a distance.
fn asked(args: impl Iterator<Item = String>, var: Option<String>) -> Option<f32> {
    let mut args = args.skip_while(|arg| !arg.starts_with(FLAG));
    let from_args = args.next().and_then(|arg| match arg.strip_prefix(FLAG) {
        Some("") => args.next(),
        Some(rest) => rest.strip_prefix('=').map(str::to_owned),
        None => None,
    });
    from_args.or(var).as_deref().and_then(metres)
}

/// A distance along the lap, as written: a finite number of metres, none
/// of them negative.
fn metres(said: &str) -> Option<f32> {
    said.trim()
        .parse::<f32>()
        .ok()
        .filter(|metres| metres.is_finite() && *metres >= 0.0)
}

#[cfg(any(test, feature = "testing"))]
mod tests {
    use super::*;

    fn args(said: &[&str]) -> impl Iterator<Item = String> + use<> {
        let said: Vec<String> = std::iter::once("motorust".to_owned())
            .chain(said.iter().map(|arg| (*arg).to_owned()))
            .collect();
        said.into_iter()
    }

    #[test]
    fn the_distance_is_read_from_the_command_line_or_the_environment() {
        assert_eq!(asked(args(&["--from", "3700"]), None), Some(3700.0));
        assert_eq!(asked(args(&["--from=3700.5"]), None), Some(3700.5));
        assert_eq!(asked(args(&["--bench", "--from", "0"]), None), Some(0.0));
        // The command line first, then the environment.
        let var = Some("120".to_owned());
        assert_eq!(asked(args(&["--from", "3700"]), var.clone()), Some(3700.0));
        assert_eq!(asked(args(&[]), var), Some(120.0));
    }

    #[test]
    fn nothing_is_asked_without_a_distance() {
        for said in [vec![], vec!["--bench"], vec!["--from"], vec!["--from="]] {
            assert_eq!(asked(args(&said), None), None, "{said:?}");
        }
        for said in ["", " ", "far", "-20", "nan", "inf"] {
            assert_eq!(asked(args(&["--from", said]), None), None, "{said:?}");
            assert_eq!(metres(said), None, "{said:?}");
        }
        assert_eq!(metres(" 2600 "), Some(2600.0));
    }
}
