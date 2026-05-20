use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

pub fn homebrew_is_installed() -> bool {
    which::which("brew").is_ok()
        || Path::new("/opt/homebrew/bin/brew").is_file()
        || Path::new("/usr/local/bin/brew").is_file()
}

fn brew_list<const N: usize>(args: [&'static str; N]) -> Result<HashSet<String>> {
    let command = || format!("brew {}", args.join(" "));
    let output = Command::new("brew")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run `{}`", command()))?;

    if !output.status.success() {
        bail!("`{}` failed with non-zero exit code", command());
    }

    let stdout = String::from_utf8(output.stdout)
        .with_context(|| format!("`{}` output was not valid UTF-8", command()))?;
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

pub fn get_taps() -> Result<HashSet<String>> {
    brew_list(["tap"])
}

pub fn get_explicitly_installed_formulae_brew() -> Result<HashSet<String>> {
    brew_list(["list", "--installed-on-request", "--full-name"])
}

pub fn get_installed_formulae_brew() -> Result<HashSet<String>> {
    brew_list(["list", "--formula"])
}

pub fn get_dependency_formulae_brew() -> Result<HashSet<String>> {
    brew_list(["list", "--installed-as-dependency", "--full-name"])
}

pub fn get_installed_casks_brew() -> Result<HashSet<String>> {
    brew_list(["list", "--cask", "--full-name"])
}

pub fn get_installed_cask_tokens_brew() -> Result<HashSet<String>> {
    brew_list(["list", "--cask"])
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

#[cfg(test)]
mod tests {
    use super::*;

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
