use std::collections::HashSet;

use anyhow::{Context, Result, bail};

use crate::{
    fetchers::steamos,
    file_checks,
    pkl_types::steamos::{DeckyPlugin, DeckyStoreChannel, DeckyUpdateChannel, SteamOsConfig},
};

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
    configured: &DeckyPlugin,
    installed: &steamos::DeckyPlugin,
) -> bool {
    match configured.directory_name.as_deref() {
        Some(directory_name) => {
            installed.name == configured.name && installed.directory_name == directory_name
        }
        None => installed.name == configured.name,
    }
}

fn format_configured_decky_plugin(plugin: &DeckyPlugin) -> String {
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

pub fn generate_diff(config: SteamOsConfig) -> Result<Vec<(&'static str, Vec<String>)>> {
    let system_hostname = steamos::get_hostname()?;
    let system_charge_limit = steamos::get_charge_limit()?.unwrap_or(100);
    let user_steam_settings = steamos::get_user_steam_settings()?;
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
    let installed_flatpaks = steamos::get_installed_flatpak_apps()?;
    let system_decky_installed = steamos::is_decky_installed()?;
    let enabled_systemd_units = steamos::get_enabled_systemd_units()?;

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
        if config.steam_os_settings.steam_developer_mode != user_steam_settings.developer_mode {
            steam_os_settings_mismatches.push(format!(
                "config steamDeveloperMode = {}, system steamDeveloperMode = {}",
                config.steam_os_settings.steam_developer_mode, user_steam_settings.developer_mode,
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

        if config.steam_settings.sign_into_friends != steam_user_settings.sign_into_friends {
            steam_settings_mismatches.push(format!(
                "{} -> config signIntoFriends = {}, system signIntoFriends = {}",
                steam_account_id,
                config.steam_settings.sign_into_friends,
                steam_user_settings.sign_into_friends,
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
                != system_decky_update_channel_name(&system_decky_settings.update_channel)
            {
                decky_settings_mismatches.push(format!(
                    "config updateChannel = {}, system updateChannel = {}",
                    decky_update_channel_name(&decky.settings.update_channel),
                    system_decky_update_channel_name(&system_decky_settings.update_channel,),
                ));
            }
            if decky_store_channel_name(&decky.settings.store_channel)
                != system_decky_store_channel_name(&system_decky_settings.store_channel)
            {
                decky_settings_mismatches.push(format!(
                    "config storeChannel = {}, system storeChannel = {}",
                    decky_store_channel_name(&decky.settings.store_channel),
                    system_decky_store_channel_name(&system_decky_settings.store_channel,),
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
                        configured_decky_plugin_matches_installed(plugin, installed_plugin)
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
            let mut installed_decky_plugins_not_configured: Vec<String> = installed_decky_plugins
                .iter()
                .filter(|installed_plugin| {
                    !decky.plugins.iter().any(|plugin| {
                        configured_decky_plugin_matches_installed(plugin, installed_plugin)
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
                        configured_decky_plugin_matches_installed(plugin, installed_plugin)
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
        let configured_desktop_files: HashSet<&str> = desktop.iter().map(String::as_str).collect();

        {
            let mut configured_desktop_files_missing: Vec<String> = configured_desktop_files
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
                .filter(|file_name| !configured_desktop_files.contains(file_name.as_str()))
                .cloned()
                .collect();
            desktop_files_not_configured.sort();
            if !desktop_files_not_configured.is_empty() {
                sections.push(("Desktop files not in config", desktop_files_not_configured));
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

    Ok(sections)
}
