mod macos;
mod pkl_types;

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Parser)]
#[command(version, author, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Diff system configuration and current system state
    Diff {
        #[command(subcommand)]
        command: DiffCommands,
    },

    /// Generate shell completions
    Completions {
        /// The shell to generate the completions for
        #[arg(value_enum)]
        shell: clap_complete_command::Shell,
    },
}

#[derive(Subcommand)]
enum DiffCommands {
    /// MacOS
    #[command(name = "macos")]
    MacOs {
        /// System configuration file to compare against
        system_config: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Diff { command } => match command {
            DiffCommands::MacOs { system_config } => {
                let config = fs::read_to_string(system_config)?;
                let config = serde_json::from_str::<pkl_types::macos::MacOsConfig>(&config)
                    .context("Failed to read system config")?;
                let system_has_homebrew = macos::homebrew_is_installed();
            }
        },
        Commands::Completions { shell } => {
            shell.generate(&mut Cli::command(), &mut std::io::stdout());
        }
    }
    Ok(())
}
