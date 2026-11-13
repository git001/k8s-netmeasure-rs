//! `/proc/cpuinfo`: CPU model and microcode revision for the node context (design
//! §5). Hosts behind the same instance type can differ in CPU as well as in NIC.
//!
//! Own code on purpose: two `key : value` lines do not justify a dependency
//! (`procfs` would bring far more than this).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CpuInfo {
    /// `model name` of the first processor.
    pub model: Option<String>,
    /// `microcode` of the first processor, as printed (e.g. `0xffffffff` when a
    /// hypervisor hides it).
    pub microcode: Option<String>,
}

/// Values of the first processor block; a missing key is `None`.
pub fn parse(text: &str) -> CpuInfo {
    let first = text.split("\n\n").next().unwrap_or_default();
    let value = |key: &str| {
        first.lines().find_map(|line| {
            let (k, v) = line.split_once(':')?;
            let v = v.trim();
            (k.trim() == key && !v.is_empty()).then(|| v.to_owned())
        })
    };
    CpuInfo {
        model: value("model name"),
        microcode: value("microcode"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const X86: &str = "processor\t: 0\nvendor_id\t: GenuineIntel\n\
        model name\t: Example CPU @ 2.10GHz\nmicrocode\t: 0xffffffff\nflags\t\t: fpu\n\n\
        processor\t: 1\nmodel name\t: Other\nmicrocode\t: 0x1\n";

    #[test]
    fn first_processor_wins() {
        let c = parse(X86);
        assert_eq!(c.model.as_deref(), Some("Example CPU @ 2.10GHz"));
        assert_eq!(c.microcode.as_deref(), Some("0xffffffff"));
    }

    #[test]
    fn missing_or_empty_keys_are_none() {
        let c = parse("processor\t: 0\nmicrocode\t:\n");
        assert_eq!(
            c,
            CpuInfo {
                model: None,
                microcode: None
            }
        );
        assert_eq!(parse("").model, None);
    }
}
