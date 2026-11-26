//! Calendar dates without a timezone database.
//!
//! Dates come from EXIF (camera local time), from text inside documents, or
//! from file modification times (converted as UTC — may differ by a day
//! from local time near midnight; good enough for naming and grouping).

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

const SECONDS_PER_DAY: i64 = 86_400;
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

impl CivilDate {
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        let valid = (1900..=2200).contains(&year)
            && (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month);
        valid.then_some(Self { year, month, day })
    }

    pub fn from_system_time(time: SystemTime) -> Option<Self> {
        let seconds = i64::try_from(time.duration_since(UNIX_EPOCH).ok()?.as_secs()).ok()?;
        let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
        Self::new(year, month, day)
    }

    /// Parses EXIF `YYYY:MM:DD HH:MM:SS` (also accepts `-` separators).
    pub fn from_exif(value: &str) -> Option<Self> {
        let date = value.trim().get(..10)?;
        let mut parts = date.split([':', '-']);
        let year = parts.next()?.parse().ok()?;
        let month = parts.next()?.parse().ok()?;
        let day = parts.next()?.parse().ok()?;
        Self::new(year, month, day)
    }

    /// Finds the first unambiguous date in free text: ISO `2026-10-18`,
    /// `October 18, 2026` or `18 October 2026`. Numeric `dd/mm` vs `mm/dd`
    /// forms are ambiguous and deliberately ignored.
    pub fn find_in_text(text: &str) -> Option<Self> {
        let lower = text.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|word| !word.is_empty())
            .collect();
        words.iter().enumerate().find_map(|(index, word)| {
            parse_iso(word.trim_matches(|c: char| !c.is_ascii_digit()))
                .or_else(|| parse_month_name(&words, index))
        })
    }

    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    pub fn today() -> Option<Self> {
        Self::from_system_time(SystemTime::now())
    }

    /// Whole days from `earlier` to `self` (negative if `earlier` is later).
    pub fn days_since(self, earlier: Self) -> i64 {
        self.days_from_epoch() - earlier.days_from_epoch()
    }

    /// Howard Hinnant's `days_from_civil`.
    fn days_from_epoch(self) -> i64 {
        let month = i64::from(self.month);
        let year = i64::from(self.year) - i64::from(month <= 2);
        let era = year.div_euclid(400);
        let yoe = year.rem_euclid(400);
        let doy =
            (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }
}

fn parse_iso(word: &str) -> Option<CivilDate> {
    let bytes = word.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    CivilDate::new(
        word[..4].parse().ok()?,
        word[5..7].parse().ok()?,
        word[8..].parse().ok()?,
    )
}

fn parse_month_name(words: &[&str], index: usize) -> Option<CivilDate> {
    let month_of = |word: &str| {
        let word = word.trim_matches('.');
        MONTHS
            .iter()
            .position(|name| word.len() >= 3 && name.starts_with(word))
            .and_then(|i| u8::try_from(i + 1).ok())
    };
    let number = |word: &str| {
        word.trim_matches(|c: char| !c.is_ascii_digit())
            .parse::<i32>()
            .ok()
    };
    let month = month_of(words[index])?;
    // "October 18 2026"
    if let (Some(day), Some(year)) = (
        words.get(index + 1).and_then(|w| number(w)),
        words.get(index + 2).and_then(|w| number(w)),
    ) && let Some(date) = CivilDate::new(year, month, u8::try_from(day).ok()?)
    {
        return Some(date);
    }
    // "18 October 2026"
    let day = number(words.get(index.checked_sub(1)?)?)?;
    let year = number(words.get(index + 1)?)?;
    CivilDate::new(year, month, u8::try_from(day).ok()?)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 → (y, m, d).
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    // Ranges are guaranteed by the algorithm; out-of-range years are
    // rejected by `CivilDate::new`.
    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u8::try_from(month).unwrap_or(1),
        u8::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn converts_unix_time_to_civil_date() {
        let time = UNIX_EPOCH + Duration::from_hours(497_856); // 2026-10-18T00:00:00Z
        assert_eq!(
            CivilDate::from_system_time(time),
            CivilDate::new(2026, 10, 18)
        );
        let leap = UNIX_EPOCH + Duration::from_hours(264_384); // 2000-02-29
        assert_eq!(
            CivilDate::from_system_time(leap),
            CivilDate::new(2000, 2, 29)
        );
    }

    #[test]
    fn parses_exif_dates() {
        assert_eq!(
            CivilDate::from_exif("2024:07:04 18:30:00"),
            CivilDate::new(2024, 7, 4)
        );
        assert_eq!(CivilDate::from_exif("0000:00:00 00:00:00"), None);
    }

    #[test]
    fn finds_dates_in_text() {
        let iso = "Invoice date: 2026-03-14\nDue: 2026-04-14";
        assert_eq!(CivilDate::find_in_text(iso), CivilDate::new(2026, 3, 14));
        assert_eq!(
            CivilDate::find_in_text("Issued October 5, 2025 to"),
            CivilDate::new(2025, 10, 5)
        );
        assert_eq!(
            CivilDate::find_in_text("on 5 Oct. 2025"),
            CivilDate::new(2025, 10, 5)
        );
    }

    #[test]
    fn ignores_ambiguous_and_invalid_dates() {
        assert_eq!(CivilDate::find_in_text("03/04/2025"), None);
        assert_eq!(CivilDate::find_in_text("2025-02-30"), None);
        assert_eq!(CivilDate::find_in_text("may I help"), None);
    }

    #[test]
    fn counts_days_between_dates() {
        let a = CivilDate::new(2024, 2, 28).unwrap();
        let b = CivilDate::new(2024, 3, 1).unwrap();
        assert_eq!(b.days_since(a), 2); // leap year
        assert_eq!(a.days_since(b), -2);
        let epoch = CivilDate::new(1970, 1, 1).unwrap();
        assert_eq!(
            CivilDate::new(2026, 10, 18).unwrap().days_since(epoch),
            1_792_281_600 / 86_400
        );
    }

    #[test]
    fn formats_iso() {
        assert_eq!(CivilDate::new(2026, 1, 9).unwrap().iso(), "2026-01-09");
    }
}
