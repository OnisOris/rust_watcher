use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "watcher", version, about = "Semantic Rust codebase explorer")]
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
