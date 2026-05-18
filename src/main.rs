mod differs;
mod fetchers;
mod file_checks;
mod pkl_types;

use std::{collections::HashSet, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::{CommandFactory, Parser, Subcommand};
use validator::Validate;

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

    /// Mirror configured sync file checks into a local directory of symlinks
    FileSync {
        #[command(subcommand)]
        command: FileSyncCommands,
    },

    /// Debug commands
    Debug {
        #[command(subcommand)]
        command: DebugCommands,
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
    /// macOS
    #[command(name = "macos")]
    MacOs {
        /// System configuration file to compare against
        system_config: PathBuf,

        /// Use custom implementation of `brew list`
        ///
        /// This should produce the same output and be faster but may be incorrect in some edge cases
        #[arg(long)]
        fast_brew: bool,
    },

    /// SteamOS
    #[command(name = "steamos")]
    SteamOs {
        /// System configuration file to compare against
        system_config: PathBuf,
    },
}

#[derive(Subcommand)]
enum FileSyncCommands {
    /// macOS
    #[command(name = "macos")]
    MacOs {
        /// Output directory that will contain the mirrored symlinks
        #[arg(short = 'o', long)]
        root: PathBuf,

        /// System configuration file to compare against
        system_config: PathBuf,
    },

    /// SteamOS
    #[command(name = "steamos")]
    SteamOs {
        /// Output directory that will contain the mirrored symlinks
        #[arg(short = 'o', long)]
        root: PathBuf,

        /// System configuration file to compare against
        system_config: PathBuf,
    },
}

#[derive(Subcommand)]
enum DebugCommands {
    /// Compare macOS fast brew and `brew list`
    FastBrewCheck {},
}

fn print_sections(sections: Vec<(&'static str, Vec<String>)>) {
    if sections.is_empty() {
        println!("No differences found");
        return;
    }

    for (title, items) in sections {
        println!("{}:", title);
        for item in items {
            println!("- {}", item);
        }
        println!();
    }
}

fn print_fast_brew_comparison(label: &str, brew: &HashSet<String>, custom: &HashSet<String>) {
    if brew == custom {
        println!("{label}: brew and fast brew match");
        return;
    }

    let mut brew_sorted = brew.iter().collect::<Vec<_>>();
    brew_sorted.sort();
    let mut custom_sorted = custom.iter().collect::<Vec<_>>();
    custom_sorted.sort();
    let mut missing_from_custom = brew.difference(custom).collect::<Vec<_>>();
    missing_from_custom.sort();
    let mut missing_from_brew = custom.difference(brew).collect::<Vec<_>>();
    missing_from_brew.sort();

    println!("{label}: brew and fast brew differ");
    println!("brew: {brew_sorted:?}");
    println!();
    println!("fast brew: {custom_sorted:?}");
    println!();

    println!("missing from fast brew:");
    for item in missing_from_custom {
        println!("- {item}");
    }
    println!();

    println!("missing from brew:");
    for item in missing_from_brew {
        println!("- {item}");
    }
}

fn read_macos_config(system_config: PathBuf) -> Result<pkl_types::macos::MacOsConfig> {
    let config = fs::read_to_string(system_config)?;
    let config = serde_json::from_str::<pkl_types::macos::MacOsConfig>(&config)
        .context("Failed to read system config")?;
    config.validate().context("Invalid system config")?;
    Ok(config)
}

fn read_steamos_config(system_config: PathBuf) -> Result<pkl_types::steamos::SteamOsConfig> {
    let config = fs::read_to_string(system_config)?;
    let config = serde_json::from_str::<pkl_types::steamos::SteamOsConfig>(&config)
        .context("Failed to read system config")?;
    config.validate().context("Invalid system config")?;
    Ok(config)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Diff { command } => match command {
            DiffCommands::MacOs {
                system_config,
                fast_brew,
            } => {
                let config = read_macos_config(system_config)?;
                let sections = differs::macos::generate_diff(config, fast_brew)?;
                print_sections(sections);
            }
            DiffCommands::SteamOs { system_config } => {
                let config = read_steamos_config(system_config)?;
                let sections = differs::steamos::generate_diff(config)?;
                print_sections(sections);
            }
        },
        Commands::FileSync { command } => {
            let (files, root) = match command {
                FileSyncCommands::MacOs {
                    root,
                    system_config,
                } => (read_macos_config(system_config)?.files, root),
                FileSyncCommands::SteamOs {
                    root,
                    system_config,
                } => (read_steamos_config(system_config)?.files, root),
            };

            let report = file_checks::sync_sync_file_checks(&files, &root)?;
            for deleted in report.deleted {
                println!("Deleted {}", deleted.display());
            }
            for (symlink_path, symlink_target) in report.created {
                println!(
                    "Created {} -> {}",
                    symlink_path.display(),
                    symlink_target.display(),
                );
            }
            for warning in report.warnings {
                eprintln!("Warning: {}", warning);
            }
        }
        Commands::Debug { command } => match command {
            DebugCommands::FastBrewCheck {} => {
                let (formulae_brew, formulae_custom, casks_brew, casks_custom) =
                    std::thread::scope(|scope| -> Result<_> {
                        let formulae_brew =
                            scope.spawn(fetchers::macos::get_explicitly_installed_formulae_brew);
                        let formulae_custom =
                            scope.spawn(fetchers::macos::get_explicitly_installed_formulae_custom);
                        let casks_brew = scope.spawn(fetchers::macos::get_installed_casks_brew);
                        let casks_custom = scope.spawn(fetchers::macos::get_installed_casks_custom);

                        Ok((
                            formulae_brew
                                .join()
                                .map_err(|_| anyhow!("formulae brew thread panicked"))??,
                            formulae_custom
                                .join()
                                .map_err(|_| anyhow!("formulae custom thread panicked"))??,
                            casks_brew
                                .join()
                                .map_err(|_| anyhow!("cask brew thread panicked"))??,
                            casks_custom
                                .join()
                                .map_err(|_| anyhow!("cask custom thread panicked"))??,
                        ))
                    })?;

                print_fast_brew_comparison("formulae", &formulae_brew, &formulae_custom);
                println!();
                print_fast_brew_comparison("casks", &casks_brew, &casks_custom);
            }
        },
        Commands::Completions { shell } => {
            shell.generate(&mut Cli::command(), &mut std::io::stdout());
        }
    }
    Ok(())
}
