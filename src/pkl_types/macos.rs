use serde::{Deserialize, Serialize};
use std::{borrow::Cow, collections::HashSet};
use validator::{Validate, ValidationError, ValidationErrors};

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
#[validate(schema(function = "validate_macos_config"))]
pub struct MacOsConfig {
    #[validate(nested)]
    pub homebrew: Option<Homebrew>,
    #[validate(length(min = 1), nested)]
    pub apps: Vec<MacOsApp>,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct Homebrew {
    #[validate(custom(function = "validate_taps"))]
    pub taps: Vec<String>,
    #[validate(custom(function = "validate_formula_names"))]
    pub explicitly_installed_formulae: Vec<String>,
    #[validate(custom(function = "validate_cask_names"))]
    pub non_app_casks: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum MacOsApp {
    ManualApp(ManualApp),
    HomebrewCask(HomebrewCaskApp),
    MacAppStoreApp(MacAppStoreApp),
    TestFlightApp(TestFlightApp),
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
    #[validate(range(min = 1))]
    pub app_store_id: u64,
}

#[derive(Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
#[validate(schema(function = "validate_testflight_app"))]
pub struct TestFlightApp {
    #[serde(flatten)]
    #[validate(nested)]
    pub base: BaseMacOsApp,
    #[validate(length(min = 1))]
    pub name: String,
}

impl Validate for MacOsApp {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Self::ManualApp(app) => app.validate(),
            Self::HomebrewCask(app) => app.validate(),
            Self::MacAppStoreApp(app) => app.validate(),
            Self::TestFlightApp(app) => app.validate(),
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

fn validate_cask_names(cask_names: &Vec<String>) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for cask_name in cask_names {
        validate_cask_name(cask_name)?;
        if !seen.insert(package_name_end(cask_name)) {
            let mut error = ValidationError::new("duplicate_cask_name");
            error.add_param(Cow::from("value"), &cask_name.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn validate_formula_name(formula_name: &str) -> Result<(), ValidationError> {
    if is_valid_formula_name(formula_name) {
        return Ok(());
    }
    let mut error = ValidationError::new("invalid_formula_name");
    error.add_param(Cow::from("value"), &formula_name.to_string());
    Err(error)
}

fn validate_formula_names(formula_names: &Vec<String>) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for formula_name in formula_names {
        validate_formula_name(formula_name)?;
        if !seen.insert(package_name_end(formula_name)) {
            let mut error = ValidationError::new("duplicate_formula_name");
            error.add_param(Cow::from("value"), &formula_name.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn validate_taps(taps: &Vec<String>) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for tap in taps {
        if tap.matches('/').count() != 1 {
            let mut error = ValidationError::new("invalid_tap");
            error.add_param(Cow::from("value"), &tap.clone());
            return Err(error);
        }
        if !seen.insert(tap) {
            let mut error = ValidationError::new("duplicate_tap");
            error.add_param(Cow::from("value"), &tap.clone());
            return Err(error);
        }
    }
    Ok(())
}

fn validate_app_store_app(app: &MacAppStoreApp) -> Result<(), ValidationError> {
    if app.base.app_paths.len() == 1 {
        return Ok(());
    }

    Err(ValidationError::new("app_store_requires_single_app_path"))
}

fn validate_testflight_app(app: &TestFlightApp) -> Result<(), ValidationError> {
    if app.base.app_paths.len() == 1 {
        return Ok(());
    }

    Err(ValidationError::new("testflight_requires_single_app_path"))
}

fn validate_macos_config(config: &MacOsConfig) -> Result<(), ValidationError> {
    let mut all_app_paths = HashSet::new();
    let mut cask_names = HashSet::new();
    let mut app_store_ids = HashSet::new();
    let mut manual_and_testflight_names = HashSet::new();

    if let Some(homebrew) = &config.homebrew {
        cask_names.extend(
            homebrew
                .non_app_casks
                .iter()
                .map(|cask_name| package_name_end(cask_name)),
        );
    }

    for app in &config.apps {
        match app {
            MacOsApp::ManualApp(manual_app) => {
                if !manual_and_testflight_names.insert(&manual_app.name) {
                    return Err(ValidationError::new("duplicate_name"));
                }
                for app_path in &manual_app.base.app_paths {
                    if !all_app_paths.insert(app_path) {
                        return Err(ValidationError::new("duplicate_app_path"));
                    }
                }
            }
            MacOsApp::HomebrewCask(cask) => {
                if config.homebrew.is_none() {
                    return Err(ValidationError::new("homebrew_cask_requires_homebrew"));
                }
                if !cask_names.insert(package_name_end(&cask.cask_name)) {
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
            MacOsApp::TestFlightApp(testflight_app) => {
                if !manual_and_testflight_names.insert(&testflight_app.name) {
                    return Err(ValidationError::new("duplicate_name"));
                }
                for app_path in &testflight_app.base.app_paths {
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
        && app_path.ends_with(".app")
}

fn package_name_end(package_name: &str) -> &str {
    package_name.rsplit('/').next().unwrap_or(package_name)
}

fn is_valid_cask_name(cask_name: &str) -> bool {
    is_valid_homebrew_package_name(cask_name, false)
}

fn is_valid_formula_name(formula_name: &str) -> bool {
    is_valid_homebrew_package_name(formula_name, true)
}

fn is_valid_homebrew_package_name(package_name: &str, allow_underscore: bool) -> bool {
    let slash_count = package_name.matches('/').count();
    if slash_count != 0 && slash_count != 2 {
        return false;
    }

    let package_name = if slash_count == 2 {
        let mut parts = package_name.split('/');
        let Some(user) = parts.next() else {
            return false;
        };
        let Some(repo) = parts.next() else {
            return false;
        };
        let Some(package_name) = parts.next() else {
            return false;
        };
        if user.is_empty() || repo.is_empty() {
            return false;
        }
        package_name
    } else {
        package_name
    };

    if package_name.is_empty() || package_name.contains("--") || package_name.contains("__") {
        return false;
    }

    let mut at_count = 0;
    for c in package_name.chars() {
        if c == '@' {
            at_count += 1;
        }
        if !(c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || c == '-'
            || c == '@'
            || (allow_underscore && c == '_'))
        {
            return false;
        }
    }

    if at_count > 1 {
        return false;
    }

    let first = package_name.chars().next().unwrap();
    let last = package_name.chars().last().unwrap();

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
            app_paths: paths.iter().map(|&path| path.to_owned()).collect(),
        }
    }

    fn manual(name: &str, paths: &[&str]) -> ManualApp {
        ManualApp {
            base: base_app(paths),
            name: name.to_owned(),
        }
    }

    fn cask(cask_name: &str, paths: &[&str]) -> HomebrewCaskApp {
        HomebrewCaskApp {
            base: base_app(paths),
            cask_name: cask_name.to_owned(),
        }
    }

    fn app_store(app_store_id: u64, paths: &[&str]) -> MacAppStoreApp {
        MacAppStoreApp {
            base: base_app(paths),
            app_store_id,
        }
    }

    fn testflight(name: &str, paths: &[&str]) -> TestFlightApp {
        TestFlightApp {
            base: base_app(paths),
            name: name.to_owned(),
        }
    }

    fn homebrew() -> Homebrew {
        Homebrew {
            taps: Vec::new(),
            explicitly_installed_formulae: Vec::new(),
            non_app_casks: Vec::new(),
        }
    }

    fn homebrew_with_taps(taps: &[&str]) -> Homebrew {
        Homebrew {
            taps: taps.iter().map(|&tap| tap.to_owned()).collect(),
            explicitly_installed_formulae: Vec::new(),
            non_app_casks: Vec::new(),
        }
    }

    fn homebrew_with_non_app_casks(cask_names: &[&str]) -> Homebrew {
        Homebrew {
            taps: Vec::new(),
            explicitly_installed_formulae: Vec::new(),
            non_app_casks: cask_names
                .iter()
                .map(|&cask_name| cask_name.to_owned())
                .collect(),
        }
    }

    fn homebrew_with_formulae(formula_names: &[&str]) -> Homebrew {
        Homebrew {
            taps: Vec::new(),
            explicitly_installed_formulae: formula_names
                .iter()
                .map(|&formula_name| formula_name.to_owned())
                .collect(),
            non_app_casks: Vec::new(),
        }
    }

    fn macos(homebrew: Option<Homebrew>, apps: Vec<MacOsApp>) -> MacOsConfig {
        MacOsConfig { homebrew, apps }
    }

    // -- App path validation (BaseMacOsApp) --

    #[test]
    fn allows_valid_app_path() {
        assert!(no_constraint_violation(&cask(
            "visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn disallows_zip_in_app_path() {
        assert!(constraint_violation(&cask(
            "visual-studio-code",
            &["/Applications/Visual Studio Code.zip"]
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

    // -- ManualApp --

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

    // -- HomebrewCask --

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
        assert!(no_constraint_violation(&cask(
            "user/repo/visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
        assert!(no_constraint_violation(&cask(
            "homebrew/cask/visual-studio-code@insiders",
            &["/Applications/Visual Studio Code - Insiders.app"]
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
    fn disallows_empty_cask_name() {
        assert!(constraint_violation(&cask(
            "",
            &["/Applications/Visual Studio Code.app"]
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
    fn disallows_cask_name_with_one_slash() {
        assert!(constraint_violation(&cask(
            "repo/visual-studio-code",
            &["/Applications/Visual Studio Code.app"]
        )));
    }

    #[test]
    fn disallows_cask_name_with_more_than_two_slashes() {
        assert!(constraint_violation(&cask(
            "homebrew/cask/fonts/font-fira-code",
            &["/Applications/Font Fira Code.app"]
        )));
    }

    // -- Homebrew --

    #[test]
    fn allows_homebrew_with_no_taps() {
        assert!(no_constraint_violation(&homebrew_with_taps(&[])));
    }

    #[test]
    fn allows_homebrew_with_taps() {
        assert!(no_constraint_violation(&homebrew_with_taps(&[
            "homebrew/cask",
            "homebrew/core"
        ])));
    }

    #[test]
    fn disallows_duplicate_taps() {
        assert!(constraint_violation(&homebrew_with_taps(&[
            "homebrew/cask",
            "homebrew/cask"
        ])));
    }

    #[test]
    fn disallows_tap_with_no_slash() {
        assert!(constraint_violation(&homebrew_with_taps(&["homebrew"])));
    }

    #[test]
    fn disallows_tap_with_two_slashes() {
        assert!(constraint_violation(&homebrew_with_taps(&[
            "homebrew/cask/fonts"
        ])));
    }

    #[test]
    fn allows_homebrew_with_explicitly_installed_formulae() {
        assert!(no_constraint_violation(&homebrew_with_formulae(&[
            "xz",
            "ca-certificates",
            "hdrhistogram_c",
            "homebrew/core/openssl@3",
        ])));
    }

    #[test]
    fn disallows_invalid_explicitly_installed_formula() {
        assert!(constraint_violation(&homebrew_with_formulae(&[
            "hdrhistogram__c"
        ])));
        assert!(constraint_violation(&homebrew_with_formulae(&[
            "ca-certificates!"
        ])));
    }

    #[test]
    fn disallows_duplicate_explicitly_installed_formulae() {
        assert!(constraint_violation(&homebrew_with_formulae(&["xz", "xz"])));
        assert!(constraint_violation(&homebrew_with_formulae(&[
            "homebrew/core/openssl@3",
            "openssl@3"
        ])));
        assert!(constraint_violation(&homebrew_with_formulae(&[
            "homebrew/core/openssl@3",
            "user/repo/openssl@3"
        ])));
    }

    #[test]
    fn allows_homebrew_with_no_non_app_casks() {
        assert!(no_constraint_violation(&homebrew()));
    }

    #[test]
    fn allows_homebrew_with_non_app_casks() {
        assert!(no_constraint_violation(&homebrew_with_non_app_casks(&[
            "font-fira-code",
            "macfuse",
            "homebrew/cask/font-fira-code-nerd-font"
        ])));
    }

    #[test]
    fn disallows_invalid_non_app_cask_name() {
        assert!(constraint_violation(&homebrew_with_non_app_casks(&[
            "font-fira-code!"
        ])));
    }

    #[test]
    fn disallows_duplicate_non_app_cask_names() {
        assert!(constraint_violation(&homebrew_with_non_app_casks(&[
            "font-fira-code",
            "font-fira-code"
        ])));
        assert!(constraint_violation(&homebrew_with_non_app_casks(&[
            "homebrew/cask/font-fira-code",
            "font-fira-code"
        ])));
        assert!(constraint_violation(&homebrew_with_non_app_casks(&[
            "homebrew/cask/font-fira-code",
            "user/repo/font-fira-code"
        ])));
    }

    // -- MacAppStoreApp --

    #[test]
    fn disallows_app_store_app_with_zero_app_store_id() {
        assert!(constraint_violation(&app_store(
            0,
            &["/Applications/Visual Studio Code.app"]
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

    // -- TestFlightApp --

    #[test]
    fn allows_valid_testflight_app() {
        assert!(no_constraint_violation(&testflight(
            "My App",
            &["/Applications/My App.app"]
        )));
    }

    #[test]
    fn disallows_testflight_app_with_empty_name() {
        assert!(constraint_violation(&testflight(
            "",
            &["/Applications/My App.app"]
        )));
    }

    #[test]
    fn disallows_testflight_app_with_multiple_app_paths() {
        assert!(constraint_violation(&testflight(
            "My App",
            &["/Applications/My App.app", "/Applications/My App 2.app",]
        )));
    }

    // -- MacOsConfig --

    #[test]
    fn disallows_empty_apps() {
        assert!(constraint_violation(&macos(Some(homebrew()), vec![])));
        assert!(constraint_violation(&macos(None, vec![])));
    }

    #[test]
    fn allows_cask_with_homebrew() {
        assert!(no_constraint_violation(&macos(
            Some(homebrew()),
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
        assert!(no_constraint_violation(&macos(
            Some(homebrew()),
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
    fn allows_non_cask_with_homebrew() {
        assert!(no_constraint_violation(&macos(
            Some(homebrew()),
            vec![MacOsApp::MacAppStoreApp(app_store(
                1,
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
    }

    #[test]
    fn disallows_cask_with_no_homebrew() {
        assert!(constraint_violation(&macos(
            None,
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
        assert!(constraint_violation(&macos(
            None,
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
    fn allows_non_cask_with_no_homebrew() {
        assert!(no_constraint_violation(&macos(
            None,
            vec![MacOsApp::MacAppStoreApp(app_store(
                1,
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
    }

    #[test]
    fn disallows_duplicate_app_paths() {
        assert!(constraint_violation(&macos(
            Some(homebrew()),
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &[
                    "/Applications/Visual Studio Code.app",
                    "/Applications/Visual Studio Code.app",
                ]
            ))]
        )));
        assert!(constraint_violation(&macos(
            Some(homebrew()),
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
            Some(homebrew()),
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
            Some(homebrew()),
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
    fn allows_casks_with_the_same_tap_and_different_cask_names() {
        assert!(no_constraint_violation(&macos(
            Some(homebrew()),
            vec![
                MacOsApp::HomebrewCask(cask(
                    "homebrew/cask/visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::HomebrewCask(cask(
                    "homebrew/cask/visual-studio-code@insiders",
                    &["/Applications/Visual Studio Code - Insiders.app"]
                )),
            ]
        )));
    }

    #[test]
    fn disallows_duplicate_cask_name_ends() {
        assert!(constraint_violation(&macos(
            Some(homebrew()),
            vec![
                MacOsApp::HomebrewCask(cask(
                    "homebrew/cask/visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::HomebrewCask(cask(
                    "visual-studio-code",
                    &["/Applications/Visual Studio Code - Insiders.app"]
                )),
            ]
        )));
        assert!(constraint_violation(&macos(
            Some(homebrew()),
            vec![
                MacOsApp::HomebrewCask(cask(
                    "homebrew/cask/visual-studio-code",
                    &["/Applications/Visual Studio Code.app"]
                )),
                MacOsApp::HomebrewCask(cask(
                    "user/repo/visual-studio-code",
                    &["/Applications/Visual Studio Code - Insiders.app"]
                )),
            ]
        )));
    }

    #[test]
    fn disallows_casks_in_both_non_app_casks_and_apps() {
        assert!(constraint_violation(&macos(
            Some(homebrew_with_non_app_casks(&["visual-studio-code"])),
            vec![MacOsApp::HomebrewCask(cask(
                "visual-studio-code",
                &["/Applications/Visual Studio Code.app"]
            ))]
        )));
    }

    #[test]
    fn disallows_duplicate_app_store_ids() {
        assert!(constraint_violation(&macos(
            Some(homebrew()),
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
    fn disallows_duplicate_testflight_names() {
        assert!(constraint_violation(&macos(
            None,
            vec![
                MacOsApp::TestFlightApp(testflight("My App", &["/Applications/My App.app"])),
                MacOsApp::TestFlightApp(testflight("My App", &["/Applications/My App 2.app"])),
            ]
        )));
    }

    #[test]
    fn disallows_duplicate_manual_names() {
        assert!(constraint_violation(&macos(
            None,
            vec![
                MacOsApp::ManualApp(manual("My App", &["/Applications/My App.app"])),
                MacOsApp::ManualApp(manual("My App", &["/Applications/My App 2.app"])),
            ]
        )));
    }

    #[test]
    fn disallows_duplicate_name_across_manual_and_testflight_apps() {
        assert!(constraint_violation(&macos(
            None,
            vec![
                MacOsApp::ManualApp(manual("My App", &["/Applications/My App.app"])),
                MacOsApp::TestFlightApp(testflight("My App", &["/Applications/My App 2.app"])),
            ]
        )));
    }

    #[test]
    fn allows_manual_and_testflight_apps_with_different_names() {
        assert!(no_constraint_violation(&macos(
            None,
            vec![
                MacOsApp::ManualApp(manual("My App", &["/Applications/My App.app"])),
                MacOsApp::TestFlightApp(testflight(
                    "My Other App",
                    &["/Applications/My Other App.app"]
                )),
            ]
        )));
    }
}
