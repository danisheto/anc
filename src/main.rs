use std::process::exit;

use clap::{Parser, Subcommand};

use anc::{run, init, sync::sync};
use tokio::runtime::Runtime;

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
    r#Sync { },
    Init { },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Save { } => {
            run();
        },
        Commands::Sync { } => {
            let result = sync();
            let runtime = Runtime::new().unwrap();
            runtime.block_on(result);
        }
        Commands::Init { } => {
            if let Err(_) = init() {
                exit(5);
            }
        },
    }
}
