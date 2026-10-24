pub(super) fn utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let remainder = seconds.rem_euclid(86400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        remainder / 3600,
        remainder / 60 % 60,
        remainder % 60
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn utc_is_independent_of_timezone_and_handles_leap_days() {
        assert_eq!(super::utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::utc(951827696), "2000-02-29T12:34:56Z");
        assert_eq!(super::utc(1792886400), "2026-10-25T00:00:00Z");
    }
}
