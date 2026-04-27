use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{borrow::Cow, collections::HashSet};
use validator::{Validate, ValidationError, ValidationErrors};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum FileCheck {
    FileExists(FileExists),
    DirectoryExists(DirectoryExists),
    FileSync(FileSync),
    DirectorySync(DirectorySync),
    FileEqualsString(FileEqualsString),
    FileContainsString(FileContainsString),
    FileEqualsJson(FileEqualsJson),
    FileMatchesJson(FileMatchesJson),
    FileEqualsYaml(FileEqualsYaml),
    FileMatchesYaml(FileMatchesYaml),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileExists {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DirectoryExists {
    #[validate(custom(function = "validate_directory_path"))]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileSync {
    #[validate(custom(function = "validate_sync_file_path"))]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct DirectorySync {
    #[validate(custom(function = "validate_sync_directory_path"))]
    pub path: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileEqualsString {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileContainsString {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
    #[validate(length(min = 1))]
    pub substring: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileEqualsJson {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
    pub json: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileMatchesJson {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
    #[validate(custom(function = "validate_match_object"))]
    pub json: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileEqualsYaml {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
    pub yaml: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(rename_all = "camelCase")]
pub struct FileMatchesYaml {
    #[validate(custom(function = "validate_file_path"))]
    pub path: String,
    #[validate(custom(function = "validate_match_object"))]
    pub yaml: Value,
}

impl FileCheck {
    pub fn path(&self) -> &str {
        match self {
            Self::FileExists(check) => &check.path,
            Self::DirectoryExists(check) => &check.path,
            Self::FileSync(check) => &check.path,
            Self::DirectorySync(check) => &check.path,
            Self::FileEqualsString(check) => &check.path,
            Self::FileContainsString(check) => &check.path,
            Self::FileEqualsJson(check) => &check.path,
            Self::FileMatchesJson(check) => &check.path,
            Self::FileEqualsYaml(check) => &check.path,
            Self::FileMatchesYaml(check) => &check.path,
        }
    }

    pub fn sync_kind(&self) -> Option<SyncKind> {
        match self {
            Self::FileSync(_) => Some(SyncKind::File),
            Self::DirectorySync(_) => Some(SyncKind::Directory),
            _ => None,
        }
    }
}

impl Validate for FileCheck {
    fn validate(&self) -> Result<(), ValidationErrors> {
        match self {
            Self::FileExists(check) => check.validate(),
            Self::DirectoryExists(check) => check.validate(),
            Self::FileSync(check) => check.validate(),
            Self::DirectorySync(check) => check.validate(),
            Self::FileEqualsString(check) => check.validate(),
            Self::FileContainsString(check) => check.validate(),
            Self::FileEqualsJson(check) => check.validate(),
            Self::FileMatchesJson(check) => check.validate(),
            Self::FileEqualsYaml(check) => check.validate(),
            Self::FileMatchesYaml(check) => check.validate(),
        }
    }
}

pub fn validate_distinct_file_check_paths(
    file_checks: &Vec<FileCheck>,
) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for file_check in file_checks {
        let path = file_check.path();
        if !seen.insert(path) {
            let mut error = ValidationError::new("duplicate_path");
            error.add_param(Cow::from("value"), &path.to_owned());
            return Err(error);
        }
    }
    Ok(())
}

pub fn validate_file_path(path: &str) -> Result<(), ValidationError> {
    if is_valid_file_path(path) {
        return Ok(());
    }

    let mut error = ValidationError::new("invalid_file_path");
    error.add_param(Cow::from("value"), &path.to_owned());
    Err(error)
}

pub fn validate_directory_path(path: &str) -> Result<(), ValidationError> {
    if is_valid_directory_path(path) {
        return Ok(());
    }

    let mut error = ValidationError::new("invalid_directory_path");
    error.add_param(Cow::from("value"), &path.to_owned());
    Err(error)
}

fn validate_sync_file_path(path: &str) -> Result<(), ValidationError> {
    validate_file_path(path)?;
    validate_sync_output_path(path)
}

fn validate_sync_directory_path(path: &str) -> Result<(), ValidationError> {
    validate_directory_path(path)?;
    validate_sync_output_path(path)
}

fn validate_sync_output_path(path: &str) -> Result<(), ValidationError> {
    if path.to_lowercase().starts_with("/userhome/") {
        let mut error = ValidationError::new("sync_path_uses_reserved_userhome_prefix");
        error.add_param(Cow::from("value"), &path.to_owned());
        return Err(error);
    }

    Ok(())
}

fn validate_match_object(value: &Value) -> Result<(), ValidationError> {
    if value.is_object() {
        return Ok(());
    }

    Err(ValidationError::new("match_requires_object"))
}

fn is_valid_file_path(path: &str) -> bool {
    (path.starts_with('/') || path.starts_with("~/")) && !path.ends_with('/')
}

fn is_valid_directory_path(path: &str) -> bool {
    (path.starts_with('/') || path.starts_with("~/")) && path.ends_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn no_constraint_violation(value: &impl Validate) -> bool {
        value.validate().is_ok()
    }

    fn constraint_violation(value: &impl Validate) -> bool {
        value.validate().is_err()
    }

    #[test]
    fn allows_valid_file_paths() {
        assert!(no_constraint_violation(&FileExists {
            path: "/etc/hosts".to_owned(),
        }));
        assert!(no_constraint_violation(&FileExists {
            path: "~/.ssh/config".to_owned(),
        }));
        assert!(no_constraint_violation(&FileExists {
            path: "/userhome/.ssh/config".to_owned(),
        }));
    }

    #[test]
    fn disallows_invalid_file_paths() {
        assert!(constraint_violation(&FileExists {
            path: "etc/hosts".to_owned(),
        }));
        assert!(constraint_violation(&FileExists {
            path: "/etc/hosts/".to_owned(),
        }));
    }

    #[test]
    fn allows_valid_directory_paths() {
        assert!(no_constraint_violation(&DirectoryExists {
            path: "/etc/".to_owned(),
        }));
        assert!(no_constraint_violation(&DirectoryExists {
            path: "~/.config/".to_owned(),
        }));
        assert!(no_constraint_violation(&DirectoryExists {
            path: "/userhome/.config/".to_owned(),
        }));
    }

    #[test]
    fn disallows_invalid_directory_paths() {
        assert!(constraint_violation(&DirectoryExists {
            path: "etc/".to_owned(),
        }));
        assert!(constraint_violation(&DirectoryExists {
            path: "/etc".to_owned(),
        }));
    }

    #[test]
    fn disallows_sync_path_using_reserved_userhome_prefix() {
        assert!(constraint_violation(&FileSync {
            path: "/userhome/test.txt".to_owned(),
        }));
        assert!(constraint_violation(&DirectorySync {
            path: "/UserHome/config/".to_owned(),
        }));
    }

    #[test]
    fn disallows_empty_contains_substring() {
        assert!(constraint_violation(&FileContainsString {
            path: "/etc/hosts".to_owned(),
            substring: String::new(),
        }));
    }

    #[test]
    fn disallows_non_object_match_values() {
        assert!(constraint_violation(&FileMatchesJson {
            path: "/etc/test.json".to_owned(),
            json: json!(["a"]),
        }));
        assert!(constraint_violation(&FileMatchesYaml {
            path: "/etc/test.yaml".to_owned(),
            yaml: json!(true),
        }));
    }
}
