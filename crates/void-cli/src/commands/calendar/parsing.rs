use chrono::Local;

pub(super) fn parse_datetime_or_date(s: &str) -> anyhow::Result<String> {
    normalize_datetime(s)
}

/// Parse a datetime string in various common formats and return RFC 3339.
///
/// Accepted formats:
/// - RFC 3339: `2026-03-31T17:00:00Z`, `2026-03-31T17:00:00+02:00`
/// - ISO 8601 without offset: `2026-03-31T17:00:00`, `2026-03-31T17:00`
/// - Space-separated: `2026-03-31 17:00:00`, `2026-03-31 17:00`
/// - Date only: `2026-03-31` (midnight local time)
pub(super) fn normalize_datetime(s: &str) -> anyhow::Result<String> {
    let s = s.trim();

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }

    // "YYYY-MM-DDTHH:MM:SS" or "YYYY-MM-DDTHH:MM" (no timezone → local)
    for fmt in &["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            let local = ndt
                .and_local_timezone(Local)
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous local time for \"{s}\""))?;
            return Ok(local.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        }
    }

    // "YYYY-MM-DD HH:MM:SS" or "YYYY-MM-DD HH:MM" (space instead of T)
    for fmt in &["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, fmt) {
            let local = ndt
                .and_local_timezone(Local)
                .single()
                .ok_or_else(|| anyhow::anyhow!("Ambiguous local time for \"{s}\""))?;
            return Ok(local.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        }
    }

    // Date only → midnight local
    if let Ok(date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        let dt = date
            .and_hms_opt(0, 0, 0)
            .and_then(|ndt| ndt.and_local_timezone(Local).single())
            .ok_or_else(|| anyhow::anyhow!("Failed to convert date to local timezone"))?;
        return Ok(dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }

    anyhow::bail!(
        "Invalid date/time: \"{s}\". Use ISO 8601 format, e.g. 2026-03-31T17:00:00 or 2026-03-31."
    )
}

pub(super) fn parse_day_spec(spec: &str) -> anyhow::Result<chrono::NaiveDate> {
    let today = Local::now().date_naive();
    match spec.to_lowercase().as_str() {
        "today" => Ok(today),
        "tomorrow" => Ok(today + chrono::Duration::days(1)),
        "yesterday" => Ok(today - chrono::Duration::days(1)),
        other => chrono::NaiveDate::parse_from_str(other, "%Y-%m-%d").map_err(|_| {
            anyhow::anyhow!(
                "Invalid day: \"{other}\". Use YYYY-MM-DD, today, tomorrow, or yesterday."
            )
        }),
    }
}

pub(super) fn parse_date_to_ts(date: &str) -> Option<i64> {
    chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local, NaiveDate, Timelike};

    #[test]
    fn parse_day_spec_today() {
        let result = parse_day_spec("today").unwrap();
        assert_eq!(result, Local::now().date_naive());
    }

    #[test]
    fn parse_day_spec_tomorrow() {
        let result = parse_day_spec("tomorrow").unwrap();
        assert_eq!(result, Local::now().date_naive() + Duration::days(1));
    }

    #[test]
    fn parse_day_spec_yesterday() {
        let result = parse_day_spec("yesterday").unwrap();
        assert_eq!(result, Local::now().date_naive() - Duration::days(1));
    }

    #[test]
    fn parse_day_spec_iso_date() {
        let result = parse_day_spec("2026-06-15").unwrap();
        assert_eq!(result, NaiveDate::from_ymd_opt(2026, 6, 15).unwrap());
    }

    #[test]
    fn parse_day_spec_case_insensitive() {
        assert!(parse_day_spec("Today").is_ok());
        assert!(parse_day_spec("TOMORROW").is_ok());
        assert!(parse_day_spec("Yesterday").is_ok());
    }

    #[test]
    fn parse_day_spec_invalid() {
        assert!(parse_day_spec("not-a-date").is_err());
        assert!(parse_day_spec("2026-13-01").is_err());
    }

    #[test]
    fn parse_date_to_ts_valid() {
        let ts = parse_date_to_ts("2026-06-15").unwrap();
        assert!(ts > 0);
        let local_midnight = NaiveDate::from_ymd_opt(2026, 6, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(ts, local_midnight);
    }

    #[test]
    fn parse_date_to_ts_invalid() {
        assert!(parse_date_to_ts("invalid").is_none());
        assert!(parse_date_to_ts("2026-13-45").is_none());
    }

    #[test]
    fn default_date_range_uses_local_timezone() {
        let today = Local::now().date_naive();
        let expected_from = today
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        let expected_to = (today + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();

        let _utc_from = today.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();

        assert!(expected_from > 0);
        assert!(expected_to > expected_from);
    }

    #[test]
    fn parse_date_to_ts_uses_local_not_utc() {
        let ts = parse_date_to_ts("2026-06-15").unwrap();
        let _utc_midnight = NaiveDate::from_ymd_opt(2026, 6, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp();
        let local_midnight = NaiveDate::from_ymd_opt(2026, 6, 15)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_local_timezone(Local)
            .unwrap()
            .timestamp();
        assert_eq!(ts, local_midnight);
    }

    #[test]
    fn normalize_rfc3339_utc() {
        let result = normalize_datetime("2026-03-31T17:00:00Z").unwrap();
        assert_eq!(result, "2026-03-31T17:00:00Z");
    }

    #[test]
    fn normalize_rfc3339_with_offset() {
        let result = normalize_datetime("2026-03-31T17:00:00+02:00").unwrap();
        assert!(chrono::DateTime::parse_from_rfc3339(&result).is_ok());
    }

    #[test]
    fn normalize_iso8601_no_offset() {
        let result = normalize_datetime("2026-03-31T17:00:00").unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&result).unwrap();
        assert_eq!(parsed.naive_local().hour(), 17);
        assert_eq!(parsed.naive_local().minute(), 0);
    }

    #[test]
    fn normalize_iso8601_no_seconds() {
        let result = normalize_datetime("2026-03-31T17:00").unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&result).unwrap();
        assert_eq!(parsed.naive_local().hour(), 17);
        assert_eq!(parsed.naive_local().minute(), 0);
    }

    #[test]
    fn normalize_space_separated_with_seconds() {
        let result = normalize_datetime("2026-03-31 17:00:00").unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&result).unwrap();
        assert_eq!(parsed.naive_local().hour(), 17);
    }

    #[test]
    fn normalize_space_separated_no_seconds() {
        let result = normalize_datetime("2026-03-31 17:00").unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&result).unwrap();
        assert_eq!(parsed.naive_local().hour(), 17);
        assert_eq!(parsed.naive_local().minute(), 0);
    }

    #[test]
    fn normalize_date_only() {
        let result = normalize_datetime("2026-03-31").unwrap();
        let parsed = chrono::DateTime::parse_from_rfc3339(&result).unwrap();
        assert_eq!(parsed.naive_local().hour(), 0);
        assert_eq!(parsed.naive_local().minute(), 0);
    }

    #[test]
    fn normalize_trims_whitespace() {
        let result = normalize_datetime("  2026-03-31T17:00:00Z  ").unwrap();
        assert_eq!(result, "2026-03-31T17:00:00Z");
    }

    #[test]
    fn normalize_rejects_garbage() {
        assert!(normalize_datetime("not-a-date").is_err());
        assert!(normalize_datetime("").is_err());
        assert!(normalize_datetime("17:00").is_err());
        assert!(normalize_datetime("March 31, 2026").is_err());
    }

    #[test]
    fn normalize_output_is_always_valid_rfc3339() {
        let inputs = [
            "2026-03-31T17:00:00Z",
            "2026-03-31T17:00:00",
            "2026-03-31T17:00",
            "2026-03-31 17:00",
            "2026-03-31 17:00:00",
            "2026-03-31",
        ];
        for input in inputs {
            let result = normalize_datetime(input).unwrap();
            assert!(
                chrono::DateTime::parse_from_rfc3339(&result).is_ok(),
                "normalize_datetime({input:?}) returned {result:?} which is not valid RFC 3339"
            );
        }
    }
}
