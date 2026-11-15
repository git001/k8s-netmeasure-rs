//! Where the result file goes (design §9, revised after the first cluster runs): a
//! directory gets a generated, unique file name; an explicit file must not exist yet.
//! A result file is never overwritten silently.

use std::path::{Path, PathBuf};

use netmeasure_util::time::compact_utc;

/// `--output` as checked at start, before anything is measured.
#[derive(Debug, PartialEq, Eq)]
pub enum Output {
    /// An existing directory: the name is generated once `run_id` is known.
    Dir(PathBuf),
    /// A file name in an existing directory; the file does not exist.
    File(PathBuf),
}

/// `Err` refuses the start (exit 3).
pub fn check(output: &Path) -> Result<Output, String> {
    if output.is_dir() {
        return Ok(Output::Dir(output.to_owned()));
    }
    if output.as_os_str().to_string_lossy().ends_with('/') {
        return Err("cannot write --output: the directory does not exist".into());
    }
    let dir = output.parent().filter(|p| !p.as_os_str().is_empty());
    if dir.is_some_and(|d| !d.is_dir()) {
        return Err("cannot write --output: the directory does not exist".into());
    }
    if output.exists() {
        return Err(format!(
            "--output {} exists; a result file is never overwritten (give a directory to get a unique name)",
            output.display()
        ));
    }
    Ok(Output::File(output.to_owned()))
}

/// The final path: for a directory `<path>-<node>-<UTC start>-<run_id>.json`.
pub fn resolve(
    output: Output,
    path: &str,
    node: Option<&str>,
    start_unix_ns: i128,
    run_id: &str,
) -> PathBuf {
    match output {
        Output::File(f) => f,
        Output::Dir(d) => d.join(name(path, node, start_unix_ns, run_id)),
    }
}

fn name(path: &str, node: Option<&str>, start_unix_ns: i128, run_id: &str) -> String {
    format!(
        "{}-{}-{}-{run_id}.json",
        safe(path),
        safe(node.unwrap_or("unknown-node")),
        compact_utc(start_unix_ns)
    )
}

/// Keeps `[A-Za-z0-9._-]`, replaces everything else (`/`, spaces, …) with `_`.
fn safe(part: &str) -> String {
    part.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => c,
            _ => '_',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("nm-outpath-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn generated_name_is_unique_and_safe() {
        let n = name(
            "overlay",
            Some("node a/1"),
            1_763_174_575_000_000_000,
            "01ABC",
        );
        assert_eq!(n, "overlay-node_a_1-20251115T024255Z-01ABC.json");
        let unknown = name("underlay", None, 0, "01ABC");
        assert_eq!(unknown, "underlay-unknown-node-19700101T000000Z-01ABC.json");
    }

    #[test]
    fn directory_file_and_refusals() {
        let d = temp("check");
        assert_eq!(check(&d).unwrap(), Output::Dir(d.clone()));
        let f = d.join("r.json");
        assert_eq!(check(&f).unwrap(), Output::File(f.clone()));
        std::fs::write(&f, b"x").unwrap();
        assert!(check(&f).unwrap_err().contains("never overwritten"));
        assert!(check(&d.join("missing/r.json")).is_err());
        assert!(check(Path::new("/nonexistent-nm-dir/")).is_err());
        let resolved = resolve(Output::Dir(d.clone()), "overlay", Some("n"), 0, "01X");
        assert_eq!(resolved, d.join("overlay-n-19700101T000000Z-01X.json"));
        std::fs::remove_dir_all(&d).unwrap();
    }
}
