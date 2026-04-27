use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value as JsonValue};

use crate::{
    file_checks::shared::{ensure_paths_do_not_overlap, resolve_configured_path},
    pkl_types::file_check::FileCheck,
};

const SUBSTRING_SEARCH_CHUNK_SIZE: usize = 8192;

fn file_equals_string(path: &Path, expected: &str) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(Some(format!("{} -> file does not exist", path.display())));
    }

    let actual = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) => {
            return Ok(Some(format!(
                "{} -> failed to read file: {}",
                path.display(),
                error,
            )));
        }
    };

    if actual == expected {
        return Ok(None);
    }

    Ok(Some(format!(
        "{} -> expected {}, actual {}",
        path.display(),
        serde_json::to_string(expected).expect("string should serialize to JSON"),
        serde_json::to_string(&actual).expect("string should serialize to JSON"),
    )))
}

fn file_contains_string(path: &Path, substring: &str) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(Some(format!("{} -> file does not exist", path.display())));
    }

    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => {
            return Ok(Some(format!(
                "{} -> failed to read file: {}",
                path.display(),
                error,
            )));
        }
    };

    let contains = reader_contains_bytes(file, substring.as_bytes(), SUBSTRING_SEARCH_CHUNK_SIZE)
        .with_context(|| format!("Failed to read `{}`", path.display()))?;
    if contains {
        Ok(None)
    } else {
        Ok(Some(format!(
            "{} -> file does not contain {}",
            path.display(),
            serde_json::to_string(substring).expect("string should serialize to JSON"),
        )))
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn reader_contains_bytes<R: Read>(mut reader: R, needle: &[u8], chunk_size: usize) -> Result<bool> {
    let mut chunk = vec![0; chunk_size.max(needle.len())];
    let mut tail = Vec::new();

    loop {
        let bytes_read = reader.read(&mut chunk).context("Failed to read chunk")?;
        if bytes_read == 0 {
            return Ok(false);
        }

        let mut window = Vec::with_capacity(tail.len() + bytes_read);
        window.extend_from_slice(&tail);
        window.extend_from_slice(&chunk[..bytes_read]);

        if contains_bytes(&window, needle) {
            return Ok(true);
        }

        let overlap = needle.len().saturating_sub(1);
        tail.clear();
        if overlap != 0 {
            let start = window.len().saturating_sub(overlap);
            tail.extend_from_slice(&window[start..]);
        }
    }
}

#[derive(Clone, Copy)]
enum StructuredFileFormat {
    Json,
    Yaml,
}

impl StructuredFileFormat {
    fn name(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
        }
    }
}

enum StructuredFile {
    Parsed(JsonValue),
    Invalid(String),
}

fn read_structured_file(path: &Path, format: StructuredFileFormat) -> Result<StructuredFile> {
    let contents =
        fs::read(path).with_context(|| format!("Failed to read `{}`", path.display()))?;

    let parsed = match format {
        StructuredFileFormat::Json => {
            serde_json::from_slice::<JsonValue>(&contents).map_err(|error| error.to_string())
        }
        StructuredFileFormat::Yaml => {
            serde_yaml::from_slice::<JsonValue>(&contents).map_err(|error| error.to_string())
        }
    };

    Ok(match parsed {
        Ok(value) => StructuredFile::Parsed(value),
        Err(error) => StructuredFile::Invalid(error),
    })
}

fn pretty_json(value: &JsonValue) -> String {
    serde_json::to_string_pretty(value).expect("JSON value should serialize")
}

fn indent_block(block: &str) -> String {
    block
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_structured_value_not_equal(
    path: &Path,
    expected: &JsonValue,
    actual: &JsonValue,
) -> String {
    format!(
        "{}\n  expected:\n{}\n  actual:\n{}",
        path.display(),
        indent_block(&pretty_json(expected)),
        indent_block(&pretty_json(actual)),
    )
}

fn format_structured_value_not_match(path: &Path, incorrect_values: &JsonValue) -> String {
    format!(
        "{}\n  incorrect values:\n{}",
        path.display(),
        indent_block(&pretty_json(incorrect_values)),
    )
}

fn file_equals_structure(
    path: &Path,
    expected: &JsonValue,
    format: StructuredFileFormat,
) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(Some(format!("{} -> file does not exist", path.display())));
    }

    let actual = read_structured_file(path, format)?;
    match actual {
        StructuredFile::Parsed(actual) => {
            if &actual == expected {
                return Ok(None);
            }

            Ok(Some(format_structured_value_not_equal(
                path, expected, &actual,
            )))
        }
        StructuredFile::Invalid(error) => Ok(Some(format!(
            "{} -> file is not valid {}: {}",
            path.display(),
            format.name(),
            error,
        ))),
    }
}

fn find_mismatched_keys(
    expected: &Map<String, JsonValue>,
    actual: &JsonValue,
) -> Option<JsonValue> {
    let Some(actual_object) = actual.as_object() else {
        return Some(JsonValue::Object(expected.clone()));
    };

    let mut mismatches = Map::new();
    for (key, expected_value) in expected {
        match (expected_value, actual_object.get(key)) {
            (JsonValue::Object(expected_object), Some(actual_value)) => {
                if let Some(mismatch) = find_mismatched_keys(expected_object, actual_value) {
                    mismatches.insert(key.clone(), mismatch);
                }
            }
            (_, Some(actual_value)) if actual_value == expected_value => {}
            _ => {
                mismatches.insert(key.clone(), expected_value.clone());
            }
        }
    }

    if mismatches.is_empty() {
        None
    } else {
        Some(JsonValue::Object(mismatches))
    }
}

fn file_matches_structure(
    path: &Path,
    expected: &JsonValue,
    format: StructuredFileFormat,
) -> Result<Option<String>> {
    if !path.is_file() {
        return Ok(Some(format!("{} -> file does not exist", path.display())));
    }

    let actual = read_structured_file(path, format)?;
    match actual {
        StructuredFile::Parsed(actual) => {
            let Some(expected_object) = expected.as_object() else {
                bail!(
                    "expected match value for `{}` to be an object",
                    path.display(),
                );
            };

            let Some(incorrect_values) = find_mismatched_keys(expected_object, &actual) else {
                return Ok(None);
            };

            Ok(Some(format_structured_value_not_match(
                path,
                &incorrect_values,
            )))
        }
        StructuredFile::Invalid(error) => Ok(Some(format!(
            "{} -> file is not valid {}: {}",
            path.display(),
            format.name(),
            error,
        ))),
    }
}

fn diff_file_check(file_check: &FileCheck) -> Result<Option<String>> {
    let path = resolve_configured_path(file_check.path())?;

    match file_check {
        FileCheck::FileExists(check) => {
            if path.is_file() {
                Ok(None)
            } else {
                Ok(Some(format!("{} -> file does not exist", check.path)))
            }
        }
        FileCheck::FileSync(check) => {
            if path.is_file() {
                Ok(None)
            } else {
                Ok(Some(format!("{} -> file does not exist", check.path)))
            }
        }
        FileCheck::DirectoryExists(check) => {
            if path.is_dir() {
                Ok(None)
            } else {
                Ok(Some(format!("{} -> directory does not exist", check.path)))
            }
        }
        FileCheck::DirectorySync(check) => {
            if path.is_dir() {
                Ok(None)
            } else {
                Ok(Some(format!("{} -> directory does not exist", check.path)))
            }
        }
        FileCheck::FileEqualsString(check) => file_equals_string(&path, &check.contents),
        FileCheck::FileContainsString(check) => file_contains_string(&path, &check.substring),
        FileCheck::FileEqualsJson(check) => {
            file_equals_structure(&path, &check.json, StructuredFileFormat::Json)
        }
        FileCheck::FileMatchesJson(check) => {
            file_matches_structure(&path, &check.json, StructuredFileFormat::Json)
        }
        FileCheck::FileEqualsYaml(check) => {
            file_equals_structure(&path, &check.yaml, StructuredFileFormat::Yaml)
        }
        FileCheck::FileMatchesYaml(check) => {
            file_matches_structure(&path, &check.yaml, StructuredFileFormat::Yaml)
        }
    }
}

pub fn diff_file_checks(file_checks: &[FileCheck]) -> Result<Vec<String>> {
    let paths: Vec<PathBuf> = file_checks
        .iter()
        .map(|check| resolve_configured_path(check.path()))
        .collect::<Result<_>>()?;
    let paths: Vec<&Path> = paths.iter().map(|path| path.as_path()).collect();
    ensure_paths_do_not_overlap(&paths)?;

    let mut mismatches = Vec::new();
    for file_check in file_checks {
        if let Some(mismatch) = diff_file_check(file_check)? {
            mismatches.push(mismatch);
        }
    }
    Ok(mismatches)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io;

    use serde_json::json;

    #[test]
    fn test_find_mismatched_keys_mismatch() {
        let expected = json!({
            "a": "b",
            "nested": {
                "c": 1,
                "d": 2,
            },
        });
        let actual = json!({
            "a": "b",
            "nested": {
                "c": 9,
            },
            "extra": true,
        });

        assert_eq!(
            find_mismatched_keys(expected.as_object().unwrap(), &actual),
            Some(json!({
                "nested": {
                    "c": 1,
                    "d": 2,
                },
            })),
        );
    }

    #[test]
    fn test_find_mismatched_keys_match() {
        let expected = json!({
            "a": "b",
            "nested": {
                "c": 9,
            },
        });
        let actual = json!({
            "a": "b",
            "nested": {
                "c": 9,
                "d": 2,
            },
            "extra": true,
        });

        assert_eq!(
            find_mismatched_keys(expected.as_object().unwrap(), &actual),
            None,
        );
    }

    #[test]
    fn test_reader_contains_bytes() {
        let reader = io::Cursor::new(b"aaaaabbbbbccccc".to_vec());
        assert!(reader_contains_bytes(reader, b"abbbbbc", 4).unwrap());
    }
}
