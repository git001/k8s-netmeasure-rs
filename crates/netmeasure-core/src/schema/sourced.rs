//! Provenance of a context field (design §5): every value says where it came from, and
//! a missing value is `unknown`, never a silent default.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Read by this process.
    Observed,
    /// Read by an underlay `listen` on the same node (`--node-context-from`).
    ObservedViaNodeListener,
    /// From a command-line flag or environment variable.
    Declared,
    Unknown,
}

/// A value with its provenance; `value` is `None` exactly when `source` is `unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sourced<T> {
    pub value: Option<T>,
    pub source: Source,
}

impl<T> Sourced<T> {
    /// `observed` if there is a value, else `unknown`.
    pub fn observed(value: Option<T>) -> Self {
        Self::with(value, Source::Observed)
    }

    /// `declared` if there is a value, else `unknown`.
    pub fn declared(value: Option<T>) -> Self {
        Self::with(value, Source::Declared)
    }

    pub fn unknown() -> Self {
        Self::with(None, Source::Unknown)
    }

    fn with(value: Option<T>, source: Source) -> Self {
        let source = if value.is_some() {
            source
        } else {
            Source::Unknown
        };
        Sourced { value, source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_value_is_always_unknown() {
        assert_eq!(Sourced::observed(Some(1)).source, Source::Observed);
        assert_eq!(Sourced::declared(Some("a")).source, Source::Declared);
        assert_eq!(Sourced::<u8>::observed(None), Sourced::unknown());
        assert_eq!(Sourced::<u8>::declared(None).source, Source::Unknown);
    }

    #[test]
    fn serialises_snake_case_and_rejects_unknown_fields() {
        let s = Sourced {
            value: Some(7u32),
            source: Source::ObservedViaNodeListener,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, r#"{"value":7,"source":"observed_via_node_listener"}"#);
        assert_eq!(serde_json::from_str::<Sourced<u32>>(&json).unwrap(), s);
        let extra = r#"{"value":7,"source":"observed","x":1}"#;
        assert!(serde_json::from_str::<Sourced<u32>>(extra).is_err());
    }
}
