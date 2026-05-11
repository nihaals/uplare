use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use thiserror::Error;

pub fn homebrew_is_installed() -> bool {
    which::which("brew").is_ok()
        || Path::new("/opt/homebrew/bin/brew").is_file()
        || Path::new("/usr/local/bin/brew").is_file()
}

pub fn get_installed_casks() -> Result<HashSet<String>> {
    let output = Command::new("brew")
        .args(["list", "--cask", "--full-name"])
        .output()
        .context("Failed to run `brew list --cask --full-name`")?;

    if !output.status.success() {
        bail!("`brew list --cask --full-name` failed with non-zero exit code");
    }

    let stdout = String::from_utf8(output.stdout)
        .context("`brew list --cask --full-name` output was not valid UTF-8")?;
    Ok({
        stdout
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            })
            .collect()
    })
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
    Ok({
        stdout
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            })
            .collect()
    })
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
}
