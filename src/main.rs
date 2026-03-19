mod macos;
mod pkl_types;

use std::{collections::HashSet, fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser, Subcommand};
use validator::Validate;

use pkl_types::macos::MacOsApp;

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

fn sort_paths_by_app_name<T: AsRef<str>>(paths: &mut [T]) {
    paths.sort_by(|a, b| {
        let a = a.as_ref();
        let b = b.as_ref();
        let a_name = a.rsplit('/').next().expect("app path should have a slash");
        let b_name = b.rsplit('/').next().expect("app path should have a slash");
        a_name.cmp(b_name).then_with(|| a.cmp(b))
    });
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Diff { command } => match command {
            DiffCommands::MacOs { system_config } => {
                let config = fs::read_to_string(system_config)?;
                let config = serde_json::from_str::<pkl_types::macos::MacOsConfig>(&config)
                    .context("Failed to read system config")?;
                config.validate().context("Invalid system config")?;

                let system_has_homebrew = macos::homebrew_is_installed();
                let system_apps = macos::get_apps()?;
                let installed_casks = if system_has_homebrew {
                    macos::get_installed_casks()?
                } else {
                    HashSet::new()
                };

                let configured_app_paths: HashSet<&str> = config
                    .apps
                    .iter()
                    .flat_map(|app| match app {
                        MacOsApp::HomebrewCask(cask) => cask.base.app_paths.iter(),
                        MacOsApp::MacAppStoreApp(app_store_app) => {
                            app_store_app.base.app_paths.iter()
                        }
                    })
                    .map(String::as_str)
                    .collect();

                let configured_cask_apps: Vec<(&str, &[String])> = config
                    .apps
                    .iter()
                    .filter_map(|app| match app {
                        MacOsApp::HomebrewCask(cask) => {
                            Some((cask.cask_name.as_str(), cask.base.app_paths.as_slice()))
                        }
                        MacOsApp::MacAppStoreApp(_) => None,
                    })
                    .collect();

                let configured_casks: HashSet<&str> = configured_cask_apps
                    .iter()
                    .map(|(cask_name, _)| *cask_name)
                    .collect();

                let mut sections: Vec<(&str, Vec<String>)> = Vec::new();

                if config.install_homebrew != system_has_homebrew {
                    sections.push((
                        "Homebrew installation status mismatch",
                        vec![format!(
                            "config install_homebrew = {}, system has Homebrew = {}",
                            config.install_homebrew, system_has_homebrew
                        )],
                    ));
                }

                {
                    let mut configured_casks_not_installed: Vec<String> = configured_casks
                        .iter()
                        .filter(|&cask_name| !installed_casks.contains(*cask_name))
                        .map(|&s| s.to_owned())
                        .collect();
                    configured_casks_not_installed.sort();
                    if !configured_casks_not_installed.is_empty() {
                        sections.push((
                            "Configured casks not installed",
                            configured_casks_not_installed,
                        ));
                    }
                }

                {
                    let mut installed_casks_not_configured: Vec<String> = installed_casks
                        .iter()
                        .filter(|cask_name| !configured_casks.contains(cask_name.as_str()))
                        .cloned()
                        .collect();
                    installed_casks_not_configured.sort();
                    if !installed_casks_not_configured.is_empty() {
                        sections.push((
                            "Installed casks not in config",
                            installed_casks_not_configured,
                        ));
                    }
                }

                {
                    let mut casks_with_missing_paths = Vec::new();
                    for (cask_name, app_paths) in configured_cask_apps {
                        if !installed_casks.contains(cask_name) {
                            continue;
                        }

                        let mut missing_paths: Vec<&str> = app_paths
                            .iter()
                            .filter(|app_path| !system_apps.contains(*app_path))
                            .map(String::as_str)
                            .collect();
                        sort_paths_by_app_name(&mut missing_paths);

                        if !missing_paths.is_empty() {
                            casks_with_missing_paths.push(format!(
                                "{} -> missing {}",
                                cask_name,
                                missing_paths.join(", ")
                            ));
                        }
                    }
                    casks_with_missing_paths.sort();
                    if !casks_with_missing_paths.is_empty() {
                        sections.push((
                            "Installed casks missing configured app paths",
                            casks_with_missing_paths,
                        ));
                    }
                }

                {
                    let mut apps_not_in_config: Vec<String> = system_apps
                        .iter()
                        .filter(|app_path| !configured_app_paths.contains(app_path.as_str()))
                        .cloned()
                        .collect();
                    sort_paths_by_app_name(&mut apps_not_in_config);
                    if !apps_not_in_config.is_empty() {
                        sections.push(("Installed apps not in config", apps_not_in_config));
                    }
                }

                if sections.is_empty() {
                    println!("No differences found");
                } else {
                    for (title, items) in sections {
                        println!("{}:", title);
                        for item in items {
                            println!("- {}", item);
                        }
                        println!();
                    }
                }
            }
        },
        Commands::Completions { shell } => {
            shell.generate(&mut Cli::command(), &mut std::io::stdout());
        }
    }
    Ok(())
}
