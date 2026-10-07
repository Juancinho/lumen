//! What a second `lumen.exe` launch asks the running instance to do (single instance).
//!
//! `lumen.exe` alone shows the overlay; `--hide`, `--toggle` and `--quit` exist for
//! automation (benchmarks, scripts) and future launchers/jump lists.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstanceCommand {
    Show,
    Hide,
    Toggle,
    Quit,
}

/// The first recognised flag wins; anything else (incl. none) means show.
pub(crate) fn parse(args: &[String]) -> InstanceCommand {
    args.iter()
        .skip(1) // executable path
        .find_map(|a| match a.as_str() {
            "--hide" => Some(InstanceCommand::Hide),
            "--toggle" => Some(InstanceCommand::Toggle),
            "--quit" => Some(InstanceCommand::Quit),
            "--show" => Some(InstanceCommand::Show),
            _ => None,
        })
        .unwrap_or(InstanceCommand::Show)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn default_is_show_and_flags_map() {
        assert_eq!(parse(&args(&["lumen.exe"])), InstanceCommand::Show);
        assert_eq!(
            parse(&args(&["lumen.exe", "--hide"])),
            InstanceCommand::Hide
        );
        assert_eq!(
            parse(&args(&["lumen.exe", "x", "--toggle"])),
            InstanceCommand::Toggle
        );
        assert_eq!(
            parse(&args(&["lumen.exe", "--quit", "--show"])),
            InstanceCommand::Quit
        );
        // The executable itself is never a flag, even if named like one.
        assert_eq!(parse(&args(&["--hide"])), InstanceCommand::Show);
    }
}
