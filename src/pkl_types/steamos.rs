use std::{borrow::Cow, collections::HashSet};

use serde::{Deserialize, Serialize};
use validator::{Validate, ValidationError};

use crate::pkl_types::file_check::{FileCheck, validate_distinct_file_check_paths};

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SteamOsConfig {
    #[validate(length(min = 1))]
    pub hostname: String,
    #[validate(nested)]
    pub steam_os_settings: SteamOsSettings,
    pub steam_settings: SteamSettings,
    /// Flatpak application IDs excluding runtimes.
    #[validate(custom(function = "validate_distinct_strings_with_dot"))]
    pub installed_flatpaks: Vec<String>,
    /// `None` for uninstalled.
    #[validate(nested)]
    pub decky: Option<Decky>,
    /// This is not a complete list but a list of units that are enabled. Units not listed are not
    /// assumed to be disabled.
    #[validate(custom(function = "validate_distinct_strings_with_dot"))]
    pub enabled_systemd_units: Vec<String>,
    /// An exhaustive list of file names in `~/Desktop/` or `None` to not check.
    #[validate(custom(function = "validate_non_empty_distinct_strings"))]
    pub desktop: Option<Vec<String>>,
    /// An exhaustive ordered list of applications or `None` to not check. Must match KDE Plasma's
    /// `launchers` format.
    #[validate(custom(function = "validate_non_empty_distinct_strings"))]
    pub kde_plasma_dock: Option<Vec<String>>,
    #[validate(nested, custom(function = "validate_distinct_file_check_paths"))]
    pub files: Vec<FileCheck>,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct SteamOsSettings {
    /// System > Enable Developer Mode
    pub steam_developer_mode: bool,
    #[validate(range(min = 50, max = 100))]
    pub charge_limit: u8,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamSettings {
    /// System > 24-hour clock
    pub twenty_four_hour_clock: bool,
    pub sign_into_friends: bool,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Decky {
    pub settings: DeckySettings,
    #[validate(
        length(min = 1),
        nested,
        custom(function = "validate_distinct_plugins")
    )]
    pub plugins: Vec<DeckyPlugin>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeckySettings {
    pub update_channel: DeckyUpdateChannel,
    pub store_channel: DeckyStoreChannel,
    pub decky_update_notification: bool,
    pub plugin_update_notification: bool,
    pub developer_mode: bool,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DeckyPlugin {
    /// Based on `plugin.json`'s `name` field.
    #[validate(length(min = 1))]
    pub name: String,
    #[validate(custom(function = "validate_plugin_directory_name"))]
    pub directory_name: Option<String>,
    pub disabled: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeckyUpdateChannel {
    Stable,
    Prerelease,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DeckyStoreChannel {
    Default,
    Prerelease,
}

fn validate_distinct_strings_with_dot(values: &Vec<String>) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for value in values {
        if !value.contains('.') {
            let mut error = ValidationError::new("missing_dot");
            error.add_param(Cow::from("value"), &value.clone());
            return Err(error);
        }
        if !seen.insert(value.as_str()) {
            let mut error = ValidationError::new("duplicate_value");
            error.add_param(Cow::from("value"), &value.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn validate_non_empty_distinct_strings(values: &Vec<String>) -> Result<(), ValidationError> {
    if values.is_empty() {
        return Err(ValidationError::new("empty_list"));
    }

    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value.as_str()) {
            let mut error = ValidationError::new("duplicate_value");
            error.add_param(Cow::from("value"), &value.clone());
            return Err(error);
        }
    }

    Ok(())
}

fn validate_distinct_plugins(plugins: &Vec<DeckyPlugin>) -> Result<(), ValidationError> {
    let mut plugin_names = HashSet::new();
    let mut plugin_directory_names = HashSet::new();
    for plugin in plugins {
        if !plugin_names.insert(&plugin.name) {
            return Err(ValidationError::new("duplicate_plugin_name"));
        }

        if let Some(directory_name) = &plugin.directory_name
            && !plugin_directory_names.insert(directory_name)
        {
            return Err(ValidationError::new("duplicate_plugin_directory_name"));
        }
    }

    Ok(())
}

fn validate_plugin_directory_name(value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::new("empty_directory_name"));
    }

    if !value
        .chars()
        .all(|char| char.is_ascii_alphanumeric() || char == '-')
    {
        return Err(ValidationError::new("invalid_directory_name"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pkl_types::file_check::{DirectoryExists, FileEqualsString, FileExists};

    fn no_constraint_violation(value: &impl Validate) -> bool {
        value.validate().is_ok()
    }

    fn constraint_violation(value: &impl Validate) -> bool {
        value.validate().is_err()
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|&value| value.to_owned()).collect()
    }

    fn steam_os_settings() -> SteamOsSettings {
        SteamOsSettings {
            steam_developer_mode: false,
            charge_limit: 100,
        }
    }

    fn steam_settings() -> SteamSettings {
        SteamSettings {
            twenty_four_hour_clock: true,
            sign_into_friends: true,
        }
    }

    fn decky_settings() -> DeckySettings {
        DeckySettings {
            update_channel: DeckyUpdateChannel::Stable,
            store_channel: DeckyStoreChannel::Default,
            decky_update_notification: true,
            plugin_update_notification: true,
            developer_mode: false,
        }
    }

    fn decky_plugin(name: &str) -> DeckyPlugin {
        DeckyPlugin {
            name: name.to_owned(),
            directory_name: None,
            disabled: false,
        }
    }

    fn steamos() -> SteamOsConfig {
        SteamOsConfig {
            hostname: "test".to_owned(),
            steam_os_settings: steam_os_settings(),
            steam_settings: steam_settings(),
            installed_flatpaks: vec![],
            decky: None,
            enabled_systemd_units: vec![],
            desktop: None,
            kde_plasma_dock: None,
            files: vec![],
        }
    }

    // -- chargeLimit --

    #[test]
    fn allows_charge_limit() {
        let mut settings = steam_os_settings();
        settings.charge_limit = 50;
        assert!(no_constraint_violation(&settings));

        let mut settings = steam_os_settings();
        settings.charge_limit = 60;
        assert!(no_constraint_violation(&settings));

        let mut settings = steam_os_settings();
        settings.charge_limit = 100;
        assert!(no_constraint_violation(&settings));
    }

    #[test]
    fn disallows_invalid_charge_limit() {
        let mut settings = steam_os_settings();
        settings.charge_limit = 101;
        assert!(constraint_violation(&settings));
    }

    // -- installedFlatpaks --

    #[test]
    fn allows_flatpaks() {
        let mut config = steamos();
        config.installed_flatpaks = strings(&["org.mozilla.firefox"]);
        assert!(no_constraint_violation(&config));

        let mut config = steamos();
        config.installed_flatpaks = strings(&["org.mozilla.firefox", "net.lutris.Lutris"]);
        assert!(no_constraint_violation(&config));
    }

    #[test]
    fn disallows_flatpak_with_no_dot() {
        let mut config = steamos();
        config.installed_flatpaks = strings(&["firefox"]);
        assert!(constraint_violation(&config));
    }

    #[test]
    fn disallows_duplicate_flatpaks() {
        let mut config = steamos();
        config.installed_flatpaks = strings(&["org.mozilla.firefox", "org.mozilla.firefox"]);
        assert!(constraint_violation(&config));
    }

    // -- plugins --

    #[test]
    fn allows_decky_plugins() {
        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![decky_plugin("HLTB for Deck")],
        };
        assert!(no_constraint_violation(&decky));

        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![
                decky_plugin("HLTB for Deck"),
                DeckyPlugin {
                    name: "ProtonDB Badges".to_owned(),
                    directory_name: None,
                    disabled: true,
                },
            ],
        };
        assert!(no_constraint_violation(&decky));
    }

    #[test]
    fn allows_decky_plugin_directory_names() {
        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![DeckyPlugin {
                name: "HLTB for Deck".to_owned(),
                directory_name: Some("hltb-for-deck".to_owned()),
                disabled: false,
            }],
        };
        assert!(no_constraint_violation(&decky));

        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![
                DeckyPlugin {
                    name: "HLTB for Deck".to_owned(),
                    directory_name: Some("hltb-for-deck".to_owned()),
                    disabled: false,
                },
                DeckyPlugin {
                    name: "ProtonDB Badges".to_owned(),
                    directory_name: Some("protondb-decky".to_owned()),
                    disabled: true,
                },
            ],
        };
        assert!(no_constraint_violation(&decky));
    }

    #[test]
    fn disallows_invalid_decky_plugin_directory_names() {
        let plugin = DeckyPlugin {
            name: "HLTB for Deck".to_owned(),
            directory_name: Some("".to_owned()),
            disabled: false,
        };
        assert!(constraint_violation(&plugin));

        let plugin = DeckyPlugin {
            name: "HLTB for Deck".to_owned(),
            directory_name: Some("hltb_for_deck".to_owned()),
            disabled: false,
        };
        assert!(constraint_violation(&plugin));
    }

    #[test]
    fn disallows_duplicate_decky_plugins() {
        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![decky_plugin("HLTB for Deck"), decky_plugin("HLTB for Deck")],
        };
        assert!(constraint_violation(&decky));

        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![
                decky_plugin("HLTB for Deck"),
                DeckyPlugin {
                    name: "HLTB for Deck".to_owned(),
                    directory_name: None,
                    disabled: true,
                },
            ],
        };
        assert!(constraint_violation(&decky));
    }

    #[test]
    fn disallows_duplicate_decky_plugin_directory_names() {
        let decky = Decky {
            settings: decky_settings(),
            plugins: vec![
                DeckyPlugin {
                    name: "HLTB for Deck".to_owned(),
                    directory_name: Some("shared-directory".to_owned()),
                    disabled: false,
                },
                DeckyPlugin {
                    name: "ProtonDB Badges".to_owned(),
                    directory_name: Some("shared-directory".to_owned()),
                    disabled: true,
                },
            ],
        };
        assert!(constraint_violation(&decky));
    }

    // -- enabledSystemdUnits --

    #[test]
    fn allows_systemd_units() {
        let mut config = steamos();
        config.enabled_systemd_units = strings(&["sshd.service"]);
        assert!(no_constraint_violation(&config));

        let mut config = steamos();
        config.enabled_systemd_units = strings(&["sshd.service", "avahi-daemon.service"]);
        assert!(no_constraint_violation(&config));
    }

    #[test]
    fn disallows_systemd_unit_with_no_dot() {
        let mut config = steamos();
        config.enabled_systemd_units = strings(&["sshd"]);
        assert!(constraint_violation(&config));
    }

    #[test]
    fn disallows_duplicate_systemd_units() {
        let mut config = steamos();
        config.enabled_systemd_units = strings(&["sshd.service", "sshd.service"]);
        assert!(constraint_violation(&config));
    }

    // -- desktop --

    #[test]
    fn allows_desktop_entries() {
        let mut config = steamos();
        config.desktop = Some(strings(&["Return.desktop"]));
        assert!(no_constraint_violation(&config));

        let mut config = steamos();
        config.desktop = Some(strings(&["Return.desktop", "steam.desktop"]));
        assert!(no_constraint_violation(&config));
    }

    #[test]
    fn disallows_duplicate_desktop_entries() {
        let mut config = steamos();
        config.desktop = Some(strings(&["Return.desktop", "Return.desktop"]));
        assert!(constraint_violation(&config));
    }

    // -- kdePlasmaDock --

    #[test]
    fn allows_kde_plasma_dock_entries() {
        let mut config = steamos();
        config.kde_plasma_dock = Some(strings(&["Return.desktop"]));
        assert!(no_constraint_violation(&config));

        let mut config = steamos();
        config.kde_plasma_dock = Some(strings(&["Return.desktop", "steam.desktop"]));
        assert!(no_constraint_violation(&config));
    }

    #[test]
    fn disallows_duplicate_kde_plasma_dock_entries() {
        let mut config = steamos();
        config.kde_plasma_dock = Some(strings(&["Return.desktop", "Return.desktop"]));
        assert!(constraint_violation(&config));
    }

    // -- files --

    #[test]
    fn allows_distinct_file_checks() {
        let mut config = steamos();
        config.files = vec![
            FileCheck::FileExists(FileExists {
                path: "/etc/hosts".to_owned(),
            }),
            FileCheck::DirectoryExists(DirectoryExists {
                path: "/etc/".to_owned(),
            }),
        ];
        assert!(no_constraint_violation(&config));
    }

    #[test]
    fn disallows_duplicate_file_check_paths() {
        let mut config = steamos();
        config.files = vec![
            FileCheck::FileExists(FileExists {
                path: "/etc/hosts".to_owned(),
            }),
            FileCheck::FileEqualsString(FileEqualsString {
                path: "/etc/hosts".to_owned(),
                contents: "127.0.0.1 localhost\n".to_owned(),
            }),
        ];
        assert!(constraint_violation(&config));
    }
}
