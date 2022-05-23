use std::env;

use clap::{Parser, Subcommand};

use anc::run;

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
}

fn main() {
    let cli = Cli::parse();

    let path = env::var("TEST_ANKI").expect("For testing, need a $TEST_ANKI");
    match &cli.command {
        Commands::Save { } => {
            run(".", path);
        },
    }
}
