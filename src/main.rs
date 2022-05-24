use std::process::exit;

use clap::{Parser, Subcommand};

use anc::{run, init};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Update Anki with files in current Anc directory
    Save { },
    Init { },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Save { } => {
            run();
        },
        Commands::Init { } => {
            if let Err(_) = init() {
                exit(5);
            }
        },
    }
}
