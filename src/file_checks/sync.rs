use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::{
    file_checks::shared::{ensure_paths_do_not_overlap, normalize_path, resolve_configured_path},
    pkl_types::file_check::{FileCheck, SyncKind},
};

const HOME_SYNC_PREFIX: &str = "userhome";

fn configured_path_to_sync_root_path(configured_path: &str, root: &Path) -> Result<PathBuf> {
    let normalized = configured_path.trim_end_matches('/');
    let relative_path = if let Some(relative_path) = normalized.strip_prefix("~/") {
        PathBuf::from(HOME_SYNC_PREFIX).join(relative_path)
    } else {
        PathBuf::from(normalized.trim_start_matches('/'))
    };

    let root_path = root.join(&relative_path);
    if root_path == root {
        bail!("sync path `{configured_path}` maps to the sync root itself");
    }
    Ok(root_path)
}

struct SyncEntry {
    source_path: PathBuf,
    sync_root_path: PathBuf,
}

fn ensure_sync_entry_paths_do_not_overlap(root: &Path, entries: &[SyncEntry]) -> Result<()> {
    let sync_root_paths: Vec<&Path> = entries
        .iter()
        .map(|entry| entry.sync_root_path.as_path())
        .collect();
    ensure_paths_do_not_overlap(&sync_root_paths).with_context(|| "Sync outputs overlap")?;

    let normalized_root = normalize_path(root);
    for entry in entries {
        if normalized_root.starts_with(normalize_path(&entry.sync_root_path)) {
            bail!(
                "sync path `{}` maps to the sync root or one of its parents",
                entry.source_path.display(),
            );
        }
    }

    let source_paths: Vec<&Path> = entries
        .iter()
        .map(|entry| entry.source_path.as_path())
        .collect();
    ensure_paths_do_not_overlap(&source_paths).with_context(|| "Sync sources overlap")?;

    Ok(())
}

fn build_sync_entries(file_checks: &[FileCheck], root: &Path) -> Result<Vec<SyncEntry>> {
    let mut entries = Vec::new();
    for file_check in file_checks {
        let Some(kind) = file_check.sync_kind() else {
            continue;
        };

        let source_path = resolve_configured_path(file_check.path())?;
        match kind {
            SyncKind::File if !source_path.is_file() => {
                bail!("sync target `{}` is not a file", source_path.display());
            }
            SyncKind::Directory if !source_path.is_dir() => {
                bail!("sync target `{}` is not a directory", source_path.display());
            }
            _ => {}
        }

        let sync_root_path = configured_path_to_sync_root_path(file_check.path(), root)?;
        entries.push(SyncEntry {
            source_path,
            sync_root_path,
        });
    }

    ensure_sync_entry_paths_do_not_overlap(root, &entries)?;
    entries.sort_by(|a, b| a.sync_root_path.cmp(&b.sync_root_path));
    Ok(entries)
}

/// Gets all the symlinks in `dir` (recursively) and adds them to `symlinks`. Also adds warnings for non-symlinks in
/// `dir`.
fn get_symlinks(dir: &Path, symlinks: &mut Vec<PathBuf>, warnings: &mut Vec<String>) -> Result<()> {
    let read_dir = match fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(error) => {
            return Err(error).with_context(|| format!("Failed to read `{}`", dir.display()));
        }
    };

    for entry in read_dir {
        let entry =
            entry.with_context(|| format!("Failed to read entry in `{}`", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("Failed to get file type of `{}`", path.display()))?;

        if file_type.is_symlink() {
            symlinks.push(path);
            continue;
        }

        if file_type.is_dir() {
            get_symlinks(&path, symlinks, warnings)?;
            continue;
        }

        // Non-symlink file
        warnings.push(format!("{} exists and is not a symlink", path.display()));
    }

    Ok(())
}

fn create_symlink(target: &Path, link_path: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link_path)
}

#[derive(Debug, Default)]
pub struct FileSyncReport {
    /// (symlink_path, symlink_target)
    pub created: Vec<(PathBuf, PathBuf)>,
    pub deleted: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub fn sync_sync_file_checks(file_checks: &[FileCheck], root: &Path) -> Result<FileSyncReport> {
    let sync_entries = build_sync_entries(file_checks, root)?;
    if sync_entries.is_empty() {
        bail!("no sync file checks configured");
    }

    let target_symlink_paths: HashSet<&Path> = sync_entries
        .iter()
        .map(|entry| entry.sync_root_path.as_path())
        .collect();

    if root.is_file() {
        bail!("`{}` is a file", root.display());
    }
    fs::create_dir_all(root).with_context(|| format!("Failed to create `{}`", root.display()))?;

    let mut report = FileSyncReport::default();
    let existing_symlinks = {
        let mut existing_symlinks = Vec::new();
        get_symlinks(root, &mut existing_symlinks, &mut report.warnings)?;
        existing_symlinks.sort();
        existing_symlinks
    };

    for symlink_path in existing_symlinks {
        if target_symlink_paths.contains(symlink_path.as_path()) {
            continue;
        }

        fs::remove_file(&symlink_path)
            .with_context(|| format!("Failed to remove `{}`", symlink_path.display()))?;
        report.deleted.push(symlink_path);
    }

    for sync_entry in sync_entries {
        let Some(parent) = sync_entry.sync_root_path.parent() else {
            bail!(
                "`{}` has no parent directory",
                sync_entry.sync_root_path.display(),
            );
        };
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create `{}`", parent.display()))?;

        if sync_entry.sync_root_path.is_symlink() {
            let existing_target = fs::read_link(&sync_entry.sync_root_path).with_context(|| {
                format!(
                    "Failed to read symlink `{}`",
                    sync_entry.sync_root_path.display(),
                )
            })?;
            if existing_target != sync_entry.source_path {
                report.warnings.push(format!(
                    "{} exists and is a symlink to `{}` instead of `{}`",
                    sync_entry.sync_root_path.display(),
                    existing_target.display(),
                    sync_entry.source_path.display(),
                ));
            }
            continue;
        }

        create_symlink(&sync_entry.source_path, &sync_entry.sync_root_path).with_context(|| {
            format!(
                "Failed to create symlink `{}` -> `{}`",
                sync_entry.sync_root_path.display(),
                sync_entry.source_path.display(),
            )
        })?;
        report
            .created
            .push((sync_entry.sync_root_path, sync_entry.source_path));
    }

    report.created.sort();
    report.deleted.sort();
    report.warnings.sort();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{env, fs};

    use crate::pkl_types::file_check::{DirectorySync, FileSync};

    fn sync_entry(path: &str) -> SyncEntry {
        let source_path = resolve_configured_path(path).unwrap();
        let sync_root_path = configured_path_to_sync_root_path(path, Path::new("/root/")).unwrap();
        SyncEntry {
            source_path,
            sync_root_path,
        }
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_no_overlap() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/file1.txt"),
                sync_entry("/source/file2.txt"),
                sync_entry("/source/a/"),
            ],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_directories_parent_first() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[sync_entry("/source/a/"), sync_entry("/source/a/b/")],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_directories_child_first() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[sync_entry("/source/a/b/"), sync_entry("/source/a/")],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_three_directories_parent_first() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/a/"),
                sync_entry("/source/c/"),
                sync_entry("/source/a/b/"),
            ],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_three_directories_child_first() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/a/b/"),
                sync_entry("/source/c/"),
                sync_entry("/source/a/"),
            ],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_directories_two_dots() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/../source/a/"),
                sync_entry("/source/a/b/"),
            ],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_directory_file() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[sync_entry("/source/a/"), sync_entry("/source/a/b.txt")],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_home() {
        let home_dir: PathBuf = env::var("HOME").unwrap().into();

        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("~/source/a/"),
                sync_entry(home_dir.join("source/a/b.txt").to_str().unwrap()),
            ],
        );
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Sync sources overlap"),
        )
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_dots_file() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/a/"),
                sync_entry("/source/../source/a/b.txt"),
            ],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_dots_directory() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/../source/a/"),
                sync_entry("/source/a/b.txt"),
            ],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_dots_directory_equal() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[sync_entry("/source/../source/a/"), sync_entry("/source/a/")],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_overlap_two_dots_file_equal() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[
                sync_entry("/source/a/file.txt"),
                sync_entry("/source/../source/a/file.txt"),
            ],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_exactly_root() {
        let result = ensure_sync_entry_paths_do_not_overlap(
            Path::new("/root/"),
            &[SyncEntry {
                source_path: PathBuf::from("/"),
                sync_root_path: PathBuf::from("/root/"),
            }],
        );
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("sync path `/` maps to the sync root or one of its parents"),
        );
    }

    #[test]
    fn test_ensure_sync_entry_paths_do_not_overlap_root_parent() {
        let result =
            ensure_sync_entry_paths_do_not_overlap(Path::new("/root/"), &[sync_entry("/../")]);
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("sync path `/../` maps to the sync root or one of its parents"),
        );
    }

    #[test]
    fn test_configured_path_to_sync_root_path_root() {
        assert_eq!(
            configured_path_to_sync_root_path("/a/", Path::new("/root/")).unwrap(),
            Path::new("/root/a/")
        );
    }

    #[test]
    fn test_configured_path_to_sync_root_path_home() {
        assert_eq!(
            configured_path_to_sync_root_path("~/a.txt", Path::new("/root/")).unwrap(),
            Path::new("/root/userhome/a.txt")
        );
    }

    fn temp_dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn test_get_symlinks_empty() {
        let temp_dir = temp_dir();
        let mut symlinks = Vec::new();
        let mut warnings = Vec::new();
        get_symlinks(temp_dir.path(), &mut symlinks, &mut warnings).unwrap();
        assert!(symlinks.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_get_symlinks() {
        let temp_dir = temp_dir();
        let root = temp_dir.path().join("root");
        fs::create_dir_all(root.join("dir")).unwrap();

        // File: dir/a.txt
        // Check files causes warnings and we go into subdirectories
        fs::write(root.join("dir/a.txt"), "test").unwrap();

        // Symlink: dir/b.txt
        // Check we find a file symlink we go into subdirectories
        fs::write(temp_dir.path().join("b.txt"), "test").unwrap();
        create_symlink(&temp_dir.path().join("b.txt"), &root.join("dir/b.txt")).unwrap();

        // Symlink: dir/c
        // Check we find a directory symlink and go into subdirectories
        fs::create_dir(temp_dir.path().join("c")).unwrap();
        create_symlink(&temp_dir.path().join("c"), &root.join("dir/c")).unwrap();

        // File: dir/c/d.txt
        // Check we don't go into symlinked directories
        fs::write(temp_dir.path().join("c/d.txt"), "test").unwrap();

        let mut symlinks = Vec::new();
        let mut warnings = Vec::new();
        get_symlinks(&root, &mut symlinks, &mut warnings).unwrap();
        symlinks.sort();
        assert_eq!(symlinks, [root.join("dir/b.txt"), root.join("dir/c")]);
        assert_eq!(
            warnings,
            [format!(
                "{} exists and is not a symlink",
                root.join("dir/a.txt").display(),
            )],
        );
    }

    #[test]
    fn test_sync_sync_file_checks() {
        let temp_dir = temp_dir();
        let root = temp_dir.path().join("root");
        let source = temp_dir.path().join("source");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&source).unwrap();
        let mut file_checks = Vec::new();
        let mut add_file_check = |path: &Path| {
            let file_check = if path.is_dir() {
                let path = format!("{}/", path.to_string_lossy().trim_end_matches('/'));
                FileCheck::DirectorySync(DirectorySync { path })
            } else if path.is_file() {
                let path = path.to_string_lossy().into_owned();
                FileCheck::FileSync(FileSync { path })
            } else {
                panic!("path is not a file or directory");
            };
            file_checks.push(file_check);
        };

        // Stale symlinks are deleted (and only the symlink)
        {
            let target = source.join("stale_link.txt");
            fs::write(&target, "test").unwrap();
            create_symlink(&target, &root.join("stale_link.txt")).unwrap();
        }

        // Random files get warnings
        {
            let target = root.join("random_file.txt");
            fs::write(&target, "test").unwrap();
        }

        // Incorrect symlinks get warnings
        {
            let expected_source_path = source.join("expected_source.txt");
            let wrong_source_path = source.join("wrong_source.txt");
            fs::write(&expected_source_path, "test").unwrap();
            fs::write(&wrong_source_path, "test").unwrap();
            let sync_path = configured_path_to_sync_root_path(
                expected_source_path.to_string_lossy().as_ref(),
                &root,
            )
            .unwrap();
            fs::create_dir_all(sync_path.parent().unwrap()).unwrap();
            create_symlink(&wrong_source_path, &sync_path).unwrap();
            add_file_check(&expected_source_path);
        }

        // New directory symlinks are created
        {
            let target = source.join("new_dir/");
            fs::create_dir(&target).unwrap();
            add_file_check(&target);
        }

        // New file symlinks are created
        {
            let target = source.join("new_link.txt");
            fs::write(&target, "test").unwrap();
            add_file_check(&target);
        }

        let FileSyncReport {
            mut created,
            mut deleted,
            mut warnings,
        } = sync_sync_file_checks(&file_checks, &root).unwrap();

        {
            let deleted = deleted.remove(0);
            assert_eq!(deleted, root.join("stale_link.txt"));
            assert!(!deleted.exists());
            assert!(source.join("stale_link.txt").exists());
        }

        {
            let warning = warnings.remove(0);
            assert_eq!(
                warning,
                format!(
                    "{} exists and is not a symlink",
                    root.join("random_file.txt").display(),
                ),
            );
            assert!(root.join("random_file.txt").exists());
        }

        {
            let warning = warnings.remove(0);
            let expected_source_path = source.join("expected_source.txt");
            let wrong_source_path = source.join("wrong_source.txt");
            let sync_path = configured_path_to_sync_root_path(
                expected_source_path.to_string_lossy().as_ref(),
                &root,
            )
            .unwrap();
            assert_eq!(
                warning,
                format!(
                    "{} exists and is a symlink to `{}` instead of `{}`",
                    sync_path.display(),
                    wrong_source_path.display(),
                    expected_source_path.display(),
                ),
            );
            assert!(sync_path.is_symlink());
            assert_eq!(sync_path.read_link().unwrap(), wrong_source_path,);
        }

        {
            let (created_path, created_target) = created.remove(0);
            let expected_created_path = configured_path_to_sync_root_path(
                source.join("new_dir/").to_string_lossy().as_ref(),
                &root,
            )
            .unwrap();
            assert_eq!(expected_created_path, created_path);
            assert_eq!(created_target, source.join("new_dir/"));
            assert!(created_path.exists());
            assert!(created_path.is_symlink());
            assert_eq!(created_path.read_link().unwrap(), created_target);
        }

        {
            let (created_path, created_target) = created.remove(0);
            let expected_created_path = configured_path_to_sync_root_path(
                source.join("new_link.txt").to_string_lossy().as_ref(),
                &root,
            )
            .unwrap();
            assert_eq!(expected_created_path, created_path);
            assert_eq!(created_target, source.join("new_link.txt"));
            assert!(created_path.exists());
            assert!(created_path.is_symlink());
            assert_eq!(created_path.read_link().unwrap(), created_target);
        }

        assert!(created.is_empty());
        assert!(deleted.is_empty());
        assert!(warnings.is_empty());

        let FileSyncReport {
            created,
            deleted,
            mut warnings,
        } = sync_sync_file_checks(&file_checks, &root).unwrap();

        {
            let warning = warnings.remove(0);
            assert_eq!(
                warning,
                format!(
                    "{} exists and is not a symlink",
                    root.join("random_file.txt").display(),
                ),
            );
            assert!(root.join("random_file.txt").exists());
        }

        {
            let warning = warnings.remove(0);
            let expected_source_path = source.join("expected_source.txt");
            let wrong_source_path = source.join("wrong_source.txt");
            let sync_path = configured_path_to_sync_root_path(
                expected_source_path.to_string_lossy().as_ref(),
                &root,
            )
            .unwrap();
            assert_eq!(
                warning,
                format!(
                    "{} exists and is a symlink to `{}` instead of `{}`",
                    sync_path.display(),
                    wrong_source_path.display(),
                    expected_source_path.display(),
                ),
            );
            assert!(sync_path.is_symlink());
            assert_eq!(sync_path.read_link().unwrap(), wrong_source_path,);
        }

        assert!(created.is_empty());
        assert!(deleted.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn test_sync_sync_file_checks_excludes_root() {
        let temp_dir = temp_dir();
        let root = temp_dir.path().join("root");
        let configured_path = temp_dir.path().join("source");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&configured_path).unwrap();
        let configured_path = configured_path
            .to_string_lossy()
            .trim_end_matches('/')
            .to_string()
            + "/";
        let file_checks = [FileCheck::DirectorySync(DirectorySync {
            path: configured_path,
        })];

        assert!(sync_sync_file_checks(&file_checks, &root).is_ok());
    }

    #[test]
    fn test_sync_sync_file_checks_is_root() {
        let temp_dir = temp_dir();
        let root = temp_dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let configured_path = "/".to_owned();
        let file_checks = [FileCheck::DirectorySync(DirectorySync {
            path: configured_path,
        })];

        assert!(sync_sync_file_checks(&file_checks, &root).is_err());
    }

    #[test]
    fn test_sync_sync_file_checks_includes_root() {
        let temp_dir = temp_dir();
        let root = temp_dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let configured_path = "/../".to_owned();
        let file_checks = [FileCheck::DirectorySync(DirectorySync {
            path: configured_path,
        })];

        assert!(sync_sync_file_checks(&file_checks, &root).is_err());
    }
}
