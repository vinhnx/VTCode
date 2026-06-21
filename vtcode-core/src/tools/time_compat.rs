//! Small compatibility helpers that avoid pulling in heavy dependencies
//! (e.g. `chrono`) for trivial needs.

use std::time::{SystemTime, UNIX_EPOCH};

/// Return the current wall-clock time as an RFC 3339 / ISO 8601 string
/// in UTC with second precision. Used for telemetry/health fields that
/// previously relied on `chrono::Utc::now().to_rfc3339()`.
pub fn current_timestamp_rfc3339() -> String {
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d,
        Err(_) => return "1970-01-01T00:00:00Z".to_string(),
    };
    format_rfc3339_utc_seconds(now.as_secs())
}

fn format_rfc3339_utc_seconds(secs: u64) -> String {
    let (year, month, day, hour, minute, second) = civil_from_unix(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

/// Convert seconds since the Unix epoch to (year, month, day, hour, minute,
/// second) in UTC. Uses Howard Hinnant's `days_from_civil` / `civil_from_days`
/// algorithms, which are well-known and public domain.
fn civil_from_unix(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = (secs % 86_400) as u32;
    let hour = rem / 3600;
    let minute = (rem % 3600) / 60;
    let second = rem % 60;
    let (y, m, d) = civil_from_days(days);
    #[allow(clippy::cast_sign_loss)]
    (y, m as u32, d as u32, hour, minute, second)
}

#[cfg(test)]
const fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64;
    let m = m as i64;
    let d = d as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era as i64 * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i32, i32, i32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as i32, d as i32)
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, format_rfc3339_utc_seconds};

    #[test]
    fn formats_unix_epoch() {
        assert_eq!(format_rfc3339_utc_seconds(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn formats_known_timestamp() {
        // 2024-01-15T12:34:56Z
        #[allow(clippy::cast_sign_loss)]
        let secs = days_from_civil(2024, 1, 15) as u64 * 86_400 + 12 * 3600 + 34 * 60 + 56;
        assert_eq!(format_rfc3339_utc_seconds(secs), "2024-01-15T12:34:56Z");
    }
}
