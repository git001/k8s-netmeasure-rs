//! Durations as written on the command line (`10ms`, `5s`, `1.5s`, `250us`) and their
//! conversion. The only module that uses `jiff` (design §2); never used in a timed
//! section.

use std::time::Duration;

use jiff::SignedDuration;

/// Parses a positive duration in jiff's "friendly" format, e.g. `10ms`, `5s`,
/// `1m30s`, `1.5s`, `250us`. ISO 8601 (`PT5S`) is accepted as well. Zero and negative
/// values are errors.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("empty duration".to_owned());
    }
    let signed: SignedDuration = trimmed
        .parse()
        .map_err(|e| format!("invalid duration `{trimmed}` (e.g. 10ms, 5s, 1m30s): {e}"))?;
    if signed.is_negative() || signed.is_zero() {
        return Err(format!("duration `{trimmed}` must be greater than zero"));
    }
    Duration::try_from(signed).map_err(|e| format!("duration `{trimmed}` out of range: {e}"))
}

/// Formats a duration compactly for messages and help texts, e.g. `5s`, `10ms`.
pub fn format_duration(d: Duration) -> String {
    match SignedDuration::try_from(d) {
        Ok(s) => format!("{s:#}"),
        Err(_) => format!("{}s", d.as_secs()),
    }
}

/// UTC timestamp in RFC 3339 with nanoseconds, e.g. `2026-11-13T21:41:58.598879123Z`.
pub fn rfc3339_ns(unix_ns: i128) -> String {
    match jiff::Timestamp::from_nanosecond(unix_ns) {
        Ok(t) => format!("{t:.9}"),
        Err(_) => "invalid-time".to_owned(),
    }
}

/// Nanoseconds as `u64`, saturating (≈ 584 years).
pub fn as_nanos_u64(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_with_nanoseconds() {
        assert_eq!(
            rfc3339_ns(1_000_000_000_123),
            "1970-01-01T00:16:40.000000123Z"
        );
        assert_eq!(rfc3339_ns(i128::MAX), "invalid-time");
    }

    #[test]
    fn parses_friendly_units() {
        assert_eq!(parse_duration("10ms").unwrap(), Duration::from_millis(10));
        assert_eq!(parse_duration("5s").unwrap(), Duration::from_secs(5));
        assert_eq!(parse_duration("1m30s").unwrap(), Duration::from_secs(90));
        assert_eq!(parse_duration("1.5s").unwrap(), Duration::from_millis(1500));
        assert_eq!(
            parse_duration(" 250us ").unwrap(),
            Duration::from_micros(250)
        );
        assert_eq!(parse_duration("PT2S").unwrap(), Duration::from_secs(2));
    }

    #[test]
    fn rejects_zero_negative_empty_and_garbage() {
        assert!(
            parse_duration("0s")
                .unwrap_err()
                .contains("greater than zero")
        );
        assert!(
            parse_duration("-5s")
                .unwrap_err()
                .contains("greater than zero")
        );
        assert!(parse_duration("").unwrap_err().contains("empty"));
        assert!(
            parse_duration("fast")
                .unwrap_err()
                .contains("invalid duration")
        );
        assert!(parse_duration("10").is_err(), "a unit is required");
    }

    #[test]
    fn formats_compactly() {
        assert_eq!(format_duration(Duration::from_secs(5)), "5s");
        assert_eq!(format_duration(Duration::from_millis(10)), "10ms");
        assert_eq!(
            format_duration(Duration::from_secs(u64::MAX)),
            format!("{}s", u64::MAX)
        );
    }

    #[test]
    fn nanos_saturate() {
        assert_eq!(as_nanos_u64(Duration::from_millis(3)), 3_000_000);
        assert_eq!(as_nanos_u64(Duration::MAX), u64::MAX);
    }
}
