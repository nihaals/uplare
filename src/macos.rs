use std::{collections::HashSet, path::Path};

use anyhow::Result;

pub fn homebrew_is_installed() -> bool {
    which::which("brew").is_ok()
        || Path::new("/opt/homebrew/bin/brew").is_file()
        || Path::new("/usr/local/bin/brew").is_file()
}

/// Get the list of installed applications as either `/Applications/App.app` or `~/Applications/App.app`. Searches one
/// subdirectory deep in `/Applications` and `~/Applications`, excluding inside apps. Excludes pre-installed apps.
pub fn get_apps() -> Result<HashSet<String>> {
    // Pre-installed apps are defined as being in `/System/Applications`
    todo!()
}
