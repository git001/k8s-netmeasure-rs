//! `--label k=v` (design §9.2): keys normalised to Prometheus label names (`-` and `.`
//! become `_`); must match `[a-zA-Z_][a-zA-Z0-9_]*`, must not start with `__` and must
//! not be a reserved field name. Duplicates (also after normalisation) and empty
//! values are errors; values are UTF-8, at most 256 bytes.

use std::collections::BTreeMap;

pub const MAX_VALUE_BYTES: usize = 256;

/// Field names of logs and later metrics that a label must not shadow.
pub const RESERVED: [&str; 13] = [
    "target_node",
    "path",
    "result",
    "cycle_id",
    "run_id",
    "level",
    "message",
    "instance",
    "job",
    "timestamp",
    "target",
    "le",
    "quantile",
];

/// One `k=v` item as given (value parser of `--label`).
pub fn item(text: &str) -> Result<(String, String), String> {
    let (k, v) = text
        .split_once('=')
        .ok_or_else(|| format!("label `{text}`: expected key=value"))?;
    Ok((k.to_owned(), v.to_owned()))
}

/// Normalises and checks all labels.
pub fn normalise(items: &[(String, String)]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (raw, value) in items {
        let key = raw.replace(['-', '.'], "_");
        let valid_start = key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_');
        if !valid_start || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("label key `{raw}` is not a valid label name"));
        }
        if key.starts_with("__") || RESERVED.contains(&key.as_str()) {
            return Err(format!("label key `{raw}` is reserved"));
        }
        if value.is_empty() || value.len() > MAX_VALUE_BYTES {
            return Err(format!(
                "label `{raw}`: value must be 1–{MAX_VALUE_BYTES} bytes"
            ));
        }
        if out.insert(key.clone(), value.clone()).is_some() {
            return Err(format!(
                "label key `{key}` given more than once (after normalisation)"
            ));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(items: &[&str]) -> Result<BTreeMap<String, String>, String> {
        let parsed: Vec<_> = items.iter().map(|t| item(t).unwrap()).collect();
        normalise(&parsed)
    }

    #[test]
    fn keys_are_normalised() {
        let m = n(&["kernel-change=none", "a.b=1"]).unwrap();
        assert_eq!(m["kernel_change"], "none");
        assert_eq!(m["a_b"], "1");
    }

    #[test]
    fn invalid_reserved_duplicate_and_empty() {
        assert!(item("novalue").is_err());
        assert!(n(&["1x=a"]).unwrap_err().contains("not a valid"));
        assert!(n(&["__x=a"]).unwrap_err().contains("reserved"));
        assert!(n(&["path=a"]).unwrap_err().contains("reserved"));
        assert!(
            n(&["a-b=1", "a.b=2"])
                .unwrap_err()
                .contains("more than once")
        );
        assert!(n(&["a="]).unwrap_err().contains("1–256"));
        assert!(n(&[&format!("a={}", "x".repeat(257))]).is_err());
    }
}
