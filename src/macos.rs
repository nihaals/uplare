use std::{
    collections::HashSet,
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

pub fn homebrew_is_installed() -> bool {
    which::which("brew").is_ok()
        || Path::new("/opt/homebrew/bin/brew").is_file()
        || Path::new("/usr/local/bin/brew").is_file()
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
