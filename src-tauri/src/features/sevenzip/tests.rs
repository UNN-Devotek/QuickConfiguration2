#![cfg(test)]

use std::fs;

use super::*;

use tempfile::TempDir;

#[test]
fn test_extract_archive() {
    let tmp_dir = TempDir::new().unwrap();
    let zip_file = Path::new("tests/fixtures/test.zip");
    let extracted_file = tmp_dir.path().join("test.txt");

    assert!(!extracted_file.exists());
    extract_archive(zip_file, tmp_dir.path()).unwrap();
    assert!(extracted_file.is_file());
    assert_eq!(fs::read_to_string(extracted_file).unwrap(), "test");
}

#[test]
fn test_extract_archive_confines_traversal_and_symlinks() {
    let owned = TempDir::new().unwrap();
    let destination = owned.path().join("extracted");
    fs::create_dir(&destination).unwrap();
    let sentinel = owned.path().join("outside.txt");
    fs::write(&sentinel, "unchanged").unwrap();

    let result = extract_archive(
        Path::new("tests/fixtures/archive-traversal.tar"),
        destination.as_path(),
    );

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "unchanged");
    assert!(!destination.join("escape.txt").is_symlink());
}
