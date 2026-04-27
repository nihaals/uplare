use std::{
    env,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};

/// Expands `~/`.
pub(super) fn resolve_configured_path(configured_path: &str) -> Result<PathBuf> {
    if let Some(relative_path) = configured_path.strip_prefix("~/") {
        let home_dir = env::var("HOME").context("HOME environment variable not set")?;
        return Ok(PathBuf::from(home_dir).join(relative_path));
    }

    Ok(configured_path.into())
}

pub(super) fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

pub(super) fn ensure_paths_do_not_overlap(paths: &[&Path]) -> Result<()> {
    let normalized_paths: Vec<PathBuf> = paths.iter().map(|path| normalize_path(path)).collect();
    for (index, path) in normalized_paths.iter().enumerate() {
        for later in &normalized_paths[index + 1..] {
            if path.starts_with(later) || later.starts_with(path) {
                bail!(
                    "paths overlap: `{}` and `{}`",
                    path.display(),
                    later.display()
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path_simple() {
        assert_eq!(normalize_path(Path::new("/a/b/c/")), Path::new("/a/b/c/"));
    }

    #[test]
    fn test_normalize_path_two_dots() {
        assert_eq!(normalize_path(Path::new("/a/../c/")), Path::new("/c/"));
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_no_overlap() {
        let result = ensure_paths_do_not_overlap(&[
            Path::new("/source/file1.txt"),
            Path::new("/source/file2.txt"),
            Path::new("/source/a/"),
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_two_directories_parent_first() {
        let result =
            ensure_paths_do_not_overlap(&[Path::new("/source/a/"), Path::new("/source/a/b/")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_two_directories_child_first() {
        let result =
            ensure_paths_do_not_overlap(&[Path::new("/source/a/b/"), Path::new("/source/a/")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_three_directories_parent_first() {
        let result = ensure_paths_do_not_overlap(&[
            Path::new("/source/a/"),
            Path::new("/source/c/"),
            Path::new("/source/a/b/"),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_three_directories_child_first() {
        let result = ensure_paths_do_not_overlap(&[
            Path::new("/source/a/b/"),
            Path::new("/source/c/"),
            Path::new("/source/a/"),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_two_directories_two_dots() {
        let result = ensure_paths_do_not_overlap(&[
            Path::new("/source/../source/a/"),
            Path::new("/source/a/b/"),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_directory_file() {
        let result =
            ensure_paths_do_not_overlap(&[Path::new("/source/a/"), Path::new("/source/a/b.txt")]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_two_dots_file() {
        let result = ensure_paths_do_not_overlap(&[
            Path::new("/source/a/"),
            Path::new("/source/../source/a/b.txt"),
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_paths_do_not_overlap_overlap_two_dots_directory() {
        let result = ensure_paths_do_not_overlap(&[
            Path::new("/source/../source/a/"),
            Path::new("/source/a/b.txt"),
        ]);
        assert!(result.is_err());
    }
}
