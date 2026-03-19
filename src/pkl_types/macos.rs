use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashSet};
use validator::{Validate, ValidationError, ValidationErrors};

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
#[validate(schema(function = "validate_macos_config"))]
pub struct MacOsConfig {
    pub install_homebrew: bool,
    #[validate(length(min = 1), nested)]
    pub apps: Vec<MacOsApp>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum MacOsApp {
    ManualApp(ManualApp),
    HomebrewCask(HomebrewCaskApp),
    MacAppStoreApp(MacAppStoreApp),
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct BaseMacOsApp {
    #[validate(length(min = 1), custom(function = "validate_app_paths"))]
    pub app_paths: Vec<String>,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct ManualApp {
    #[serde(flatten)]
    #[validate(nested)]
    pub base: BaseMacOsApp,
    #[validate(length(min = 1))]
    pub name: String,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct HomebrewCaskApp {
    #[serde(flatten)]
    #[validate(nested)]
    pub base: BaseMacOsApp,
    #[validate(custom(function = "validate_cask_name"))]
    pub cask_name: String,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
#[validate(schema(function = "validate_app_store_app"))]
pub struct MacAppStoreApp {
    #[serde(flatten)]
    #[validate(nested)]
    pub base: BaseMacOsApp,
    pub app_store_id: u64,
}

impl Validate for MacOsApp {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Self::ManualApp(app) => app.validate(),
            Self::HomebrewCask(app) => app.validate(),
            Self::MacAppStoreApp(app) => app.validate(),
        }
    }
}

fn validate_app_paths(app_paths: &Vec<String>) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for app_path in app_paths {
        if !is_valid_mac_app_path(app_path) {
            let mut error = ValidationError::new("invalid_app_path");
            error.add_param(Cow::from("value"), &app_path.clone());
            return Err(error);
        }
        if !seen.insert(app_path) {
            let mut error = ValidationError::new("duplicate_app_path");
            error.add_param(Cow::from("value"), &app_path.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn validate_cask_name(cask_name: &str) -> Result<(), ValidationError> {
    if is_valid_cask_name(cask_name) {
        return Ok(());
    }
    let mut error = ValidationError::new("invalid_cask_name");
    error.add_param(Cow::from("value"), &cask_name.to_string());
    Err(error)
}

fn validate_app_store_app(app: &MacAppStoreApp) -> Result<(), ValidationError> {
    if app.base.app_paths.len() == 1 {
        return Ok(());
    }

    Err(ValidationError::new("app_store_requires_single_app_path"))
}

fn validate_macos_config(config: &MacOsConfig) -> Result<(), ValidationError> {
    let mut all_app_paths = HashSet::new();
    let mut cask_names = HashSet::new();
    let mut app_store_ids = HashSet::new();

    for app in &config.apps {
        match app {
            MacOsApp::ManualApp(manual_app) => {
                for app_path in &manual_app.base.app_paths {
                    if !all_app_paths.insert(app_path) {
                        return Err(ValidationError::new("duplicate_app_path"));
                    }
                }
            }
            MacOsApp::HomebrewCask(cask) => {
                if !config.install_homebrew {
                    return Err(ValidationError::new(
                        "homebrew_cask_requires_install_homebrew",
                    ));
                }
                if !cask_names.insert(&cask.cask_name) {
                    return Err(ValidationError::new("duplicate_cask_name"));
                }
                for app_path in &cask.base.app_paths {
                    if !all_app_paths.insert(app_path) {
                        return Err(ValidationError::new("duplicate_app_path"));
                    }
                }
            }
            MacOsApp::MacAppStoreApp(app_store_app) => {
                if !app_store_ids.insert(app_store_app.app_store_id) {
                    return Err(ValidationError::new("duplicate_app_store_id"));
                }
                for app_path in &app_store_app.base.app_paths {
                    if !all_app_paths.insert(app_path) {
                        return Err(ValidationError::new("duplicate_app_path"));
                    }
                }
            }
        }
    }

    Ok(())
}

fn is_valid_mac_app_path(app_path: &str) -> bool {
    (app_path.starts_with("/Applications/") || app_path.starts_with("~/Applications/"))
        && !app_path.ends_with('/')
}

fn is_valid_cask_name(cask_name: &str) -> bool {
    if cask_name.is_empty() || cask_name.contains("--") {
        return false;
    }

    let mut at_count = 0;
    for c in cask_name.chars() {
        if c == '@' {
            at_count += 1;
        }
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '@') {
            return false;
        }
    }

    if at_count > 1 {
        return false;
    }

    let first = cask_name.chars().next().unwrap();
    let last = cask_name.chars().last().unwrap();

    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && (last.is_ascii_lowercase() || last.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_constraint_violation(value: &impl Validate) -> bool {
        value.validate().is_ok()
    }

    fn constraint_violation(value: &impl Validate) -> bool {
        value.validate().is_err()
    }

    fn base_app(paths: &[&str]) -> BaseMacOsApp {
        BaseMacOsApp {
            app_paths: paths.iter().map(|path| path.to_string()).collect(),
        }
    }

    fn cask(cask_name: &str, paths: &[&str]) -> HomebrewCaskApp {
        HomebrewCaskApp {
            base: base_app(paths),
            cask_name: cask_name.to_string(),
        }
    }

    fn app_store(app_store_id: u64, paths: &[&str]) -> MacAppStoreApp {
        MacAppStoreApp {
            base: base_app(paths),
            app_store_id,
        }
    }

    fn manual(name: &str, paths: &[&str]) -> ManualApp {
        ManualApp {
            base: base_app(paths),
            name: name.to_string(),
        }
    }

    fn macos(install_homebrew: bool, apps: Vec<MacOsApp>) -> MacOsConfig {
        MacOsConfig {
            install_homebrew,
            apps,
        }
    }

    #[test]
    fn allows_valid_cask() {
        assert!(no_constraint_violation(&cask(
            "visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
        assert!(no_constraint_violation(&cask(
            "visual-studio-code",
            &["~/Applications/Visual Studio Code.app"]
        )));
        assert!(no_constraint_violation(&cask(
            "visual-studio-code@insiders",
            &["/Applications/Visual Studio Code - Insiders.app"]
        )));
        assert!(no_constraint_violation(&cask(
            "a",
            &["/Applications/a.app"]
        )));
    }

    #[test]
    fn disallows_exclamation_mark_in_cask_name() {
        assert!(constraint_violation(&cask(
            "visual-studio-code!",
            &["/Applications/Visual Studio Code.app"]
        )));
        assert!(constraint_violation(&cask(
            "!visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
        assert!(constraint_violation(&cask(
            "visual!-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn disallows_leading_and_trailing_dash_in_cask_name() {
        assert!(constraint_violation(&cask(
            "-visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
        assert!(constraint_violation(&cask(
            "visual-studio-code-",
            &["/Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn disallows_leading_and_trailing_at_in_cask_name() {
        assert!(constraint_violation(&cask(
            "@visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
        assert!(constraint_violation(&cask(
            "visual-studio-code@",
            &["/Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn disallows_multiple_at_in_cask_name() {
        assert!(constraint_violation(&cask(
            "visual-studio-code@@insiders",
            &["/Applications/Visual Studio Code - Insiders.app"]
        )));
        assert!(constraint_violation(&cask(
            "visual-studio-code@i@nsiders",
            &["/Applications/Visual Studio Code - Insiders.app"]
        )));
    }

    #[test]
    fn disallows_double_dash_in_cask_name() {
        assert!(constraint_violation(&cask(
            "visual-studio--code",
            &["/Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn disallows_trailing_slash_in_app_path() {
        assert!(constraint_violation(&cask(
            "visual-studio-code",
            &["/Applications/Visual Studio Code.app/"]
        )));
    }

    #[test]
    fn disallows_relative_app_path() {
        assert!(constraint_violation(&cask(
            "visual-studio-code",
            &["Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn allows_cask_with_install_homebrew() {
        assert!(no_constraint_violation(&macos(
            true,
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
        assert!(no_constraint_violation(&macos(
            true,
            vec![
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::MacAppStoreApp(app_store(
                    1,
                    &["/Applications/Visual Studio Code - Insiders.app"]
                )),
            ]
        )));
    }

    #[test]
    fn allows_non_cask_with_install_homebrew() {
        assert!(no_constraint_violation(&macos(
            true,
            vec![MacOsApp::MacAppStoreApp(app_store(
                1,
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
    }

    #[test]
    fn disallows_cask_with_no_install_homebrew() {
        assert!(constraint_violation(&macos(
            false,
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
        assert!(constraint_violation(&macos(
            false,
            vec![
                MacOsApp::MacAppStoreApp(app_store(1, &["/Applications/Visual Studio Code.app"])),
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
            ]
        )));
    }

    #[test]
    fn allows_non_cask_with_no_install_homebrew() {
        assert!(no_constraint_violation(&macos(
            false,
            vec![MacOsApp::MacAppStoreApp(app_store(
                1,
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
    }

    #[test]
    fn disallows_empty_apps() {
        assert!(constraint_violation(&macos(true, vec![])));
        assert!(constraint_violation(&macos(false, vec![])));
    }

    #[test]
    fn disallows_duplicate_app_paths() {
        assert!(constraint_violation(&macos(
            true,
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &[
                    "/Applications/Visual Studio Code.app",
                    "/Applications/Visual Studio Code.app",
                ]
            ))]
        )));
        assert!(constraint_violation(&macos(
            true,
            vec![
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code-2",
                    &["/Applications/Visual Studio Code.app"]
                )),
            ]
        )));
        assert!(constraint_violation(&macos(
            true,
            vec![
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::MacAppStoreApp(app_store(1, &["/Applications/Visual Studio Code.app"])),
            ]
        )));
    }

    #[test]
    fn disallows_duplicate_cask_names() {
        assert!(constraint_violation(&macos(
            true,
            vec![
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code - Insiders.app"]
                )),
            ]
        )));
    }

    #[test]
    fn disallows_duplicate_app_store_ids() {
        assert!(constraint_violation(&macos(
            true,
            vec![
                MacOsApp::MacAppStoreApp(app_store(1, &["/Applications/Visual Studio Code.app"])),
                MacOsApp::MacAppStoreApp(app_store(
                    1,
                    &["/Applications/Visual Studio Code - Insiders.app"]
                )),
            ]
        )));
    }

    #[test]
    fn disallows_app_store_app_with_multiple_app_paths() {
        assert!(constraint_violation(&app_store(
            1,
            &[
                "/Applications/Visual Studio Code.app",
                "/Applications/Visual Studio Code - Insiders.app",
            ]
        )));
    }

    #[test]
    fn allows_cask_with_multiple_app_paths() {
        assert!(no_constraint_violation(&cask(
            "visual-studio-code",
            &[
                "/Applications/Visual Studio Code.app",
                "/Applications/Visual Studio Code - Insiders.app",
            ]
        )));
    }

    #[test]
    fn allows_manual_app_with_multiple_app_paths() {
        assert!(no_constraint_violation(&manual(
            "Visual Studio Code",
            &[
                "/Applications/Visual Studio Code.app",
                "/Applications/Visual Studio Code - Insiders.app",
            ]
        )));
    }

    #[test]
    fn disallows_manual_app_with_empty_name() {
        assert!(constraint_violation(&manual(
            "",
            &["/Applications/Visual Studio Code.app"]
        )));
    }
}
