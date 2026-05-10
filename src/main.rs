mod file_checks;
mod macos;
mod pkl_types;
mod steamos;

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};
use validator::Validate;

use pkl_types::{
    macos::MacOsApp,
    steamos::{DeckyStoreChannel, DeckyUpdateChannel},
};

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
    /// SteamOS
    #[command(name = "steamos")]
    SteamOs {
        /// Output directory that will contain the mirrored symlinks
        #[arg(short = 'o', long = "root")]
        root: PathBuf,

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

fn decky_update_channel_name(channel: &DeckyUpdateChannel) -> &'static str {
    match channel {
        DeckyUpdateChannel::Stable => "stable",
        DeckyUpdateChannel::Prerelease => "prerelease",
    }
}

fn system_decky_update_channel_name(channel: &steamos::UpdateChannel) -> &'static str {
    match channel {
        steamos::UpdateChannel::Stable => "stable",
        steamos::UpdateChannel::Prerelease => "prerelease",
    }
}

fn decky_store_channel_name(channel: &DeckyStoreChannel) -> &'static str {
    match channel {
        DeckyStoreChannel::Default => "default",
        DeckyStoreChannel::Prerelease => "prerelease",
    }
}

fn system_decky_store_channel_name(channel: &steamos::StoreChannel) -> &'static str {
    match channel {
        steamos::StoreChannel::Default => "default",
        steamos::StoreChannel::Prerelease => "prerelease",
    }
}

fn configured_decky_plugin_matches_installed(
    configured: &pkl_types::steamos::DeckyPlugin,
    installed: &steamos::DeckyPlugin,
) -> bool {
    match configured.directory_name.as_deref() {
        Some(directory_name) => {
            installed.name == configured.name && installed.directory_name == directory_name
        }
        None => installed.name == configured.name,
    }
}

fn format_configured_decky_plugin(plugin: &pkl_types::steamos::DeckyPlugin) -> String {
    match plugin.directory_name.as_deref() {
        Some(directory_name) => format!("{} ({})", plugin.name, directory_name),
        None => plugin.name.clone(),
    }
}

fn format_installed_decky_plugin(plugin: &steamos::DeckyPlugin) -> String {
    format!("{} ({})", plugin.name, plugin.directory_name)
}

fn format_list(items: &[String]) -> String {
    if items.is_empty() {
        "(empty)".to_owned()
    } else {
        items.join(", ")
    }
}

fn print_sections(sections: Vec<(&str, Vec<String>)>) {
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
                let (installed_app_store_apps, installed_testflight_apps): (
                    HashMap<u64, String>,
                    HashSet<String>,
                ) = match macos::get_installed_mas_apps() {
                    Ok(installed_mas_apps) => {
                        let mut app_store = HashMap::new();
                        let mut testflight = HashSet::new();
                        for app in installed_mas_apps {
                            match app {
                                macos::MacAppStoreApp::AppStore { app_id, app_name } => {
                                    app_store.insert(app_id, app_name);
                                }
                                macos::MacAppStoreApp::TestFlight { app_name } => {
                                    testflight.insert(app_name);
                                }
                            }
                        }
                        (app_store, testflight)
                    }
                    Err(macos::MacAppStoreListError::MasNotFound) => {
                        eprintln!(
                            "`mas` is not installed, so installed Mac App Store apps cannot be checked"
                        );
                        (HashMap::new(), HashSet::new())
                    }
                    Err(error) => {
                        return Err(error).context("Failed to get installed Mac App Store apps");
                    }
                };

                let configured_app_paths: HashSet<&str> = config
                    .apps
                    .iter()
                    .flat_map(|app| match app {
                        MacOsApp::ManualApp(manual_app) => manual_app.base.app_paths.iter(),
                        MacOsApp::HomebrewCask(cask) => cask.base.app_paths.iter(),
                        MacOsApp::MacAppStoreApp(app_store_app) => {
                            app_store_app.base.app_paths.iter()
                        }
                        MacOsApp::TestFlightApp(testflight_app) => {
                            testflight_app.base.app_paths.iter()
                        }
                    })
                    .map(String::as_str)
                    .collect();

                let configured_manual_apps: Vec<(&str, &[String])> = config
                    .apps
                    .iter()
                    .filter_map(|app| match app {
                        MacOsApp::ManualApp(manual_app) => Some((
                            manual_app.name.as_str(),
                            manual_app.base.app_paths.as_slice(),
                        )),
                        _ => None,
                    })
                    .collect();

                let configured_cask_apps: Vec<(&str, &[String])> = config
                    .apps
                    .iter()
                    .filter_map(|app| match app {
                        MacOsApp::HomebrewCask(cask) => {
                            Some((cask.cask_name.as_str(), cask.base.app_paths.as_slice()))
                        }
                        _ => None,
                    })
                    .collect();

                let configured_app_store_apps: Vec<(u64, &[String])> = config
                    .apps
                    .iter()
                    .filter_map(|app| match app {
                        MacOsApp::MacAppStoreApp(app_store_app) => Some((
                            app_store_app.app_store_id,
                            app_store_app.base.app_paths.as_slice(),
                        )),
                        _ => None,
                    })
                    .collect();

                let configured_testflight_apps: Vec<(&str, &[String])> = config
                    .apps
                    .iter()
                    .filter_map(|app| match app {
                        MacOsApp::TestFlightApp(testflight_app) => Some((
                            testflight_app.name.as_str(),
                            testflight_app.base.app_paths.as_slice(),
                        )),
                        _ => None,
                    })
                    .collect();

                let configured_casks: HashSet<&str> = configured_cask_apps
                    .iter()
                    .map(|(cask_name, _)| *cask_name)
                    .collect();
                let configured_non_app_casks: HashSet<&str> = config
                    .homebrew
                    .as_ref()
                    .map(|homebrew| homebrew.non_app_casks.iter().map(String::as_str).collect())
                    .unwrap_or_default();
                let all_configured_casks: HashSet<&str> = configured_casks
                    .union(&configured_non_app_casks)
                    .copied()
                    .collect();
                let configured_app_store_ids: HashSet<u64> = configured_app_store_apps
                    .iter()
                    .map(|(app_store_id, _)| *app_store_id)
                    .collect();
                let configured_testflight_names: HashSet<&str> = configured_testflight_apps
                    .iter()
                    .map(|(name, _)| *name)
                    .collect();

                let mut sections: Vec<(&str, Vec<String>)> = Vec::new();

                if config.homebrew.is_some() != system_has_homebrew {
                    sections.push((
                        "Homebrew installation status mismatch",
                        vec![format!(
                            "config homebrew installed = {}, system has Homebrew = {}",
                            config.homebrew.is_some(),
                            system_has_homebrew,
                        )],
                    ));
                }

                {
                    let mut manual_apps_with_missing_paths = Vec::new();
                    for (app_name, app_paths) in configured_manual_apps {
                        let mut missing_paths: Vec<&str> = app_paths
                            .iter()
                            .filter(|app_path| !system_apps.contains(*app_path))
                            .map(String::as_str)
                            .collect();
                        sort_paths_by_app_name(&mut missing_paths);

                        if !missing_paths.is_empty() {
                            manual_apps_with_missing_paths.push(format!(
                                "{} -> missing {}",
                                app_name,
                                missing_paths.join(", "),
                            ));
                        }
                    }
                    manual_apps_with_missing_paths.sort();
                    if !manual_apps_with_missing_paths.is_empty() {
                        sections.push((
                            "Configured manual apps missing configured app paths",
                            manual_apps_with_missing_paths,
                        ));
                    }
                }

                {
                    let mut configured_casks_not_installed: Vec<String> = all_configured_casks
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
                        .filter(|cask_name| !all_configured_casks.contains(cask_name.as_str()))
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
                                missing_paths.join(", "),
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
                    let mut configured_app_store_not_installed: Vec<String> =
                        configured_app_store_ids
                            .iter()
                            .filter(|app_store_id| {
                                !installed_app_store_apps.contains_key(app_store_id)
                            })
                            .map(|app_store_id| app_store_id.to_string())
                            .collect();
                    configured_app_store_not_installed.sort();
                    if !configured_app_store_not_installed.is_empty() {
                        sections.push((
                            "Configured App Store apps not installed",
                            configured_app_store_not_installed,
                        ));
                    }
                }

                {
                    let mut installed_app_store_not_configured: Vec<String> =
                        installed_app_store_apps
                            .iter()
                            .filter(|(app_store_id, _)| {
                                !configured_app_store_ids.contains(app_store_id)
                            })
                            .map(|(app_store_id, app_name)| {
                                format!("{} ({})", app_name, app_store_id)
                            })
                            .collect();
                    installed_app_store_not_configured.sort();
                    if !installed_app_store_not_configured.is_empty() {
                        sections.push((
                            "Installed App Store apps not in config",
                            installed_app_store_not_configured,
                        ));
                    }
                }

                {
                    let mut app_store_apps_with_missing_paths = Vec::new();
                    for (app_store_id, app_paths) in configured_app_store_apps {
                        if !installed_app_store_apps.contains_key(&app_store_id) {
                            continue;
                        }

                        let mut missing_paths: Vec<&str> = app_paths
                            .iter()
                            .filter(|app_path| !system_apps.contains(*app_path))
                            .map(String::as_str)
                            .collect();
                        sort_paths_by_app_name(&mut missing_paths);

                        if !missing_paths.is_empty() {
                            app_store_apps_with_missing_paths.push(format!(
                                "{} -> missing {}",
                                app_store_id,
                                missing_paths.join(", "),
                            ));
                        }
                    }
                    app_store_apps_with_missing_paths.sort();
                    if !app_store_apps_with_missing_paths.is_empty() {
                        sections.push((
                            "Installed App Store apps missing configured app paths",
                            app_store_apps_with_missing_paths,
                        ));
                    }
                }

                {
                    let mut configured_testflight_not_installed: Vec<String> =
                        configured_testflight_names
                            .iter()
                            .filter(|name| !installed_testflight_apps.contains(**name))
                            .map(|&s| s.to_owned())
                            .collect();
                    configured_testflight_not_installed.sort();
                    if !configured_testflight_not_installed.is_empty() {
                        sections.push((
                            "Configured TestFlight apps not installed",
                            configured_testflight_not_installed,
                        ));
                    }
                }

                {
                    let mut installed_testflight_not_configured: Vec<String> =
                        installed_testflight_apps
                            .iter()
                            .filter(|name| !configured_testflight_names.contains(name.as_str()))
                            .cloned()
                            .collect();
                    installed_testflight_not_configured.sort();
                    if !installed_testflight_not_configured.is_empty() {
                        sections.push((
                            "Installed TestFlight apps not in config",
                            installed_testflight_not_configured,
                        ));
                    }
                }

                {
                    let mut testflight_apps_with_missing_paths = Vec::new();
                    for (app_name, app_paths) in &configured_testflight_apps {
                        if !installed_testflight_apps.contains(*app_name) {
                            continue;
                        }

                        let mut missing_paths: Vec<&str> = app_paths
                            .iter()
                            .filter(|app_path| !system_apps.contains(*app_path))
                            .map(String::as_str)
                            .collect();
                        sort_paths_by_app_name(&mut missing_paths);

                        if !missing_paths.is_empty() {
                            testflight_apps_with_missing_paths.push(format!(
                                "{} -> missing {}",
                                app_name,
                                missing_paths.join(", "),
                            ));
                        }
                    }
                    testflight_apps_with_missing_paths.sort();
                    if !testflight_apps_with_missing_paths.is_empty() {
                        sections.push((
                            "Installed TestFlight apps missing configured app paths",
                            testflight_apps_with_missing_paths,
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

                print_sections(sections);
            }
            DiffCommands::SteamOs { system_config } => {
                let config = read_steamos_config(system_config)?;

                let system_hostname = steamos::get_hostname()?;
                let system_charge_limit = steamos::get_charge_limit()?.unwrap_or(100);
                let installed_flatpaks = steamos::get_installed_flatpak_apps()?;
                let user_steam_settings = steamos::get_user_steam_settings()?;
                let system_decky_installed = steamos::is_decky_installed()?;
                let enabled_systemd_units = steamos::get_enabled_systemd_units()?;

                let (steam_account_id, steam_user_settings, steam_client_user_settings) = {
                    let steam_user_ids = steamos::get_steam_user_settings_ids()?;
                    let mut steam_user_ids = steam_user_ids.into_iter();
                    let steam_account_id = steam_user_ids
                        .next()
                        .context("Expected exactly one Steam user settings directory")?;
                    if steam_user_ids.next().is_some() {
                        bail!(
                            "Found multiple Steam user settings directories; only one is currently supported"
                        );
                    }
                    let steam_user_settings = steamos::get_steam_user_settings(&steam_account_id)?;
                    let steam_client_user_settings =
                        steamos::get_steam_client_user_settings(&steam_account_id)?;
                    (
                        steam_account_id,
                        steam_user_settings,
                        steam_client_user_settings,
                    )
                };

                let configured_flatpaks: HashSet<&str> = config
                    .installed_flatpaks
                    .iter()
                    .map(String::as_str)
                    .collect();

                let mut sections: Vec<(&str, Vec<String>)> = Vec::new();

                if config.hostname != system_hostname {
                    sections.push((
                        "Hostname mismatch",
                        vec![format!(
                            "config hostname = {}, system hostname = {}",
                            config.hostname, system_hostname,
                        )],
                    ));
                }

                {
                    let mut steam_os_settings_mismatches = Vec::new();
                    if config.steam_os_settings.steam_developer_mode
                        != user_steam_settings.developer_mode
                    {
                        steam_os_settings_mismatches.push(format!(
                            "config steamDeveloperMode = {}, system steamDeveloperMode = {}",
                            config.steam_os_settings.steam_developer_mode,
                            user_steam_settings.developer_mode,
                        ));
                    }

                    if config.steam_os_settings.charge_limit != system_charge_limit {
                        steam_os_settings_mismatches.push(format!(
                            "config chargeLimit = {}, system chargeLimit = {}",
                            config.steam_os_settings.charge_limit, system_charge_limit,
                        ));
                    }

                    if !steam_os_settings_mismatches.is_empty() {
                        sections.push(("SteamOS settings mismatch", steam_os_settings_mismatches));
                    }
                }

                {
                    let mut steam_settings_mismatches = Vec::new();
                    if config.steam_settings.sign_into_friends
                        != steam_user_settings.sign_into_friends
                    {
                        steam_settings_mismatches.push(format!(
                            "{} -> config signIntoFriends = {}, system signIntoFriends = {}",
                            steam_account_id,
                            config.steam_settings.sign_into_friends,
                            steam_user_settings.sign_into_friends,
                        ));
                    }

                    if config.steam_settings.twenty_four_hour_clock
                        != steam_client_user_settings.twenty_four_hour_clock
                    {
                        steam_settings_mismatches.push(format!(
                            "{} -> config twentyFourHourClock = {}, system twentyFourHourClock = {}",
                            steam_account_id,
                            config.steam_settings.twenty_four_hour_clock,
                            steam_client_user_settings.twenty_four_hour_clock,
                        ));
                    }

                    if !steam_settings_mismatches.is_empty() {
                        sections.push(("Steam settings mismatch", steam_settings_mismatches));
                    }
                }

                {
                    let mut configured_flatpaks_not_installed: Vec<String> = configured_flatpaks
                        .iter()
                        .filter(|flatpak| !installed_flatpaks.contains(**flatpak))
                        .map(|&flatpak| flatpak.to_owned())
                        .collect();
                    configured_flatpaks_not_installed.sort();
                    if !configured_flatpaks_not_installed.is_empty() {
                        sections.push((
                            "Configured Flatpaks not installed",
                            configured_flatpaks_not_installed,
                        ));
                    }
                }

                {
                    let mut installed_flatpaks_not_configured: Vec<String> = installed_flatpaks
                        .iter()
                        .filter(|flatpak| !configured_flatpaks.contains(flatpak.as_str()))
                        .cloned()
                        .collect();
                    installed_flatpaks_not_configured.sort();
                    if !installed_flatpaks_not_configured.is_empty() {
                        sections.push((
                            "Installed Flatpaks not in config",
                            installed_flatpaks_not_configured,
                        ));
                    }
                }

                if config.decky.is_some() != system_decky_installed {
                    sections.push((
                        "Decky installation status mismatch",
                        vec![format!(
                            "config decky installed = {}, system Decky installed = {}",
                            config.decky.is_some(),
                            system_decky_installed,
                        )],
                    ));
                }

                if let (Some(decky), true) = (&config.decky, system_decky_installed) {
                    let system_decky_settings = steamos::get_decky_settings()?;
                    let installed_decky_plugins = steamos::get_installed_decky_plugins()?;

                    {
                        let mut decky_settings_mismatches = Vec::new();
                        if decky_update_channel_name(&decky.settings.update_channel)
                            != system_decky_update_channel_name(
                                &system_decky_settings.update_channel,
                            )
                        {
                            decky_settings_mismatches.push(format!(
                                "config updateChannel = {}, system updateChannel = {}",
                                decky_update_channel_name(&decky.settings.update_channel),
                                system_decky_update_channel_name(
                                    &system_decky_settings.update_channel,
                                ),
                            ));
                        }
                        if decky_store_channel_name(&decky.settings.store_channel)
                            != system_decky_store_channel_name(&system_decky_settings.store_channel)
                        {
                            decky_settings_mismatches.push(format!(
                                "config storeChannel = {}, system storeChannel = {}",
                                decky_store_channel_name(&decky.settings.store_channel),
                                system_decky_store_channel_name(
                                    &system_decky_settings.store_channel,
                                ),
                            ));
                        }
                        if decky.settings.decky_update_notification
                            != system_decky_settings.decky_update_notifications
                        {
                            decky_settings_mismatches.push(format!(
                                "config deckyUpdateNotification = {}, system deckyUpdateNotification = {}",
                                decky.settings.decky_update_notification,
                                system_decky_settings.decky_update_notifications,
                            ));
                        }
                        if decky.settings.plugin_update_notification
                            != system_decky_settings.plugins_update_notifications
                        {
                            decky_settings_mismatches.push(format!(
                                "config pluginUpdateNotification = {}, system pluginUpdateNotification = {}",
                                decky.settings.plugin_update_notification,
                                system_decky_settings.plugins_update_notifications,
                            ));
                        }
                        if decky.settings.developer_mode != system_decky_settings.developer_mode {
                            decky_settings_mismatches.push(format!(
                                "config developerMode = {}, system developerMode = {}",
                                decky.settings.developer_mode, system_decky_settings.developer_mode,
                            ));
                        }

                        if !decky_settings_mismatches.is_empty() {
                            sections.push(("Decky settings mismatch", decky_settings_mismatches));
                        }
                    }

                    {
                        let mut configured_decky_plugins_not_installed: Vec<String> = decky
                            .plugins
                            .iter()
                            .filter(|plugin| {
                                !installed_decky_plugins.iter().any(|installed_plugin| {
                                    configured_decky_plugin_matches_installed(
                                        plugin,
                                        installed_plugin,
                                    )
                                })
                            })
                            .map(format_configured_decky_plugin)
                            .collect();
                        configured_decky_plugins_not_installed.sort();
                        if !configured_decky_plugins_not_installed.is_empty() {
                            sections.push((
                                "Configured Decky plugins not installed",
                                configured_decky_plugins_not_installed,
                            ));
                        }
                    }

                    {
                        let mut installed_decky_plugins_not_configured: Vec<String> =
                            installed_decky_plugins
                                .iter()
                                .filter(|installed_plugin| {
                                    !decky.plugins.iter().any(|plugin| {
                                        configured_decky_plugin_matches_installed(
                                            plugin,
                                            installed_plugin,
                                        )
                                    })
                                })
                                .map(format_installed_decky_plugin)
                                .collect();
                        installed_decky_plugins_not_configured.sort();
                        if !installed_decky_plugins_not_configured.is_empty() {
                            sections.push((
                                "Installed Decky plugins not in config",
                                installed_decky_plugins_not_configured,
                            ));
                        }
                    }

                    {
                        let mut decky_plugin_state_mismatches = Vec::new();
                        for plugin in &decky.plugins {
                            let Some(installed_plugin) =
                                installed_decky_plugins.iter().find(|installed_plugin| {
                                    configured_decky_plugin_matches_installed(
                                        plugin,
                                        installed_plugin,
                                    )
                                })
                            else {
                                continue;
                            };

                            let system_plugin_disabled = system_decky_settings
                                .disabled_plugins
                                .contains(installed_plugin.name.as_str());
                            if plugin.disabled != system_plugin_disabled {
                                decky_plugin_state_mismatches.push(format!(
                                    "{} -> config disabled = {}, system disabled = {}",
                                    format_configured_decky_plugin(plugin),
                                    plugin.disabled,
                                    system_plugin_disabled,
                                ));
                            }
                        }
                        decky_plugin_state_mismatches.sort();
                        if !decky_plugin_state_mismatches.is_empty() {
                            sections.push((
                                "Decky plugin disabled status mismatch",
                                decky_plugin_state_mismatches,
                            ));
                        }
                    }
                }

                {
                    let configured_enabled_systemd_units: HashSet<&str> = config
                        .enabled_systemd_units
                        .iter()
                        .map(String::as_str)
                        .collect();
                    let mut configured_enabled_systemd_units_not_enabled: Vec<String> =
                        configured_enabled_systemd_units
                            .iter()
                            .filter(|unit| !enabled_systemd_units.contains(**unit))
                            .map(|&unit| unit.to_owned())
                            .collect();
                    configured_enabled_systemd_units_not_enabled.sort();
                    if !configured_enabled_systemd_units_not_enabled.is_empty() {
                        sections.push((
                            "Configured enabled systemd units not enabled",
                            configured_enabled_systemd_units_not_enabled,
                        ));
                    }
                }

                if let Some(desktop) = &config.desktop {
                    let desktop_files = steamos::get_desktop_files()?;
                    let configured_desktop_files: HashSet<&str> =
                        desktop.iter().map(String::as_str).collect();

                    {
                        let mut configured_desktop_files_missing: Vec<String> =
                            configured_desktop_files
                                .iter()
                                .filter(|file_name| !desktop_files.contains(**file_name))
                                .map(|&file_name| file_name.to_owned())
                                .collect();
                        configured_desktop_files_missing.sort();
                        if !configured_desktop_files_missing.is_empty() {
                            sections.push((
                                "Configured desktop files missing",
                                configured_desktop_files_missing,
                            ));
                        }
                    }

                    {
                        let mut desktop_files_not_configured: Vec<String> = desktop_files
                            .iter()
                            .filter(|file_name| {
                                !configured_desktop_files.contains(file_name.as_str())
                            })
                            .cloned()
                            .collect();
                        desktop_files_not_configured.sort();
                        if !desktop_files_not_configured.is_empty() {
                            sections.push((
                                "Desktop files not in config",
                                desktop_files_not_configured,
                            ));
                        }
                    }
                }

                if let Some(kde_plasma_dock) = &config.kde_plasma_dock {
                    let system_kde_plasma_dock = steamos::get_kde_plasma_dock_apps()?;
                    if kde_plasma_dock != &system_kde_plasma_dock {
                        sections.push((
                            "KDE Plasma dock mismatch",
                            vec![
                                format!("config = {}", format_list(kde_plasma_dock)),
                                format!("system = {}", format_list(&system_kde_plasma_dock)),
                            ],
                        ));
                    }
                }

                {
                    let file_check_mismatches = file_checks::diff_file_checks(&config.files)?;
                    if !file_check_mismatches.is_empty() {
                        sections.push(("File checks mismatch", file_check_mismatches));
                    }
                }

                print_sections(sections);
            }
        },
        Commands::FileSync { command } => match command {
            FileSyncCommands::SteamOs {
                root,
                system_config,
            } => {
                let config = read_steamos_config(system_config)?;
                let report = file_checks::sync_sync_file_checks(&config.files, &root)?;
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
        },
        Commands::Completions { shell } => {
            shell.generate(&mut Cli::command(), &mut std::io::stdout());
        }
    }
    Ok(())
}
