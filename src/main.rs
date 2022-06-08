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

    let output = match &cli.command {
        Commands::Save { } => {
            run()
                .map(|successes| {
                    let added_length = successes.iter()
                        .map(|(_, added, _)| added)
                        .max()
                        .map(|m| m.to_string().len());
                    let updated_length = successes.iter()
                        .map(|(_, _, updated)| updated)
                        .max()
                        .map(|m| m.to_string().len());
                    let output: Vec<String> = successes.into_iter()
                        .filter(|(_, added, updated)| *added != 0 || *updated != 0)
                        .map(|(name, added, updated)| format!(
                                "{added:apad$} added and {updated:upad$} updated to {name}",
                                added=added,
                                updated=updated,
                                apad=added_length.unwrap(),
                                upad=updated_length.unwrap()
                        ))
                        .collect();
                    if output.is_empty() {
                        vec!["Nothing was added or updated".to_string()]
                    } else {
                        output
                    }
                })
        },
        Commands::Sync { } => {
            let result = sync();
            let runtime = Runtime::new().unwrap();
            runtime.block_on(result)
        }
        Commands::Init { } => {
            init()
        },
    };
    match output {
        Err(e) => {
            eprintln!("{}", e.concat());
            exit(1);
        },
        Ok(successes) => {
            eprintln!("{}", successes.concat());
        }
    }
}
