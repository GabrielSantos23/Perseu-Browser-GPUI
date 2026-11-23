//! Wall-clock dates for timestamps, in the Windows time zone, worded in
//! English the way the history page reads them.

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// Milliseconds between 1601-01-01 (FILETIME's epoch) and 1970-01-01.
const EPOCH_GAP_MS: u64 = 11_644_473_600_000;
const MS_PER_DAY: u64 = 86_400_000;
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A moment as the clock on this computer showed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub weekday: u16,
    pub hour: u16,
    pub minute: u16,
}

impl LocalTime {
    /// The local time of `ms` milliseconds after the Unix epoch.
    pub fn at(ms: u64) -> Option<Self> {
        let ticks = (ms + EPOCH_GAP_MS) * 10_000;
        let file = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        let mut local = SYSTEMTIME::default();
        // SAFETY: both calls only write the stack structures passed to them.
        unsafe {
            FileTimeToSystemTime(&file, &mut utc).ok()?;
            SystemTimeToTzSpecificLocalTime(None, &utc, &mut local).ok()?;
        }
        Some(Self {
            year: local.wYear,
            month: local.wMonth,
            day: local.wDay,
            weekday: local.wDayOfWeek,
            hour: local.wHour,
            minute: local.wMinute,
        })
    }

    /// The calendar day, which is what a reader means by "yesterday".
    pub fn day_key(self) -> (u16, u16, u16) {
        (self.year, self.month, self.day)
    }

    /// "3:05 PM".
    pub fn time_label(self) -> String {
        let (hour, period) = match self.hour {
            0 => (12, "AM"),
            hour @ 1..=11 => (hour, "AM"),
            12 => (12, "PM"),
            hour => (hour - 12, "PM"),
        };
        format!("{hour}:{:02} {period}", self.minute)
    }
}

/// "Today", "Yesterday", "Monday, 23 November", or with the year when it
/// is not this one.
pub fn day_label(at: LocalTime, now_ms: u64) -> String {
    let today = LocalTime::at(now_ms);
    let yesterday = LocalTime::at(now_ms.saturating_sub(MS_PER_DAY));
    if today.map(LocalTime::day_key) == Some(at.day_key()) {
        return "Today".to_string();
    }
    if yesterday.map(LocalTime::day_key) == Some(at.day_key()) {
        return "Yesterday".to_string();
    }
    let month = MONTHS[usize::from(at.month.clamp(1, 12)) - 1];
    if today.is_some_and(|today| today.year == at.year) {
        let weekday = WEEKDAYS[usize::from(at.weekday % 7)];
        format!("{weekday}, {} {month}", at.day)
    } else {
        format!("{} {month} {}", at.day, at.year)
    }
}

/// Milliseconds since the Unix epoch at the most recent local midnight.
pub fn start_of_today(now_ms: u64) -> u64 {
    let Some(now) = LocalTime::at(now_ms) else {
        return now_ms;
    };
    let into_day = (u64::from(now.hour) * 60 + u64::from(now.minute)) * 60_000 + now_ms % 60_000;
    now_ms.saturating_sub(into_day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_on_a_twelve_hour_clock() {
        let at = |hour, minute| LocalTime {
            year: 2026,
            month: 11,
            day: 23,
            weekday: 1,
            hour,
            minute,
        };
        assert_eq!(at(0, 5).time_label(), "12:05 AM");
        assert_eq!(at(12, 0).time_label(), "12:00 PM");
        assert_eq!(at(15, 30).time_label(), "3:30 PM");
    }

    #[test]
    fn days_are_named_relative_to_now() {
        let now = 1_795_000_000_000;
        let today = LocalTime::at(now).unwrap();
        assert_eq!(day_label(today, now), "Today");
        let yesterday = LocalTime::at(now - MS_PER_DAY).unwrap();
        assert_eq!(day_label(yesterday, now), "Yesterday");
        let old = LocalTime::at(now - 400 * MS_PER_DAY).unwrap();
        assert!(day_label(old, now).ends_with(&old.year.to_string()));
        assert!(start_of_today(now) <= now && now - start_of_today(now) < MS_PER_DAY);
    }
}
