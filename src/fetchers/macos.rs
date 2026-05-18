use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use thiserror::Error;

pub fn homebrew_is_installed() -> bool {
    which::which("brew").is_ok()
        || Path::new("/opt/homebrew/bin/brew").is_file()
        || Path::new("/usr/local/bin/brew").is_file()
}

pub fn get_taps() -> Result<HashSet<String>> {
    let output = Command::new("brew")
        .arg("tap")
        .output()
        .context("Failed to run `brew tap`")?;

    if !output.status.success() {
        bail!("`brew tap` failed with non-zero exit code");
    }

    let stdout =
        String::from_utf8(output.stdout).context("`brew tap` output was not valid UTF-8")?;
    Ok(stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        })
        .collect())
}

pub fn get_explicitly_installed_formulae_brew() -> Result<HashSet<String>> {
    let output = Command::new("brew")
        .args(["list", "--installed-on-request", "--full-name"])
        .output()
        .context("Failed to run `brew list --installed-on-request --full-name`")?;

    if !output.status.success() {
        bail!("`brew list --installed-on-request --full-name` failed with non-zero exit code");
    }

    let stdout = String::from_utf8(output.stdout)
        .context("`brew list --installed-on-request --full-name` output was not valid UTF-8")?;
    Ok(stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        })
        .collect())
}

pub fn get_installed_casks_brew() -> Result<HashSet<String>> {
    let output = Command::new("brew")
        .args(["list", "--cask", "--full-name"])
        .output()
        .context("Failed to run `brew list --cask --full-name`")?;

    if !output.status.success() {
        bail!("`brew list --cask --full-name` failed with non-zero exit code");
    }

    let stdout = String::from_utf8(output.stdout)
        .context("`brew list --cask --full-name` output was not valid UTF-8")?;
    Ok(stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        })
        .collect())
}

fn homebrew_prefix() -> Result<PathBuf> {
    if let Ok(prefix) = env::var("HOMEBREW_PREFIX") {
        return Ok(prefix.into());
    }

    for prefix in ["/opt/homebrew", "/usr/local"] {
        let prefix = PathBuf::from(prefix);
        if prefix.join("bin/brew").is_file() {
            return Ok(prefix);
        }
    }

    bail!("HOMEBREW_PREFIX not set and failed to find Homebrew prefix");
}

fn path_file_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .context("Homebrew path does not have a file name")?
        .to_str()
        .context("Homebrew path is not valid UTF-8")?
        .to_owned();
    ensure!(!name.is_empty(), "Homebrew path has an empty file name");
    Ok(name)
}

#[derive(Deserialize)]
struct FormulaInstallReceipt {
    installed_on_request: bool,
    source: FormulaInstallSource,
}

#[derive(Deserialize)]
struct FormulaInstallSource {
    tap: String,
}

impl FormulaInstallReceipt {
    fn from_token(token: &str, prefix: &Path) -> Result<Self> {
        let receipt_path = {
            let receipt_path = prefix.join("opt").join(token).join("INSTALL_RECEIPT.json");
            if !receipt_path.is_file() {
                bail!(
                    "failed to find Homebrew install receipt for formula `{}`",
                    token,
                );
            }
            receipt_path
        };
        let receipt = fs::read_to_string(&receipt_path).with_context(|| {
            format!(
                "Failed to read Homebrew install receipt `{}`",
                receipt_path.display(),
            )
        })?;
        serde_json::from_str(&receipt).with_context(|| {
            format!(
                "Failed to parse Homebrew install receipt `{}`",
                receipt_path.display(),
            )
        })
    }
}

fn full_formula_name(name: &str, tap: &str) -> String {
    match tap {
        "homebrew/core" => name.to_owned(),
        tap => format!("{tap}/{name}"),
    }
}

/// Checks if the path is a directory and not a symlink
fn is_direct_directory(path: &Path) -> Result<bool> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).context("Failed to get metadata for Homebrew path"),
    };
    Ok(metadata.is_dir() && !metadata.file_type().is_symlink())
}

pub fn get_explicitly_installed_formulae_custom() -> Result<HashSet<String>> {
    let prefix = homebrew_prefix()?;
    let cellar = prefix.join("Cellar");
    ensure!(cellar.is_dir(), "homebrew Cellar directory does not exist");

    let mut formulae = HashSet::new();
    for formula_entry in fs::read_dir(&cellar).context("Failed to read Homebrew Cellar")? {
        let formula_path = formula_entry?.path();
        if !is_direct_directory(&formula_path)? {
            continue;
        }

        let formula_name = path_file_name(&formula_path)?;
        let receipt = FormulaInstallReceipt::from_token(&formula_name, &prefix)?;
        if receipt.installed_on_request {
            formulae.insert(full_formula_name(&formula_name, &receipt.source.tap));
        }
    }

    Ok(formulae)
}

#[derive(Deserialize)]
struct CaskInstallReceipt {
    source: CaskInstallSource,
}

#[derive(Deserialize)]
struct CaskInstallSource {
    tap: String,
}

fn cask_installed_tap(cask_path: &Path) -> Result<Option<String>> {
    // For old cask installs, this file doesn't exist
    // Even if an old cask is updated, this file will still not be created
    // However, parsing this seems to be the simplest option and our implementation should not be expected to be
    // perfect, we are optimising for speed and simplicity while catching enough of the common cases to be accurate for
    // most systems
    // It also contains the metadata at install time which could differ from the metadata for the current version
    // `brew list --cask --full-name` also handles multiple taps providing the same cask name in incorrect ways
    // so this is probably fine
    let receipt_path = cask_path.join(".metadata/INSTALL_RECEIPT.json");
    if !receipt_path.is_file() {
        return Ok(None);
    }
    let receipt = fs::read_to_string(&receipt_path).with_context(|| {
        format!(
            "Failed to read Homebrew cask install receipt `{}`",
            receipt_path.display(),
        )
    })?;
    let receipt: CaskInstallReceipt = serde_json::from_str(&receipt).with_context(|| {
        format!(
            "Failed to parse Homebrew cask install receipt `{}`",
            receipt_path.display(),
        )
    })?;

    Ok(Some(receipt.source.tap))
}

fn full_cask_name(token: &str, tap: Option<&str>) -> String {
    match tap {
        // `None` is not actually the same as `homebrew/cask`, it means unknown
        Some("homebrew/cask") | None => token.to_owned(),
        Some(tap) => format!("{tap}/{token}"),
    }
}

pub fn get_installed_casks_custom() -> Result<HashSet<String>> {
    let caskroom = homebrew_prefix()?.join("Caskroom");
    let mut casks = HashSet::new();
    ensure!(
        caskroom.is_dir(),
        "homebrew Caskroom directory does not exist"
    );

    for cask_entry in fs::read_dir(&caskroom).context("Failed to read Homebrew Caskroom")? {
        let cask_path = cask_entry?.path();
        if !is_direct_directory(&cask_path)? {
            continue;
        }

        let token = path_file_name(&cask_path)?;
        casks.insert(full_cask_name(
            &token,
            cask_installed_tap(&cask_path)?.as_deref(),
        ));
    }

    Ok(casks)
}

/// Get the list of installed applications as either `/Applications/App.app` or `~/Applications/App.app`. Searches one
/// subdirectory deep in `/Applications` and `~/Applications`, excluding inside apps. Excludes pre-installed apps.
pub fn get_apps() -> Result<HashSet<String>> {
    let home_dir: PathBuf = env::var("HOME")
        .context("HOME environment variable not set")?
        .into();

    let mut apps = HashSet::new();
    for (root, is_home_root) in [
        (PathBuf::from("/Applications"), false),
        (home_dir.join("Applications"), true),
    ] {
        if !root.is_dir() {
            continue;
        }

        let home_dir = if is_home_root {
            Some(home_dir.as_path())
        } else {
            None
        };

        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            add_potential_app(&mut apps, &path, home_dir)?;

            if path.is_dir() && !is_app_bundle(&path) {
                for nested_entry in fs::read_dir(&path)? {
                    let nested_path = nested_entry?.path();
                    add_potential_app(&mut apps, &nested_path, home_dir)?;
                }
            }
        }
    }

    Ok(apps)
}

fn is_app_bundle(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "app") && path.is_dir()
}

fn add_potential_app(
    apps: &mut HashSet<String>,
    path: &Path,
    home_dir: Option<&Path>,
) -> Result<()> {
    if !is_app_bundle(path) {
        return Ok(());
    }
    if [
        "/Applications/Utilities/Feedback Assistant.app",
        "/Applications/Safari.app",
    ]
    .contains(&path.to_str().context("App path is not valid UTF-8")?)
    {
        return Ok(());
    }

    if let Some(home_dir) = home_dir {
        let relative_path = path
            .strip_prefix(home_dir)
            .context("App in home does not have home as prefix")?;
        apps.insert(format!(
            "~/{}",
            relative_path
                .to_str()
                .context("App path is not valid UTF-8")?
        ));
    } else {
        apps.insert(
            path.to_str()
                .context("App path is not valid UTF-8")?
                .to_owned(),
        );
    }

    Ok(())
}

#[derive(PartialEq, Eq, Hash)]
pub enum MacAppStoreApp {
    AppStore { app_id: u64, app_name: String },
    TestFlight { app_name: String },
}

#[derive(Error, Debug)]
pub enum MacAppStoreListError {
    #[error("`mas` is not in PATH")]
    MasNotFound,
    #[error("failed to run `mas list`: {0}")]
    MasListCommand(#[from] std::io::Error),
    #[error("`mas list` failed with non-zero exit code: {code:?}")]
    MasListFailed { code: Option<i32> },
    #[error("`mas list` output was not valid UTF-8: {0}")]
    InvalidUtf8Output(#[from] std::string::FromUtf8Error),
    #[error("failed to parse `mas list` line: `{line}`")]
    MalformedLine { line: String },
    #[error("failed to parse app id in `mas list` line: `{line}`")]
    InvalidAppId { line: String },
}

pub fn get_installed_mas_apps() -> Result<HashSet<MacAppStoreApp>, MacAppStoreListError> {
    if which::which("mas").is_err() {
        return Err(MacAppStoreListError::MasNotFound);
    }

    let output = Command::new("mas").arg("list").output()?;

    if !output.status.success() {
        return Err(MacAppStoreListError::MasListFailed {
            code: output.status.code(),
        });
    }

    let stdout = String::from_utf8(output.stdout)?;
    let mut apps = HashSet::new();

    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let (app_id, app_name) = parse_mas_list_line(line)?;
        if app_id == 0 {
            apps.insert(MacAppStoreApp::TestFlight { app_name });
        } else {
            apps.insert(MacAppStoreApp::AppStore { app_id, app_name });
        }
    }

    Ok(apps)
}

fn parse_mas_list_line(line: &str) -> Result<(u64, String), MacAppStoreListError> {
    let (app_id_raw, remainder) = line.split_once(char::is_whitespace).ok_or_else(|| {
        MacAppStoreListError::MalformedLine {
            line: line.to_owned(),
        }
    })?;

    let app_id =
        app_id_raw
            .trim()
            .parse::<u64>()
            .map_err(|_| MacAppStoreListError::InvalidAppId {
                line: line.to_owned(),
            })?;

    let trimmed_remainder = remainder.trim_start();
    let version_start =
        trimmed_remainder
            .rfind(" (")
            .ok_or_else(|| MacAppStoreListError::MalformedLine {
                line: line.to_owned(),
            })?;

    if !trimmed_remainder.ends_with(')') {
        return Err(MacAppStoreListError::MalformedLine {
            line: line.to_owned(),
        });
    }

    let app_name = trimmed_remainder[..version_start].trim_end().to_owned();
    if app_name.is_empty() {
        return Err(MacAppStoreListError::MalformedLine {
            line: line.to_owned(),
        });
    }

    Ok((app_id, app_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mas_list_line_app_store() {
        let line = " 899247664  TestFlight             (4.1.0)".trim();
        assert_eq!(
            parse_mas_list_line(line).unwrap(),
            (899247664, "TestFlight".to_owned()),
        );
    }

    #[test]
    fn test_parse_mas_list_line_testflight() {
        let line = "         0  TestFlight             (4.1.0)".trim();
        assert_eq!(
            parse_mas_list_line(line).unwrap(),
            (0, "TestFlight".to_owned()),
        );
    }

    #[test]
    #[ignore = "requires `brew`"]
    fn test_get_explicitly_installed_formulae_custom() {
        let brew = get_explicitly_installed_formulae_brew().unwrap();
        let custom = get_explicitly_installed_formulae_custom().unwrap();
        assert_eq!(brew, custom);
    }

    #[test]
    #[ignore = "requires `brew`"]
    fn test_get_installed_casks_custom() {
        let brew = get_installed_casks_brew().unwrap();
        let custom = get_installed_casks_custom().unwrap();
        assert_eq!(brew, custom);
    }
}
