use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "wt", version, about = "Interactive semantic Rust explorer")]
pub struct Cli {
    #[arg(long, global = true, help = "Emit stable machine-readable JSON")]
    pub json: bool,

    #[arg(
        value_name = "PATH",
        default_value = ".",
        help = "Path to a Cargo project"
    )]
    pub path: PathBuf,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Doctor,
    Summary,
    Symbol {
        name: String,
    },
    Refs {
        name: String,
    },
    Definition {
        name: String,
    },
    Calls {
        name: String,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=4))]
        depth: u8,
    },
    Callers {
        name: String,
        #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(1..=4))]
        depth: u8,
    },
    Diagnostics {
        #[arg(long, conflicts_with = "warnings")]
        errors: bool,
        #[arg(long, conflicts_with = "errors")]
        warnings: bool,
    },
    Explain {
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_selects_tui_and_summary_is_explicit() {
        let tui = Cli::try_parse_from(["wt"]).unwrap();
        assert!(tui.command.is_none());
        assert!(!tui.json);

        let summary = Cli::try_parse_from(["wt", "summary"]).unwrap();
        assert!(matches!(summary.command, Some(Command::Summary)));

        let json = Cli::try_parse_from(["wt", "--json"]).unwrap();
        assert!(json.command.is_none());
        assert!(json.json);
    }
}
