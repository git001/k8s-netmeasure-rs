//! Result files written by `run` in end-to-end tests.

use netmeasure_core::schema::result::ResultFile;

/// A fresh directory per test; the result file inside it.
pub fn out_path(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("nm-run-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("result.json")
}

pub fn read(path: &std::path::Path) -> ResultFile {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).expect("result file matches the schema strictly")
}
