mod differs;
mod fetchers;
mod file_checks;
mod pkl_types;

use std::{collections::HashSet, fs, path::PathBuf};

use anyhow::{Context, Result, anyhow, bail};
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
        /// This should produce the same output and be faster but may be incorrect in some edge cases. See `debug
        /// fast-brew-check`.
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
    /// Compare macOS' `--fast-brew` and `brew list`
    ///
    /// This runs both the custom implementation and the `brew list` commands used if `--fast-brew` is not
    /// used. If they match, `--fast-brew` will produce the same diff as without the flag.
    FastBrewCheck {},

    /// Check for packages missing from `brew list --full-name`
    ///
    /// In some cases, installed casks may show in `brew list --cask` but be unexpectedly missing from `brew list --cask
    /// --full-name`. This has so far only been reported for casks but this command also attempts to check for a
    /// similar issue with formulae. If any packages are missing, this may lead to differences with `--fast-brew` (see
    /// `debug fast-brew-check`) and installed packages not being detected. Reinstalling the affected packages may
    /// resolve the issue.
    BrokenBrewInstallMetadataCheck {},
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

fn push_fast_brew_comparison(
    output: &mut String,
    label: &str,
    brew: &HashSet<String>,
    fast_brew: &HashSet<String>,
) {
    if brew == fast_brew {
        output.push_str(&format!("{label}: brew and fast brew match\n"));
        return;
    }

    let mut missing_from_fast_brew = brew.difference(fast_brew).collect::<Vec<_>>();
    missing_from_fast_brew.sort();
    let mut missing_from_brew = fast_brew.difference(brew).collect::<Vec<_>>();
    missing_from_brew.sort();

    output.push_str(&format!("{label}: brew and fast brew differ\n"));
    push_list(output, "brew", sorted_items(brew), true);
    push_list(output, "fast brew", sorted_items(fast_brew), true);
    push_list(
        output,
        "missing from fast brew",
        missing_from_fast_brew,
        false,
    );
    push_list(output, "missing from brew", missing_from_brew, false);
}

fn fast_brew_comparison_output(
    formulae_brew: &HashSet<String>,
    formulae_fast_brew: &HashSet<String>,
    casks_brew: &HashSet<String>,
    casks_fast_brew: &HashSet<String>,
) -> String {
    let mut output = String::new();
    push_fast_brew_comparison(&mut output, "formulae", formulae_brew, formulae_fast_brew);
    output.push('\n');
    push_fast_brew_comparison(&mut output, "casks", casks_brew, casks_fast_brew);
    output
}

fn sorted_items(items: &HashSet<String>) -> Vec<&String> {
    let mut items = items.iter().collect::<Vec<_>>();
    items.sort();
    items
}

fn push_list(output: &mut String, title: &str, items: Vec<&String>, single_line: bool) {
    if items.is_empty() {
        return;
    }

    if !output.ends_with("\n\n") {
        output.push('\n');
    }

    output.push_str(title);
    output.push_str(":\n");
    if single_line {
        output.push_str(&format!("{items:?}"));
        output.push('\n');
    } else {
        for item in items {
            output.push_str("- ");
            output.push_str(item);
            output.push('\n');
        }
    }
}

fn strip_tap_prefixes(items: &HashSet<String>) -> Result<HashSet<String>> {
    let mut stripped = HashSet::new();
    for item in items {
        let token = item.rsplit('/').next().unwrap_or(item).to_owned();
        if !stripped.insert(token.clone()) {
            bail!(
                "cannot compare Homebrew metadata because multiple full names resolve to `{token}`",
            );
        }
    }
    Ok(stripped)
}

fn broken_brew_install_metadata_check_output(
    formulae: &HashSet<String>,
    dependency_formulae: &HashSet<String>,
    requested_formulae: &HashSet<String>,
    cask_tokens: &HashSet<String>,
    full_name_casks: &HashSet<String>,
) -> Result<String> {
    let dependency_formulae = strip_tap_prefixes(dependency_formulae)?;
    let requested_formulae = strip_tap_prefixes(requested_formulae)?;
    let full_name_casks = strip_tap_prefixes(full_name_casks)?;

    let expected_requested_formulae = formulae
        .difference(&dependency_formulae)
        .cloned()
        .collect::<HashSet<_>>();

    let formulae_match = expected_requested_formulae == requested_formulae;
    let casks_match = cask_tokens == &full_name_casks;

    let mut output = String::new();

    if formulae_match {
        output.push_str("No formula list issues found\n");
    } else {
        output.push_str("Formulae lists differ\n\n");
        push_list(
            &mut output,
            "brew list --formula",
            sorted_items(formulae),
            true,
        );
        push_list(
            &mut output,
            "brew list --installed-as-dependency --full-name",
            sorted_items(&dependency_formulae),
            true,
        );
        push_list(
            &mut output,
            "Expected installed-on-request formulae",
            sorted_items(&expected_requested_formulae),
            true,
        );
        push_list(
            &mut output,
            "brew list --installed-on-request --full-name",
            sorted_items(&requested_formulae),
            true,
        );

        let mut missing_from_installed_on_request = expected_requested_formulae
            .difference(&requested_formulae)
            .collect::<Vec<_>>();
        missing_from_installed_on_request.sort();
        let mut unexpected_installed_on_request = requested_formulae
            .difference(&expected_requested_formulae)
            .collect::<Vec<_>>();
        unexpected_installed_on_request.sort();

        push_list(
            &mut output,
            "Missing from brew list --installed-on-request --full-name",
            missing_from_installed_on_request,
            false,
        );
        push_list(
            &mut output,
            "Unexpected in brew list --installed-on-request --full-name",
            unexpected_installed_on_request,
            false,
        );
    }

    output.push_str("\n\n");

    if casks_match {
        output.push_str("No cask list issues found\n");
    } else {
        output.push_str("Cask lists differ\n\n");

        let mut missing_from_full_name =
            cask_tokens.difference(&full_name_casks).collect::<Vec<_>>();
        missing_from_full_name.sort();
        let mut missing_from_bare = full_name_casks.difference(cask_tokens).collect::<Vec<_>>();
        missing_from_bare.sort();

        push_list(
            &mut output,
            "In brew list --cask but missing from brew list --cask --full-name",
            missing_from_full_name,
            false,
        );
        push_list(
            &mut output,
            "In brew list --cask --full-name but missing from brew list --cask",
            missing_from_bare,
            false,
        );
    }

    Ok(output)
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

                print!(
                    "{}",
                    fast_brew_comparison_output(
                        &formulae_brew,
                        &formulae_custom,
                        &casks_brew,
                        &casks_custom,
                    ),
                );
            }
            DebugCommands::BrokenBrewInstallMetadataCheck {} => {
                let (formulae, dependency_formulae, requested_formulae, cask_tokens, casks) =
                    std::thread::scope(|scope| -> Result<_> {
                        let formulae = scope.spawn(fetchers::macos::get_installed_formulae_brew);
                        let dependency_formulae =
                            scope.spawn(fetchers::macos::get_dependency_formulae_brew);
                        let requested_formulae =
                            scope.spawn(fetchers::macos::get_explicitly_installed_formulae_brew);
                        let cask_tokens =
                            scope.spawn(fetchers::macos::get_installed_cask_tokens_brew);
                        let casks = scope.spawn(fetchers::macos::get_installed_casks_brew);

                        Ok((
                            formulae
                                .join()
                                .map_err(|_| anyhow!("formulae thread panicked"))??,
                            dependency_formulae
                                .join()
                                .map_err(|_| anyhow!("dependency formulae thread panicked"))??,
                            requested_formulae
                                .join()
                                .map_err(|_| anyhow!("requested formulae thread panicked"))??,
                            cask_tokens
                                .join()
                                .map_err(|_| anyhow!("cask tokens thread panicked"))??,
                            casks
                                .join()
                                .map_err(|_| anyhow!("casks thread panicked"))??,
                        ))
                    })?;

                print!(
                    "{}",
                    broken_brew_install_metadata_check_output(
                        &formulae,
                        &dependency_formulae,
                        &requested_formulae,
                        &cask_tokens,
                        &casks,
                    )?,
                );
            }
        },
        Commands::Completions { shell } => {
            shell.generate(&mut Cli::command(), &mut std::io::stdout());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|&item| item.to_owned()).collect()
    }

    #[test]
    fn broken_brew_install_metadata_check_output_when_metadata_is_valid() {
        assert_eq!(
            broken_brew_install_metadata_check_output(
                &set(&["dependency", "requested", "tapped-requested"]),
                &set(&["homebrew/core/dependency"]),
                &set(&["requested", "user/repo/tapped-requested"]),
                &set(&["cask", "tapped-cask"]),
                &set(&["cask", "user/repo/tapped-cask"]),
            )
            .unwrap(),
            concat!(
                "No formula list issues found\n",
                "\n\nNo cask list issues found\n",
            ),
        );
    }

    #[test]
    fn broken_brew_install_metadata_check_output_for_formulae_mismatch() {
        assert_eq!(
            broken_brew_install_metadata_check_output(
                &set(&["dependency", "missing-requested", "requested"]),
                &set(&["homebrew/core/dependency"]),
                &set(&["requested", "user/repo/unexpected-requested"]),
                &set(&["cask"]),
                &set(&["cask"]),
            )
            .unwrap(),
            concat!(
                "Formulae lists differ\n",
                "\n",
                "brew list --formula:\n",
                "[\"dependency\", \"missing-requested\", \"requested\"]\n",
                "\nbrew list --installed-as-dependency --full-name:\n",
                "[\"dependency\"]\n",
                "\nExpected installed-on-request formulae:\n",
                "[\"missing-requested\", \"requested\"]\n",
                "\nbrew list --installed-on-request --full-name:\n",
                "[\"requested\", \"unexpected-requested\"]\n",
                "\nMissing from brew list --installed-on-request --full-name:\n",
                "- missing-requested\n",
                "\nUnexpected in brew list --installed-on-request --full-name:\n",
                "- unexpected-requested\n",
                "\n\nNo cask list issues found\n",
            ),
        );
    }

    #[test]
    fn broken_brew_install_metadata_check_output_for_cask_mismatch() {
        assert_eq!(
            broken_brew_install_metadata_check_output(
                &set(&["requested"]),
                &set(&[]),
                &set(&["requested"]),
                &set(&["missing-full-name", "shared"]),
                &set(&["missing-bare", "user/repo/shared"]),
            )
            .unwrap(),
            concat!(
                "No formula list issues found\n",
                "\n\nCask lists differ\n",
                "\n",
                "In brew list --cask but missing from brew list --cask --full-name:\n",
                "- missing-full-name\n",
                "\nIn brew list --cask --full-name but missing from brew list --cask:\n",
                "- missing-bare\n",
            ),
        );
    }

    #[test]
    fn broken_brew_install_metadata_check_output_for_formulae_and_cask_mismatch() {
        assert_eq!(
            broken_brew_install_metadata_check_output(
                &set(&["dependency", "missing-requested", "requested"]),
                &set(&["homebrew/core/dependency"]),
                &set(&["requested", "user/repo/unexpected-requested"]),
                &set(&["missing-full-name", "shared"]),
                &set(&["missing-bare", "user/repo/shared"]),
            )
            .unwrap(),
            concat!(
                "Formulae lists differ\n",
                "\n",
                "brew list --formula:\n",
                "[\"dependency\", \"missing-requested\", \"requested\"]\n",
                "\nbrew list --installed-as-dependency --full-name:\n",
                "[\"dependency\"]\n",
                "\nExpected installed-on-request formulae:\n",
                "[\"missing-requested\", \"requested\"]\n",
                "\nbrew list --installed-on-request --full-name:\n",
                "[\"requested\", \"unexpected-requested\"]\n",
                "\nMissing from brew list --installed-on-request --full-name:\n",
                "- missing-requested\n",
                "\nUnexpected in brew list --installed-on-request --full-name:\n",
                "- unexpected-requested\n",
                "\n\nCask lists differ\n",
                "\n",
                "In brew list --cask but missing from brew list --cask --full-name:\n",
                "- missing-full-name\n",
                "\nIn brew list --cask --full-name but missing from brew list --cask:\n",
                "- missing-bare\n",
            ),
        );
    }

    #[test]
    fn broken_brew_install_metadata_check_output_errors_on_duplicate_stripped_formulae() {
        assert_eq!(
            broken_brew_install_metadata_check_output(
                &set(&["requested"]),
                &set(&[]),
                &set(&["requested", "user/repo/requested"]),
                &set(&["cask"]),
                &set(&["cask"]),
            )
            .unwrap_err()
            .to_string(),
            "cannot compare Homebrew metadata because multiple full names resolve to `requested`",
        );
    }

    #[test]
    fn broken_brew_install_metadata_check_output_errors_on_duplicate_stripped_casks() {
        assert_eq!(
            broken_brew_install_metadata_check_output(
                &set(&["requested"]),
                &set(&[]),
                &set(&["requested"]),
                &set(&["cask"]),
                &set(&["cask", "user/repo/cask"]),
            )
            .unwrap_err()
            .to_string(),
            "cannot compare Homebrew metadata because multiple full names resolve to `cask`",
        );
    }
}
