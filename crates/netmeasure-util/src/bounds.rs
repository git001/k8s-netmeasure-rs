//! Range-checked values: one error message format for every flag (design §2).

use std::fmt::Display;

/// Returns `value` if `min <= value <= max`, otherwise an error naming `what`.
pub fn in_range<T: PartialOrd + Display>(
    what: &str,
    value: T,
    min: T,
    max: T,
) -> Result<T, String> {
    if value < min || value > max {
        Err(format!(
            "{what} must be between {min} and {max}, got {value}"
        ))
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_inclusive_bounds_and_rejects_outside() {
        assert_eq!(in_range("--count", 1, 1, 100), Ok(1));
        assert_eq!(in_range("--count", 100, 1, 100), Ok(100));
        assert_eq!(
            in_range("--count", 101, 1, 100),
            Err("--count must be between 1 and 100, got 101".to_owned())
        );
        assert!(in_range("--count", 0, 1, 100).is_err());
    }
}
