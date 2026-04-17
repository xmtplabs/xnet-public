use jiff::tz::TimeZone;
use jiff::Timestamp;

/// (display label, offset hours, abbreviation)
const TIMEZONES: &[(&str, i8, &str)] = &[
    ("UTC", 0, "UTC"),
    ("EST", -4, "EDT"),
    ("CST", -5, "CDT"),
    ("PST", -7, "PDT"),
    ("CET", 2, "CEST"),
    ("JST", 9, "JST"),
];

/// Format a cutover timestamp (nanoseconds) into (label, formatted_time) pairs.
pub fn format_cutover_times(ns: u64) -> Vec<(String, String)> {
    let ts = match Timestamp::from_nanosecond(ns as i128) {
        Ok(t) => t,
        Err(_) => return vec![("UTC".to_string(), "invalid timestamp".to_string())],
    };

    let utc_date = ts.to_zoned(TimeZone::UTC).date();

    TIMEZONES
        .iter()
        .map(|(label, offset_h, abbr)| {
            let tz = TimeZone::fixed(jiff::tz::offset(*offset_h));
            let zdt = ts.to_zoned(tz);
            let time_str = zdt.strftime("%H:%M").to_string();
            let local_date = zdt.date();

            let value = if *label == "UTC" {
                time_str
            } else if local_date > utc_date {
                format!("{}+1 {}", time_str, abbr)
            } else if local_date < utc_date {
                format!("{}-1 {}", time_str, abbr)
            } else {
                format!("{} {}", time_str, abbr)
            };
            (label.to_string(), value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutover_timezones() {
        let tz = format_cutover_times(1776443504_000_000_000); // 2026-04-17T16:31:44Z
        let map: std::collections::BTreeMap<_, _> = tz.into_iter().collect();
        assert_eq!(map["UTC"], "16:31");
        assert_eq!(map["EST"], "12:31 EDT");
        assert_eq!(map["PST"], "09:31 PDT");
        assert_eq!(map["JST"], "01:31+1 JST");
    }
}
