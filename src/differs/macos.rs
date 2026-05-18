use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, anyhow};

use crate::{
    fetchers::macos,
    file_checks,
    pkl_types::macos::{MacOsApp, MacOsConfig},
};

fn sort_paths_by_app_name<T: AsRef<str>>(paths: &mut [T]) {
    paths.sort_by(|a, b| {
        let a = a.as_ref();
        let b = b.as_ref();
        let a_name = a.rsplit('/').next().expect("app path should have a slash");
        let b_name = b.rsplit('/').next().expect("app path should have a slash");
        a_name.cmp(b_name).then_with(|| a.cmp(b))
    });
}

struct State {
    homebrew: Option<HomebrewState>,
    apps: HashSet<String>,
    mas_apps: HashSet<macos::MacAppStoreApp>,
}

struct HomebrewState {
    taps: HashSet<String>,
    explicitly_installed_formulae: HashSet<String>,
    installed_casks: HashSet<String>,
}

fn get_state(fast_brew: bool) -> Result<State> {
    std::thread::scope(|scope| {
        let homebrew = scope.spawn(|| -> Result<Option<HomebrewState>> {
            if !macos::homebrew_is_installed() {
                return Ok(None);
            }

            let taps = scope.spawn(macos::get_taps);
            let formulae = scope.spawn(if fast_brew {
                macos::get_explicitly_installed_formulae_custom
            } else {
                macos::get_explicitly_installed_formulae_brew
            });
            let casks = scope.spawn(if fast_brew {
                macos::get_installed_casks_custom
            } else {
                macos::get_installed_casks_brew
            });

            Ok(Some(HomebrewState {
                taps: taps
                    .join()
                    .map_err(|_| anyhow!("Homebrew taps fetch thread panicked"))??,
                explicitly_installed_formulae: formulae
                    .join()
                    .map_err(|_| anyhow!("Homebrew formulae fetch thread panicked"))??,
                installed_casks: casks
                    .join()
                    .map_err(|_| anyhow!("Homebrew casks fetch thread panicked"))??,
            }))
        });

        let apps = scope.spawn(macos::get_apps);
        let mas_apps = scope.spawn(|| match macos::get_installed_mas_apps() {
            Ok(installed_mas_apps) => Ok(installed_mas_apps),
            Err(macos::MacAppStoreListError::MasNotFound) => {
                eprintln!(
                    "`mas` is not installed, so installed Mac App Store apps cannot be checked"
                );
                Ok(HashSet::new())
            }
            Err(error) => Err(error).context("Failed to get installed Mac App Store apps"),
        });

        Ok(State {
            homebrew: homebrew
                .join()
                .map_err(|_| anyhow!("Homebrew fetch thread panicked"))??,
            apps: apps
                .join()
                .map_err(|_| anyhow!("app fetch thread panicked"))??,
            mas_apps: mas_apps
                .join()
                .map_err(|_| anyhow!("Mac App Store fetch thread panicked"))??,
        })
    })
}

pub fn generate_diff(
    config: MacOsConfig,
    fast_brew: bool,
) -> Result<Vec<(&'static str, Vec<String>)>> {
    let State {
        homebrew,
        apps: system_apps,
        mas_apps,
    } = get_state(fast_brew)?;
    let system_has_homebrew = homebrew.is_some();
    let (installed_taps, installed_formulae, installed_casks) = homebrew
        .map(|homebrew| {
            (
                homebrew.taps,
                homebrew.explicitly_installed_formulae,
                homebrew.installed_casks,
            )
        })
        .unwrap_or_default();
    let (installed_app_store_apps, installed_testflight_apps): (
        HashMap<u64, String>,
        HashSet<String>,
    ) = {
        let mut app_store = HashMap::new();
        let mut testflight = HashSet::new();
        for app in mas_apps {
            match app {
                macos::MacAppStoreApp::AppStore { app_id, app_name } => {
                    app_store.insert(app_id, app_name);
                }
                macos::MacAppStoreApp::TestFlight { app_name } => {
                    testflight.insert(app_name);
                }
            }
        }
        (app_store, testflight)
    };

    let configured_taps: HashSet<&str> = config
        .homebrew
        .as_ref()
        .map(|homebrew| homebrew.taps.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let configured_formulae: HashSet<&str> = config
        .homebrew
        .as_ref()
        .map(|homebrew| {
            homebrew
                .explicitly_installed_formulae
                .iter()
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let configured_non_app_casks: HashSet<&str> = config
        .homebrew
        .as_ref()
        .map(|homebrew| homebrew.non_app_casks.iter().map(String::as_str).collect())
        .unwrap_or_default();

    let configured_manual_apps: Vec<(&str, &[String])> = config
        .apps
        .iter()
        .filter_map(|app| match app {
            MacOsApp::ManualApp(manual_app) => Some((
                manual_app.name.as_str(),
                manual_app.base.app_paths.as_slice(),
            )),
            _ => None,
        })
        .collect();

    let configured_cask_apps: Vec<(&str, &[String])> = config
        .apps
        .iter()
        .filter_map(|app| match app {
            MacOsApp::HomebrewCask(cask) => {
                Some((cask.cask_name.as_str(), cask.base.app_paths.as_slice()))
            }
            _ => None,
        })
        .collect();

    let configured_app_store_apps: Vec<(u64, &[String])> = config
        .apps
        .iter()
        .filter_map(|app| match app {
            MacOsApp::MacAppStoreApp(app_store_app) => Some((
                app_store_app.app_store_id,
                app_store_app.base.app_paths.as_slice(),
            )),
            _ => None,
        })
        .collect();

    let configured_testflight_apps: Vec<(&str, &[String])> = config
        .apps
        .iter()
        .filter_map(|app| match app {
            MacOsApp::TestFlightApp(testflight_app) => Some((
                testflight_app.name.as_str(),
                testflight_app.base.app_paths.as_slice(),
            )),
            _ => None,
        })
        .collect();

    let configured_casks: HashSet<&str> = configured_cask_apps
        .iter()
        .map(|(cask_name, _)| *cask_name)
        .collect();
    let all_configured_casks: HashSet<&str> = configured_casks
        .union(&configured_non_app_casks)
        .copied()
        .collect();
    let configured_app_store_ids: HashSet<u64> = configured_app_store_apps
        .iter()
        .map(|(app_store_id, _)| *app_store_id)
        .collect();
    let configured_testflight_names: HashSet<&str> = configured_testflight_apps
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let configured_app_paths: HashSet<&str> = config
        .apps
        .iter()
        .flat_map(|app| match app {
            MacOsApp::ManualApp(manual_app) => manual_app.base.app_paths.iter(),
            MacOsApp::HomebrewCask(cask) => cask.base.app_paths.iter(),
            MacOsApp::MacAppStoreApp(app_store_app) => app_store_app.base.app_paths.iter(),
            MacOsApp::TestFlightApp(testflight_app) => testflight_app.base.app_paths.iter(),
        })
        .map(String::as_str)
        .collect();

    let mut sections: Vec<(&str, Vec<String>)> = Vec::new();

    if config.homebrew.is_some() != system_has_homebrew {
        sections.push((
            "Homebrew installation status mismatch",
            vec![format!(
                "config homebrew installed = {}, system has Homebrew = {}",
                config.homebrew.is_some(),
                system_has_homebrew,
            )],
        ));
    }

    {
        let mut configured_taps_not_installed: Vec<String> = configured_taps
            .iter()
            .filter(|&tap| !installed_taps.contains(*tap))
            .map(|&tap| tap.to_owned())
            .collect();
        configured_taps_not_installed.sort();
        if !configured_taps_not_installed.is_empty() {
            sections.push((
                "Configured Homebrew taps not installed",
                configured_taps_not_installed,
            ));
        }
    }

    {
        let mut installed_taps_not_configured: Vec<String> = installed_taps
            .iter()
            .filter(|tap| !configured_taps.contains(tap.as_str()))
            .cloned()
            .collect();
        installed_taps_not_configured.sort();
        if !installed_taps_not_configured.is_empty() {
            sections.push((
                "Installed Homebrew taps not in config",
                installed_taps_not_configured,
            ));
        }
    }

    {
        let mut configured_formulae_not_installed: Vec<String> = configured_formulae
            .iter()
            .filter(|&formula_name| !installed_formulae.contains(*formula_name))
            .map(|&formula_name| formula_name.to_owned())
            .collect();
        configured_formulae_not_installed.sort();
        if !configured_formulae_not_installed.is_empty() {
            sections.push((
                "Configured Homebrew formulae not explicitly installed",
                configured_formulae_not_installed,
            ));
        }
    }

    {
        let mut installed_formulae_not_configured: Vec<String> = installed_formulae
            .iter()
            .filter(|formula_name| !configured_formulae.contains(formula_name.as_str()))
            .cloned()
            .collect();
        installed_formulae_not_configured.sort();
        if !installed_formulae_not_configured.is_empty() {
            sections.push((
                "Explicitly installed Homebrew formulae not in config",
                installed_formulae_not_configured,
            ));
        }
    }

    {
        let mut manual_apps_with_missing_paths = Vec::new();
        for (app_name, app_paths) in configured_manual_apps {
            let mut missing_paths: Vec<&str> = app_paths
                .iter()
                .filter(|app_path| !system_apps.contains(*app_path))
                .map(String::as_str)
                .collect();
            sort_paths_by_app_name(&mut missing_paths);

            if !missing_paths.is_empty() {
                manual_apps_with_missing_paths.push(format!(
                    "{} -> missing {}",
                    app_name,
                    missing_paths.join(", "),
                ));
            }
        }
        manual_apps_with_missing_paths.sort();
        if !manual_apps_with_missing_paths.is_empty() {
            sections.push((
                "Configured manual apps missing configured app paths",
                manual_apps_with_missing_paths,
            ));
        }
    }

    {
        let mut configured_casks_not_installed: Vec<String> = all_configured_casks
            .iter()
            .filter(|&cask_name| !installed_casks.contains(*cask_name))
            .map(|&s| s.to_owned())
            .collect();
        configured_casks_not_installed.sort();
        if !configured_casks_not_installed.is_empty() {
            sections.push((
                "Configured casks not installed",
                configured_casks_not_installed,
            ));
        }
    }

    {
        let mut installed_casks_not_configured: Vec<String> = installed_casks
            .iter()
            .filter(|cask_name| !all_configured_casks.contains(cask_name.as_str()))
            .cloned()
            .collect();
        installed_casks_not_configured.sort();
        if !installed_casks_not_configured.is_empty() {
            sections.push((
                "Installed casks not in config",
                installed_casks_not_configured,
            ));
        }
    }

    {
        let mut casks_with_missing_paths = Vec::new();
        for (cask_name, app_paths) in configured_cask_apps {
            if !installed_casks.contains(cask_name) {
                continue;
            }

            let mut missing_paths: Vec<&str> = app_paths
                .iter()
                .filter(|app_path| !system_apps.contains(*app_path))
                .map(String::as_str)
                .collect();
            sort_paths_by_app_name(&mut missing_paths);

            if !missing_paths.is_empty() {
                casks_with_missing_paths.push(format!(
                    "{} -> missing {}",
                    cask_name,
                    missing_paths.join(", "),
                ));
            }
        }
        casks_with_missing_paths.sort();
        if !casks_with_missing_paths.is_empty() {
            sections.push((
                "Installed casks missing configured app paths",
                casks_with_missing_paths,
            ));
        }
    }

    {
        let mut configured_app_store_not_installed: Vec<String> = configured_app_store_ids
            .iter()
            .filter(|app_store_id| !installed_app_store_apps.contains_key(app_store_id))
            .map(|app_store_id| app_store_id.to_string())
            .collect();
        configured_app_store_not_installed.sort();
        if !configured_app_store_not_installed.is_empty() {
            sections.push((
                "Configured App Store apps not installed",
                configured_app_store_not_installed,
            ));
        }
    }

    {
        let mut installed_app_store_not_configured: Vec<String> = installed_app_store_apps
            .iter()
            .filter(|(app_store_id, _)| !configured_app_store_ids.contains(app_store_id))
            .map(|(app_store_id, app_name)| format!("{} ({})", app_name, app_store_id))
            .collect();
        installed_app_store_not_configured.sort();
        if !installed_app_store_not_configured.is_empty() {
            sections.push((
                "Installed App Store apps not in config",
                installed_app_store_not_configured,
            ));
        }
    }

    {
        let mut app_store_apps_with_missing_paths = Vec::new();
        for (app_store_id, app_paths) in configured_app_store_apps {
            if !installed_app_store_apps.contains_key(&app_store_id) {
                continue;
            }

            let mut missing_paths: Vec<&str> = app_paths
                .iter()
                .filter(|app_path| !system_apps.contains(*app_path))
                .map(String::as_str)
                .collect();
            sort_paths_by_app_name(&mut missing_paths);

            if !missing_paths.is_empty() {
                app_store_apps_with_missing_paths.push(format!(
                    "{} -> missing {}",
                    app_store_id,
                    missing_paths.join(", "),
                ));
            }
        }
        app_store_apps_with_missing_paths.sort();
        if !app_store_apps_with_missing_paths.is_empty() {
            sections.push((
                "Installed App Store apps missing configured app paths",
                app_store_apps_with_missing_paths,
            ));
        }
    }

    {
        let mut configured_testflight_not_installed: Vec<String> = configured_testflight_names
            .iter()
            .filter(|name| !installed_testflight_apps.contains(**name))
            .map(|&s| s.to_owned())
            .collect();
        configured_testflight_not_installed.sort();
        if !configured_testflight_not_installed.is_empty() {
            sections.push((
                "Configured TestFlight apps not installed",
                configured_testflight_not_installed,
            ));
        }
    }

    {
        let mut installed_testflight_not_configured: Vec<String> = installed_testflight_apps
            .iter()
            .filter(|name| !configured_testflight_names.contains(name.as_str()))
            .cloned()
            .collect();
        installed_testflight_not_configured.sort();
        if !installed_testflight_not_configured.is_empty() {
            sections.push((
                "Installed TestFlight apps not in config",
                installed_testflight_not_configured,
            ));
        }
    }

    {
        let mut testflight_apps_with_missing_paths = Vec::new();
        for (app_name, app_paths) in &configured_testflight_apps {
            if !installed_testflight_apps.contains(*app_name) {
                continue;
            }

            let mut missing_paths: Vec<&str> = app_paths
                .iter()
                .filter(|app_path| !system_apps.contains(*app_path))
                .map(String::as_str)
                .collect();
            sort_paths_by_app_name(&mut missing_paths);

            if !missing_paths.is_empty() {
                testflight_apps_with_missing_paths.push(format!(
                    "{} -> missing {}",
                    app_name,
                    missing_paths.join(", "),
                ));
            }
        }
        testflight_apps_with_missing_paths.sort();
        if !testflight_apps_with_missing_paths.is_empty() {
            sections.push((
                "Installed TestFlight apps missing configured app paths",
                testflight_apps_with_missing_paths,
            ));
        }
    }

    {
        let mut apps_not_in_config: Vec<String> = system_apps
            .iter()
            .filter(|app_path| !configured_app_paths.contains(app_path.as_str()))
            .cloned()
            .collect();
        sort_paths_by_app_name(&mut apps_not_in_config);
        if !apps_not_in_config.is_empty() {
            sections.push(("Installed apps not in config", apps_not_in_config));
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
