use std::{collections::HashSet, env, fs, path::PathBuf, process::Command};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

pub fn get_hostname() -> Result<String> {
    let hostname = fs::read_to_string("/etc/hostname").context("Failed to read `/etc/hostname`")?;
    Ok(hostname.trim().to_owned())
}

pub fn get_charge_limit() -> Result<Option<u8>> {
    let output = Command::new("steamosctl")
        .arg("get-max-charge-level")
        .output()
        .context("Failed to run `steamosctl get-max-charge-level`")?;

    if !output.status.success() {
        bail!("`steamosctl get-max-charge-level` failed with non-zero exit code");
    }

    let stdout = String::from_utf8(output.stdout)
        .context("`steamosctl get-max-charge-level` output is not valid UTF-8")?;
    parse_charge_limit_output(&stdout)
        .context("Failed to parse `steamosctl get-max-charge-level` output")
}

fn parse_charge_limit_output(stdout: &str) -> Result<Option<u8>> {
    let line = stdout.trim_end_matches('\n');
    let raw_value = line
        .strip_prefix("Max charge level: ")
        .context("stdout did not start with `Max charge level: `")?;
    let charge_limit = raw_value
        .parse::<i8>()
        .with_context(|| format!("Found an invalid charge limit `{raw_value}`"))?;

    match charge_limit {
        -1 => Ok(None),
        1..=100 => Ok(Some(charge_limit as u8)),
        _ => bail!("found out-of-range charge limit `{charge_limit}`; expected -1 or 1..=100",),
    }
}

pub struct UserSteamSettings {
    pub developer_mode: bool,
}

/// Get settings from `~/.steam/steam/config/config.vdf`
pub fn get_user_steam_settings() -> Result<UserSteamSettings> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let config_path = home_dir.join(".steam/steam/config/config.vdf");
    let config = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read `{}`", config_path.display()))?;
    parse_user_steam_settings(&config)
        .with_context(|| format!("Failed to parse `{}`", config_path.display()))
}

fn parse_user_steam_settings(config: &str) -> Result<UserSteamSettings> {
    let vdf = steam_vdf_parser::parse_text(config).context("Config is not valid VDF")?;
    let raw_developer_mode = vdf
        .get_str(&["developer", "DevModeEnabled"])
        .context("missing `developer/DevModeEnabled`")?;

    let developer_mode = match raw_developer_mode {
        "0" => false,
        "1" => true,
        _ => bail!(
            "found invalid `developer/DevModeEnabled` value `{raw_developer_mode}`; expected `0` or `1`"
        ),
    };

    Ok(UserSteamSettings { developer_mode })
}

/// Resolves `~/.local/share/Steam/userdata/*/config/localconfig.vdf`
pub fn get_steam_user_settings_ids() -> Result<HashSet<String>> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let userdata_dir = home_dir.join(".local/share/Steam/userdata");
    let mut steam_account_ids = HashSet::new();
    for entry in fs::read_dir(&userdata_dir)
        .with_context(|| format!("Failed to read `{}`", userdata_dir.display()))?
    {
        let entry = entry
            .with_context(|| format!("Failed to read entry in `{}`", userdata_dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let localconfig_path = path.join("config/localconfig.vdf");
        if !localconfig_path.is_file() {
            continue;
        }

        let Ok(steam_account_id) = entry.file_name().into_string() else {
            continue;
        };
        if !steam_account_ids.insert(steam_account_id.clone()) {
            bail!("found duplicate Steam account ID `{steam_account_id}`");
        }
    }

    Ok(steam_account_ids)
}

pub struct SteamUserSettings {
    pub sign_into_friends: bool,
}

/// Get settings from `~/.local/share/Steam/userdata/*/config/localconfig.vdf`
pub fn get_steam_user_settings(steam_account_id: &str) -> Result<SteamUserSettings> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let config_path = home_dir.join(format!(
        ".local/share/Steam/userdata/{steam_account_id}/config/localconfig.vdf"
    ));
    let config = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read `{}`", config_path.display()))?;
    parse_steam_user_settings(&config)
        .with_context(|| format!("Failed to parse `{}`", config_path.display()))
}

fn parse_steam_user_settings(config: &str) -> Result<SteamUserSettings> {
    let vdf = steam_vdf_parser::parse_text(config).context("Config is not valid VDF")?;
    let raw_sign_into_friends = vdf
        .get_str(&["friends", "SignIntoFriends"])
        .context("missing `friends/SignIntoFriends`")?;

    let sign_into_friends = match raw_sign_into_friends {
        "0" => false,
        "1" => true,
        _ => bail!(
            "found invalid `friends/SignIntoFriends` value `{raw_sign_into_friends}`; expected `0` or `1`"
        ),
    };

    Ok(SteamUserSettings { sign_into_friends })
}

pub struct SteamClientUserSettings {
    pub twenty_four_hour_clock: bool,
}

/// Get settings from `~/.local/share/Steam/userdata/*/7/remote/sharedconfig.vdf`
pub fn get_steam_client_user_settings(steam_account_id: &str) -> Result<SteamClientUserSettings> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let config_path = home_dir.join(format!(
        ".local/share/Steam/userdata/{steam_account_id}/7/remote/sharedconfig.vdf"
    ));
    let config = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read `{}`", config_path.display()))?;
    parse_steam_client_user_settings(&config)
        .with_context(|| format!("Failed to parse `{}`", config_path.display()))
}

#[derive(Deserialize)]
struct RawSteamClientFriendsUiSettings {
    #[serde(rename = "b24HourClock")]
    twenty_four_hour_clock: bool,
}

fn parse_steam_client_user_settings(config: &str) -> Result<SteamClientUserSettings> {
    let vdf = steam_vdf_parser::parse_text(config).context("Config is not valid VDF")?;
    let raw_friends_ui_json = vdf
        .get_str(&["Software", "Valve", "Steam", "friendsui", "FriendsUIJSON"])
        .context("missing `Software/Valve/Steam/friendsui/FriendsUIJSON`")?;
    let friends_ui_settings: RawSteamClientFriendsUiSettings =
        serde_json::from_str(raw_friends_ui_json)
            .context("`Software/Valve/Steam/friendsui/FriendsUIJSON` is not valid JSON")?;

    Ok(SteamClientUserSettings {
        twenty_four_hour_clock: friends_ui_settings.twenty_four_hour_clock,
    })
}

pub fn get_installed_flatpak_apps() -> Result<HashSet<String>> {
    let output = Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
        .context("Failed to run `flatpak list --app --columns=application`")?;

    if !output.status.success() {
        bail!("`flatpak list --app --columns=application` failed with non-zero exit code");
    }

    let stdout = String::from_utf8(output.stdout)
        .context("`flatpak list --app --columns=application` output is not valid UTF-8")?;
    parse_installed_flatpak_apps_output(&stdout)
        .context("Failed to parse `flatpak list --app --columns=application` output")
}

fn parse_installed_flatpak_apps_output(stdout: &str) -> Result<HashSet<String>> {
    let lines = stdout.lines();
    let mut apps = HashSet::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !apps.insert(trimmed.to_owned()) {
            bail!("found duplicate application ID `{trimmed}`");
        }
    }

    Ok(apps)
}

/// Checks if `~/homebrew/services/PluginLoader` exists.
///
/// This is handled separately to the settings file not existing in case Decky has been uninstalled
/// (at least the binary) but user data such as settings have not been deleted.
pub fn is_decky_installed() -> Result<bool> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let plugin_loader_path = home_dir.join("homebrew/services/PluginLoader");
    Ok(plugin_loader_path.is_file())
}

#[derive(Debug, PartialEq, Eq)]
pub struct DeckySettings {
    pub update_channel: UpdateChannel,
    pub store_channel: StoreChannel,
    pub decky_update_notifications: bool,
    pub plugins_update_notifications: bool,
    pub developer_mode: bool,
    pub disabled_plugins: HashSet<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UpdateChannel {
    Stable,
    Prerelease,
}

#[derive(Debug, PartialEq, Eq)]
pub enum StoreChannel {
    Default,
    Prerelease,
}

pub fn get_decky_settings() -> Result<DeckySettings> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let settings_path = home_dir.join("homebrew/settings/loader.json");
    let settings = fs::read_to_string(&settings_path)
        .with_context(|| format!("Failed to read `{}`", settings_path.display()))?;
    parse_decky_settings(&settings).context("Failed to parse Decky settings")
}

#[derive(Deserialize)]
struct RawDeckySettings {
    branch: u8,
    store: u8,
    #[serde(rename = "notificationSettings")]
    notification_settings: RawDeckyNotificationSettings,
    #[serde(rename = "developer.enabled")]
    developer_enabled: bool,
    disabled_plugins: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawDeckyNotificationSettings {
    decky_updates: bool,
    plugin_updates: bool,
}

fn parse_decky_settings(settings: &str) -> Result<DeckySettings> {
    let settings: RawDeckySettings =
        serde_json::from_str(settings).context("Settings is not valid JSON")?;

    let update_channel = match settings.branch {
        0 => UpdateChannel::Stable,
        1 => UpdateChannel::Prerelease,
        branch => bail!("found unexpected branch value `{branch}`"),
    };

    let store_channel = match settings.store {
        0 => StoreChannel::Default,
        1 => StoreChannel::Prerelease,
        store => bail!("found unexpected store value `{store}`"),
    };

    Ok(DeckySettings {
        update_channel,
        store_channel,
        decky_update_notifications: settings.notification_settings.decky_updates,
        plugins_update_notifications: settings.notification_settings.plugin_updates,
        developer_mode: settings.developer_enabled,
        disabled_plugins: settings.disabled_plugins.into_iter().collect(),
    })
}

#[derive(Hash, PartialEq, Eq)]
pub struct DeckyPlugin {
    pub name: String,
    pub directory_name: String,
}

pub fn get_installed_decky_plugins() -> Result<HashSet<DeckyPlugin>> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let plugins_dir = home_dir.join("homebrew/plugins");
    let mut plugins = HashSet::new();
    let mut seen_plugin_names = HashSet::new();

    for entry in fs::read_dir(&plugins_dir)
        .with_context(|| format!("Failed to read `{}`", plugins_dir.display()))?
    {
        let entry = entry
            .with_context(|| format!("Failed to read entry in `{}`", plugins_dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let plugin_json_path = path.join("plugin.json");
        if !plugin_json_path.is_file() {
            bail!("expected `{}` to exist", plugin_json_path.display());
        }

        let plugin_json = fs::read_to_string(&plugin_json_path)
            .with_context(|| format!("Failed to read `{}`", plugin_json_path.display()))?;
        let plugin_name = parse_decky_plugin_manifest(&plugin_json)
            .with_context(|| format!("Failed to parse `{}`", plugin_json_path.display()))?;
        let plugin = DeckyPlugin {
            name: plugin_name.clone(),
            directory_name: entry.file_name().into_string().map_err(|entry_name| {
                anyhow!("found non-UTF-8 plugin directory name `{:?}`", entry_name)
            })?,
        };
        if !seen_plugin_names.insert(plugin_name.clone()) {
            bail!("found duplicate plugin name `{plugin_name}`");
        }
        plugins.insert(plugin);
    }

    Ok(plugins)
}

#[derive(Deserialize)]
struct RawDeckyPluginManifest {
    name: String,
}

fn parse_decky_plugin_manifest(plugin_json: &str) -> Result<String> {
    let manifest: RawDeckyPluginManifest =
        serde_json::from_str(plugin_json).context("Failed to parse JSON")?;
    Ok(manifest.name)
}

pub fn get_enabled_systemd_units() -> Result<HashSet<String>> {
    let output = Command::new("systemctl")
        .args([
            "list-unit-files",
            "--type=service",
            "--state=enabled",
            "--no-pager",
        ])
        .output()
        .context("Failed to run `systemctl list-unit-files --type=service --state=enabled`")?;

    if !output.status.success() {
        bail!(
            "`systemctl list-unit-files --type=service --state=enabled` failed with non-zero exit code"
        );
    }

    let stdout = String::from_utf8(output.stdout).context(
        "`systemctl list-unit-files --type=service --state=enabled` output is not valid UTF-8",
    )?;
    parse_enabled_systemd_units_output(&stdout).context(
        "Failed to parse `systemctl list-unit-files --type=service --state=enabled` output",
    )
}

fn parse_enabled_systemd_units_output(stdout: &str) -> Result<HashSet<String>> {
    let mut lines = stdout.lines();
    let header = lines.next().context("stdout is empty")?;
    let header_columns = header.split_whitespace().collect::<Vec<_>>();
    if header_columns.len() < 3
        || header_columns[0] != "UNIT"
        || header_columns[1] != "FILE"
        || header_columns[2] != "STATE"
    {
        bail!("unexpected header `{header}`");
    }

    let mut units = HashSet::new();
    loop {
        let line = lines.next().context("stdout is missing count line")?;

        if line.trim().is_empty() {
            let count_line = lines.next().context("stdout is missing count line")?;
            let count = parse_systemd_unit_count_line(count_line)?
                .context("found unexpected line after unit list separator")?;
            if lines.next().is_some() {
                bail!("found unexpected content after the count line");
            }
            if count != units.len() {
                bail!("found count `{count}` but parsed `{}` units", units.len());
            }
            return Ok(units);
        }

        if parse_systemd_unit_count_line(line)?.is_some() {
            bail!("missing empty line before count line");
        }

        let unit = parse_enabled_systemd_unit_line(line)?;
        if !units.insert(unit.clone()) {
            bail!("found duplicate unit file `{unit}`");
        }
    }
}

fn parse_enabled_systemd_unit_line(line: &str) -> Result<String> {
    let columns = line.split_whitespace().collect::<Vec<_>>();
    if columns.len() < 2 {
        bail!("found unexpected line `{line}`");
    }

    let [unit_file, state] = [columns[0], columns[1]];
    if state != "enabled" {
        bail!("found unexpected state `{state}`");
    }

    Ok(unit_file.to_owned())
}

fn parse_systemd_unit_count_line(line: &str) -> Result<Option<usize>> {
    let words = line.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        bail!("found unexpected blank line");
    }
    if words.len() != 4 {
        return Ok(None);
    }
    if words[1] != "unit" || words[2] != "files" || words[3] != "listed." {
        return Ok(None);
    }

    let count = words[0]
        .parse::<usize>()
        .with_context(|| format!("found invalid count `{}`", words[0]))?;
    Ok(Some(count))
}

pub fn get_desktop_files() -> Result<HashSet<String>> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let desktop_dir = home_dir.join("Desktop");
    let mut entries = HashSet::new();

    for entry in fs::read_dir(&desktop_dir)
        .with_context(|| format!("Failed to read `{}`", desktop_dir.display()))?
    {
        let entry = entry
            .with_context(|| format!("Failed to read entry in `{}`", desktop_dir.display()))?;
        let entry_name = entry
            .file_name()
            .into_string()
            .map_err(|entry_name| anyhow!("found non-UTF-8 entry name `{:?}`", entry_name))?;

        if !entries.insert(entry_name.clone()) {
            bail!("found duplicate desktop entry `{entry_name}`");
        }
    }

    Ok(entries)
}

pub fn get_kde_plasma_dock_apps() -> Result<Vec<String>> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();
    let config_path = home_dir.join(".config/plasma-org.kde.plasma.desktop-appletsrc");
    let config = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read `{}`", config_path.display()))?;
    parse_kde_plasma_desktop_config(&config)
        .with_context(|| format!("Failed to parse `{}`", config_path.display()))
}

fn parse_kde_plasma_desktop_config(config: &str) -> Result<Vec<String>> {
    let mut launchers = None;

    for line in config.lines() {
        let Some(raw_launchers) = line.strip_prefix("launchers=") else {
            continue;
        };

        if launchers.is_some() {
            bail!("found multiple `launchers=` lines");
        }

        launchers = Some(parse_kde_plasma_launchers(raw_launchers)?);
    }

    launchers.context("Config did not contain a `launchers=` line")
}

fn parse_kde_plasma_launchers(raw_launchers: &str) -> Result<Vec<String>> {
    if raw_launchers.is_empty() {
        return Ok(Vec::new());
    }

    let mut launchers = Vec::new();
    for launcher in raw_launchers.split(',') {
        if launcher.is_empty() {
            bail!("found empty launcher entry");
        }
        launchers.push(launcher.to_owned());
    }

    Ok(launchers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_charge_limit_output_valid() {
        assert_eq!(
            parse_charge_limit_output("Max charge level: 75\n").unwrap(),
            Some(75),
        );
    }

    #[test]
    fn test_parse_charge_limit_output_none() {
        assert_eq!(
            parse_charge_limit_output("Max charge level: -1\n").unwrap(),
            None,
        );
    }

    #[test]
    fn test_parse_charge_limit_output_out_of_range() {
        assert!(parse_charge_limit_output("Max charge level: 101\n").is_err());
    }

    #[test]
    fn test_parse_charge_limit_output_incorrect_prefix() {
        assert!(parse_charge_limit_output("charge level: 75\n").is_err());
    }

    #[test]
    fn test_parse_charge_limit_output_trailing_characters_after_number() {
        assert!(parse_charge_limit_output("Max charge level: 75a\n").is_err());
    }

    #[test]
    fn test_parse_charge_limit_output_trailing_characters() {
        assert!(parse_charge_limit_output("Max charge level: 75\na\n").is_err());
    }

    #[test]
    fn test_parse_user_steam_settings_valid() {
        let settings = parse_user_steam_settings(concat!(
            r#""InstallConfigStore""#,
            "\n",
            r#"{"#,
            "\n",
            r#"    "developer""#,
            "\n",
            r#"    {"#,
            "\n",
            r#"        "DevModeEnabled"                "1""#,
            "\n",
            r#"    }"#,
            "\n",
            r#"}"#,
            "\n",
        ))
        .unwrap();

        assert!(settings.developer_mode);
    }

    #[test]
    fn test_parse_user_steam_settings_invalid_value() {
        assert!(
            parse_user_steam_settings(concat!(
                r#""InstallConfigStore""#,
                "\n",
                r#"{"#,
                "\n",
                r#"    "developer""#,
                "\n",
                r#"    {"#,
                "\n",
                r#"        "DevModeEnabled"                "2""#,
                "\n",
                r#"    }"#,
                "\n",
                r#"}"#,
                "\n",
            ))
            .is_err(),
        );
    }

    #[test]
    fn test_parse_user_steam_settings_missing_value() {
        assert!(
            parse_user_steam_settings(concat!(
                r#""InstallConfigStore""#,
                "\n",
                r#"{"#,
                "\n",
                r#"    "a""#,
                "\n",
                r#"    {"#,
                "\n",
                r#"        "b"                "0""#,
                "\n",
                r#"    }"#,
                "\n",
                r#"}"#,
                "\n",
            ))
            .is_err(),
        );
    }

    #[test]
    fn test_parse_steam_user_settings_valid() {
        let settings = parse_steam_user_settings(concat!(
            r#""UserLocalConfigStore""#,
            "\n",
            r#"{"#,
            "\n",
            r#"    "friends""#,
            "\n",
            r#"    {"#,
            "\n",
            r#"        "SignIntoFriends"        "0""#,
            "\n",
            r#"    }"#,
            "\n",
            r#"}"#,
            "\n",
        ))
        .unwrap();

        assert!(!settings.sign_into_friends);
    }

    #[test]
    fn test_parse_steam_user_settings_invalid_value() {
        assert!(
            parse_steam_user_settings(concat!(
                r#""UserLocalConfigStore""#,
                "\n",
                r#"{"#,
                "\n",
                r#"    "friends""#,
                "\n",
                r#"    {"#,
                "\n",
                r#"        "SignIntoFriends"        "2""#,
                "\n",
                r#"    }"#,
                "\n",
                r#"}"#,
                "\n",
            ))
            .is_err(),
        );
    }

    #[test]
    fn test_parse_steam_user_settings_missing_value() {
        assert!(
            parse_steam_user_settings(concat!(
                r#""UserLocalConfigStore""#,
                "\n",
                r#"{"#,
                "\n",
                r#"    "a""#,
                "\n",
                r#"    {"#,
                "\n",
                r#"        "b"        "0""#,
                "\n",
                r#"    }"#,
                "\n",
                r#"}"#,
                "\n",
            ))
            .is_err(),
        );
    }

    #[test]
    fn test_parse_steam_client_user_settings_valid() {
        let settings = parse_steam_client_user_settings(concat!(
            r#""UserRoamingConfigStore""#,
            "\n",
            r#"{"#,
            "\n",
            r#"    "Software""#,
            "\n",
            r#"    {"#,
            "\n",
            r#"        "Valve""#,
            "\n",
            r#"        {"#,
            "\n",
            r#"            "Steam""#,
            "\n",
            r#"            {"#,
            "\n",
            r#"                "friendsui""#,
            "\n",
            r#"                {"#,
            "\n",
            r#"                    "FriendsUIJSON"         "{\"b24HourClock\":true}""#,
            "\n",
            r#"                }"#,
            "\n",
            r#"            }"#,
            "\n",
            r#"        }"#,
            "\n",
            r#"    }"#,
            "\n",
            r#"}"#,
            "\n",
        ))
        .unwrap();

        assert!(settings.twenty_four_hour_clock);
    }

    #[test]
    fn test_parse_steam_client_user_settings_invalid_value() {
        assert!(
            parse_steam_client_user_settings(concat!(
                r#""UserRoamingConfigStore""#,
                "\n",
                r#"{"#,
                "\n",
                r#"    "Software""#,
                "\n",
                r#"    {"#,
                "\n",
                r#"        "Valve""#,
                "\n",
                r#"        {"#,
                "\n",
                r#"            "Steam""#,
                "\n",
                r#"            {"#,
                "\n",
                r#"                "friendsui""#,
                "\n",
                r#"                {"#,
                "\n",
                r#"                    "FriendsUIJSON"         "{\"b24HourClock\":1}""#,
                "\n",
                r#"                }"#,
                "\n",
                r#"            }"#,
                "\n",
                r#"        }"#,
                "\n",
                r#"    }"#,
                "\n",
                r#"}"#,
                "\n",
            ))
            .is_err(),
        );
    }

    #[test]
    fn test_parse_steam_client_user_settings_missing_value() {
        assert!(
            parse_steam_client_user_settings(concat!(
                r#""UserRoamingConfigStore""#,
                "\n",
                r#"{"#,
                "\n",
                r#"    "Software""#,
                "\n",
                r#"    {"#,
                "\n",
                r#"        "Valve""#,
                "\n",
                r#"        {"#,
                "\n",
                r#"            "Steam""#,
                "\n",
                r#"            {"#,
                "\n",
                r#"                "a""#,
                "\n",
                r#"                {"#,
                "\n",
                r#"                    "b"         "{\"b24HourClock\":true}""#,
                "\n",
                r#"                }"#,
                "\n",
                r#"            }"#,
                "\n",
                r#"        }"#,
                "\n",
                r#"    }"#,
                "\n",
                r#"}"#,
                "\n",
            ))
            .is_err(),
        );
    }

    #[test]
    fn test_parse_installed_flatpak_apps_output_valid() {
        assert_eq!(
            parse_installed_flatpak_apps_output(
                "com.github.Matoking.protontricks\norg.mozilla.firefox\n"
            )
            .unwrap(),
            HashSet::from([
                "com.github.Matoking.protontricks".to_owned(),
                "org.mozilla.firefox".to_owned(),
            ]),
        );
    }

    #[test]
    fn test_parse_installed_flatpak_apps_output_empty() {
        assert_eq!(
            parse_installed_flatpak_apps_output("\n").unwrap(),
            HashSet::<String>::new(),
        );
    }

    #[test]
    fn test_parse_installed_flatpak_apps_output_duplicate_application_id() {
        assert!(
            parse_installed_flatpak_apps_output(
                "com.github.Matoking.protontricks\ncom.github.Matoking.protontricks\n",
            )
            .is_err()
        );
    }

    #[test]
    fn test_parse_decky_settings_valid() {
        assert_eq!(
            parse_decky_settings(
                r#"{
                    "branch": 0,
                    "store": 1,
                    "developer.enabled": true,
                    "notificationSettings": {
                        "deckyUpdates": false,
                        "pluginUpdates": true
                    },
                    "disabled_plugins": ["a", "a", "b"]
                }"#,
            )
            .unwrap(),
            DeckySettings {
                update_channel: UpdateChannel::Stable,
                store_channel: StoreChannel::Prerelease,
                decky_update_notifications: false,
                plugins_update_notifications: true,
                developer_mode: true,
                disabled_plugins: HashSet::from(["a".to_owned(), "b".to_owned()]),
            },
        );
    }

    #[test]
    fn test_parse_decky_settings_invalid_branch() {
        assert!(
            parse_decky_settings(
                r#"{
                    "branch": 2,
                    "store": 0,
                    "developer.enabled": true,
                    "notificationSettings": {
                        "deckyUpdates": false,
                        "pluginUpdates": true
                    },
                    "disabled_plugins": []
                }"#,
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_decky_settings_invalid_store() {
        assert!(
            parse_decky_settings(
                r#"{
                    "branch": 0,
                    "store": 2,
                    "developer.enabled": true,
                    "notificationSettings": {
                        "deckyUpdates": false,
                        "pluginUpdates": true
                    },
                    "disabled_plugins": []
                }"#,
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_decky_plugin_manifest_valid() {
        assert_eq!(
            parse_decky_plugin_manifest(
                r#"{
                    "name": "ProtonDB Badges"
                }"#,
            )
            .unwrap(),
            "ProtonDB Badges",
        );
    }

    #[test]
    fn test_parse_decky_plugin_manifest_missing_name() {
        assert!(
            parse_decky_plugin_manifest(
                r#"{
                    "flags": []
                }"#,
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_valid() {
        assert_eq!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   WHATEVER\nbluetooth.service                  enabled disabled\nsystemd-timesyncd.service          enabled nonsense\n\n2 unit files listed.\n",
            )
            .unwrap(),
            HashSet::from([
                "bluetooth.service".to_owned(),
                "systemd-timesyncd.service".to_owned(),
            ]),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_missing_empty_line_before_count() {
        assert!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   WHATEVER\nbluetooth.service                  enabled disabled\nsystemd-timesyncd.service          enabled nonsense\n2 unit files listed.\n",
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_empty() {
        assert_eq!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   PRESET\n\n0 unit files listed.\n",
            )
            .unwrap(),
            HashSet::<String>::new(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_invalid_header() {
        assert!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATUS  PRESET\navahi-daemon.service               enabled disabled\n\n1 unit files listed.\n",
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_invalid_state() {
        assert!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   PRESET\navahi-daemon.service               disabled disabled\n\n1 unit files listed.\n",
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_ignores_trailing_columns() {
        assert_eq!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   PRESET\navahi-daemon.service               enabled static\n\n1 unit files listed.\n",
            )
            .unwrap(),
            HashSet::from(["avahi-daemon.service".to_owned()]),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_count_mismatch() {
        assert!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   PRESET\navahi-daemon.service               enabled disabled\n\n2 unit files listed.\n",
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_extra_lines_after_count() {
        assert!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   PRESET\navahi-daemon.service               enabled disabled\n\n1 unit files listed.\nextra\n",
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_enabled_systemd_units_output_duplicate_unit_file() {
        assert!(
            parse_enabled_systemd_units_output(
                "UNIT FILE                          STATE   PRESET\navahi-daemon.service               enabled disabled\navahi-daemon.service               enabled enabled\n\n2 unit files listed.\n",
            )
            .is_err(),
        );
    }

    #[test]
    fn test_parse_kde_plasma_dock_apps_valid() {
        assert_eq!(
            parse_kde_plasma_desktop_config(
                "[Containments][1][Applets][2][Configuration][General]\nlaunchers=applications:systemsettings.desktop,applications:org.kde.discover.desktop,preferred://filemanager,preferred://browser\n",
            )
            .unwrap(),
            vec![
                "applications:systemsettings.desktop".to_owned(),
                "applications:org.kde.discover.desktop".to_owned(),
                "preferred://filemanager".to_owned(),
                "preferred://browser".to_owned(),
            ],
        );
    }

    #[test]
    fn test_parse_kde_plasma_dock_apps_missing_launchers() {
        assert!(parse_kde_plasma_desktop_config("[Containments][1]\n").is_err());
    }

    #[test]
    fn test_parse_kde_plasma_dock_apps_multiple_launchers_lines() {
        assert!(parse_kde_plasma_desktop_config("launchers=applications:org.kde.discover.desktop\nlaunchers=applications:systemsettings.desktop\n").is_err());
    }

    #[test]
    fn test_parse_kde_plasma_dock_apps_empty_launchers() {
        assert_eq!(
            parse_kde_plasma_desktop_config("launchers=\n").unwrap(),
            Vec::<String>::new()
        );
    }
}
