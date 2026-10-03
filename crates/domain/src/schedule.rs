//! Publish times (PRD story 86, ADR-0006): when a scheduled upload goes
//! live, as the user types it and as the network takes it.
//!
//! The user picks a date and a time in their own time zone; the network
//! takes an instant in UTC (YouTube's `status.publishAt`, ISO 8601). The
//! rules here turn one into the other:
//!
//! - Dates follow the interface language's order (`DateOrder`): month
//!   first in en-US (`10/04/2026`), day first in pt-BR (`04/10/2026`).
//!   Times are `18:30`; en-US also takes `6:30 PM`. Minutes are the
//!   finest step.
//! - A local time a clock change skips (spring forward) moves forward by
//!   the gap; one it repeats (fall back) is the first of the two. The
//!   review shows the resolved time with its zone, so the user sees which.
//! - A publish time must be later than now when the user confirms:
//!   YouTube publishes a past time at once, which is not what a schedule
//!   asked for.

use std::fmt;
use std::time::{Duration, SystemTime};

use jiff::Timestamp;
use jiff::civil::{Date, DateTime, Time};
use jiff::tz::TimeZone;

/// How long after its publish time a scheduled video may stay private
/// before Bardo reads it as kept private by the network rather than
/// about to go live.
pub const SCHEDULE_GRACE: Duration = Duration::from_secs(15 * 60);

/// The time zone publish times are typed and shown in: the system's.
#[derive(Debug, Clone, PartialEq)]
pub struct Zone(TimeZone);

impl Zone {
    pub fn new(zone: TimeZone) -> Self {
        Self(zone)
    }

    pub fn utc() -> Self {
        Self(TimeZone::UTC)
    }

    /// The zone's IANA name (`America/Sao_Paulo`); `None` for a zone known
    /// only by its offset.
    pub fn name(&self) -> Option<&str> {
        self.0.iana_name()
    }
}

/// The order a date's day and month are typed and shown in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DateOrder {
    /// `MM/DD/YYYY` (en-US).
    MonthFirst,
    /// `DD/MM/YYYY` (pt-BR).
    DayFirst,
}

/// A publish time the user typed that cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ScheduleProblem {
    /// Not a date in the language's order, or no such day.
    #[error("not a date")]
    Date,
    /// Not a time of day.
    #[error("not a time")]
    Time,
    /// Already past: the network would publish it at once.
    #[error("the time has passed")]
    Past,
}

impl ScheduleProblem {
    pub const ALL: [ScheduleProblem; 3] = [
        ScheduleProblem::Date,
        ScheduleProblem::Time,
        ScheduleProblem::Past,
    ];

    pub fn code(self) -> &'static str {
        match self {
            ScheduleProblem::Date => "date",
            ScheduleProblem::Time => "time",
            ScheduleProblem::Past => "past",
        }
    }
}

/// A moment as the clock on the wall of `zone` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i16,
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    /// 0 to 23.
    pub hour: u8,
    pub minute: u8,
    /// 1 (Monday) to 7 (Sunday).
    pub weekday: u8,
    /// The zone's IANA name, or its offset when it has no name.
    pub zone: String,
    /// The zone's offset from UTC then, e.g. `UTC−03:00`.
    pub offset: String,
}

impl LocalTime {
    /// The time as a 12-hour clock: the hour (1 to 12) and whether it is
    /// after noon.
    pub fn twelve_hour(&self) -> (u8, bool) {
        let hour = match self.hour % 12 {
            0 => 12,
            hour => hour,
        };
        (hour, self.hour >= 12)
    }
}

fn timestamp(at: SystemTime) -> Timestamp {
    // Times Bardo handles are within a few years of now: always in range.
    Timestamp::try_from(at).unwrap_or(Timestamp::UNIX_EPOCH)
}

/// `at` on the wall clock of `zone`.
pub fn local_time(at: SystemTime, zone: &Zone) -> LocalTime {
    let zoned = timestamp(at).to_zoned(zone.0.clone());
    let seconds = zoned.offset().seconds();
    let sign = if seconds < 0 { '−' } else { '+' };
    let minutes = seconds.unsigned_abs() / 60;
    let offset = format!("UTC{sign}{:02}:{:02}", minutes / 60, minutes % 60);
    LocalTime {
        year: zoned.year(),
        month: zoned.month() as u8,
        day: zoned.day() as u8,
        hour: zoned.hour() as u8,
        minute: zoned.minute() as u8,
        weekday: zoned.weekday().to_monday_one_offset() as u8,
        zone: zone.name().map_or_else(|| offset.clone(), str::to_owned),
        offset,
    }
}

/// The date of `local` as the user types it.
pub fn date_text(local: &LocalTime, order: DateOrder) -> String {
    let (first, second) = match order {
        DateOrder::MonthFirst => (local.month, local.day),
        DateOrder::DayFirst => (local.day, local.month),
    };
    format!("{first:02}/{second:02}/{}", local.year)
}

fn number(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() || text.len() > 4 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_date(text: &str, order: DateOrder) -> Result<Date, ScheduleProblem> {
    let parts: Vec<&str> = text.trim().split(['/', '-', '.']).collect();
    let [first, second, year] = parts.as_slice() else {
        return Err(ScheduleProblem::Date);
    };
    let (Some(first), Some(second), Some(year)) = (number(first), number(second), number(year))
    else {
        return Err(ScheduleProblem::Date);
    };
    let (month, day) = match order {
        DateOrder::MonthFirst => (first, second),
        DateOrder::DayFirst => (second, first),
    };
    // A two-digit year is this century's.
    let year = if year < 100 { 2000 + year } else { year };
    Date::new(
        i16::try_from(year).map_err(|_| ScheduleProblem::Date)?,
        i8::try_from(month).map_err(|_| ScheduleProblem::Date)?,
        i8::try_from(day).map_err(|_| ScheduleProblem::Date)?,
    )
    .map_err(|_| ScheduleProblem::Date)
}

fn parse_time(text: &str) -> Result<Time, ScheduleProblem> {
    // `p.m.` reads as `pm`.
    let text = text.trim().to_ascii_lowercase().replace('.', "");
    let (clock, after_noon) = if let Some(clock) = text.strip_suffix("pm") {
        (clock.trim_end(), Some(true))
    } else if let Some(clock) = text.strip_suffix("am") {
        (clock.trim_end(), Some(false))
    } else {
        (text.as_str(), None)
    };
    let (hour, minute) = match clock.split_once([':', 'h']) {
        Some((hour, "")) => (number(hour), Some(0)),
        Some((hour, minute)) if minute.trim().len() == 2 => (number(hour), number(minute)),
        Some(_) => return Err(ScheduleProblem::Time),
        // `6 pm`; a lone number needs am or pm.
        None if after_noon.is_some() => (number(clock), Some(0)),
        None => return Err(ScheduleProblem::Time),
    };
    let (Some(mut hour), Some(minute)) = (hour, minute) else {
        return Err(ScheduleProblem::Time);
    };
    if let Some(after_noon) = after_noon {
        if !(1..=12).contains(&hour) {
            return Err(ScheduleProblem::Time);
        }
        hour = match (hour, after_noon) {
            (12, false) => 0,
            (12, true) => 12,
            (hour, true) => hour + 12,
            (hour, false) => hour,
        };
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) {
        return Err(ScheduleProblem::Time);
    }
    Ok(Time::constant(hour as i8, minute as i8, 0, 0))
}

/// The instant the user means by `date` and `time` in `zone`. A time a
/// clock change skips moves forward by the gap; a repeated one is the
/// first.
pub fn publish_time(
    date: &str,
    time: &str,
    order: DateOrder,
    zone: &Zone,
) -> Result<SystemTime, ScheduleProblem> {
    let date = parse_date(date, order)?;
    let time = parse_time(time)?;
    let zoned = zone
        .0
        .to_ambiguous_zoned(DateTime::from_parts(date, time))
        .compatible()
        .map_err(|_| ScheduleProblem::Date)?;
    Ok(SystemTime::from(zoned.timestamp()))
}

/// Refuses a publish time that is not later than `now`.
pub fn check_publish_time(at: SystemTime, now: SystemTime) -> Result<(), ScheduleProblem> {
    if at > now {
        Ok(())
    } else {
        Err(ScheduleProblem::Past)
    }
}

/// The publish time a review starts with: tomorrow, at the next whole
/// hour.
pub fn default_publish_time(now: SystemTime, zone: &Zone) -> SystemTime {
    let zoned = timestamp(now).to_zoned(zone.0.clone());
    let next_hour = zoned
        .datetime()
        .date()
        .at(zoned.hour(), 0, 0, 0)
        .checked_add(jiff::Span::new().days(1).hours(1))
        .unwrap_or_else(|_| zoned.datetime());
    zone.0
        .to_ambiguous_zoned(next_hour)
        .compatible()
        .map(|zoned| SystemTime::from(zoned.timestamp()))
        .unwrap_or(now)
}

/// `at` as the network takes it: ISO 8601 in UTC, to the second
/// (`2026-10-04T21:00:00Z`).
pub struct Rfc3339(pub SystemTime);

impl fmt::Display for Rfc3339 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole =
            Timestamp::from_second(timestamp(self.0).as_second()).unwrap_or(Timestamp::UNIX_EPOCH);
        write!(f, "{whole}")
    }
}

/// Reads an ISO 8601 instant the network sent (`2026-10-04T21:00:00Z`,
/// `2026-10-04T21:00:00.000Z`, or with an offset).
pub fn parse_rfc3339(text: &str) -> Option<SystemTime> {
    text.trim().parse::<Timestamp>().ok().map(SystemTime::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(name: &str) -> Zone {
        Zone::new(TimeZone::get(name).unwrap())
    }

    fn utc(text: &str) -> SystemTime {
        parse_rfc3339(text).unwrap()
    }

    #[test]
    fn a_date_follows_the_languages_order() {
        let sao_paulo = zone("America/Sao_Paulo");
        let at = publish_time("04/10/2026", "18:30", DateOrder::DayFirst, &sao_paulo);
        assert_eq!(at, Ok(utc("2026-10-04T21:30:00Z")), "4 October, UTC−3");
        let at = publish_time("10/04/2026", "18:30", DateOrder::MonthFirst, &sao_paulo);
        assert_eq!(at, Ok(utc("2026-10-04T21:30:00Z")), "October 4");
        assert_eq!(
            publish_time("4/10/26", "18:30", DateOrder::DayFirst, &sao_paulo),
            Ok(utc("2026-10-04T21:30:00Z")),
            "short forms"
        );
        assert_eq!(
            publish_time("2026-10-04", "18:30", DateOrder::DayFirst, &sao_paulo),
            Err(ScheduleProblem::Date),
            "a year first is not the language's order"
        );
    }

    #[test]
    fn days_that_do_not_exist_and_junk_are_refused() {
        let z = Zone::utc();
        for date in ["31/02/2027", "13/13/2026", "", "1/2", "a/b/c", "1/2/3/4"] {
            assert_eq!(
                publish_time(date, "10:00", DateOrder::DayFirst, &z),
                Err(ScheduleProblem::Date),
                "{date:?}"
            );
        }
        assert_eq!(
            publish_time("29/02/2028", "10:00", DateOrder::DayFirst, &z),
            Ok(utc("2028-02-29T10:00:00Z")),
            "a leap day"
        );
    }

    #[test]
    fn times_take_a_24_hour_clock_or_am_and_pm() {
        let z = Zone::utc();
        let at = |time: &str| publish_time("10/04/2026", time, DateOrder::MonthFirst, &z);
        assert_eq!(at("18:30"), Ok(utc("2026-10-04T18:30:00Z")));
        assert_eq!(at(" 7:05 "), Ok(utc("2026-10-04T07:05:00Z")));
        assert_eq!(at("18h30"), Ok(utc("2026-10-04T18:30:00Z")));
        assert_eq!(at("18h"), Ok(utc("2026-10-04T18:00:00Z")));
        assert_eq!(at("6:30 PM"), Ok(utc("2026-10-04T18:30:00Z")));
        assert_eq!(at("6pm"), Ok(utc("2026-10-04T18:00:00Z")));
        assert_eq!(at("12:15 am"), Ok(utc("2026-10-04T00:15:00Z")));
        assert_eq!(at("12:15 p.m."), Ok(utc("2026-10-04T12:15:00Z")));
        for time in ["24:00", "18:60", "18:5", "18", "13 pm", "0 am", "", "noon"] {
            assert_eq!(at(time), Err(ScheduleProblem::Time), "{time:?}");
        }
    }

    #[test]
    fn a_skipped_time_moves_forward_and_a_repeated_one_is_the_first() {
        let new_york = zone("America/New_York");
        // 2027-03-14: 02:00 jumps to 03:00 (EST to EDT).
        assert_eq!(
            publish_time("03/14/2027", "2:30", DateOrder::MonthFirst, &new_york),
            Ok(utc("2027-03-14T07:30:00Z")),
            "03:30 EDT"
        );
        // 2026-11-01: 01:30 happens twice (EDT, then EST).
        assert_eq!(
            publish_time("11/01/2026", "1:30", DateOrder::MonthFirst, &new_york),
            Ok(utc("2026-11-01T05:30:00Z")),
            "the first 01:30, EDT"
        );
    }

    #[test]
    fn only_a_time_later_than_now_is_taken() {
        let now = utc("2026-10-03T12:00:00Z");
        assert_eq!(check_publish_time(utc("2026-10-03T12:01:00Z"), now), Ok(()));
        assert_eq!(check_publish_time(now, now), Err(ScheduleProblem::Past));
        assert_eq!(
            check_publish_time(utc("2026-10-03T11:00:00Z"), now),
            Err(ScheduleProblem::Past)
        );
    }

    #[test]
    fn a_moment_shows_on_the_zones_wall_clock_with_the_zone() {
        let local = local_time(utc("2026-10-04T21:30:00Z"), &zone("America/Sao_Paulo"));
        assert_eq!(
            local,
            LocalTime {
                year: 2026,
                month: 10,
                day: 4,
                hour: 18,
                minute: 30,
                weekday: 7,
                zone: "America/Sao_Paulo".into(),
                offset: "UTC−03:00".into(),
            }
        );
        assert_eq!(date_text(&local, DateOrder::DayFirst), "04/10/2026");
        assert_eq!(date_text(&local, DateOrder::MonthFirst), "10/04/2026");
        assert_eq!(local.twelve_hour(), (6, true));

        let summer = local_time(utc("2026-07-01T16:00:00Z"), &zone("America/New_York"));
        assert_eq!((summer.hour, summer.offset.as_str()), (12, "UTC−04:00"));
        assert_eq!(summer.twelve_hour(), (12, true));
        let winter = local_time(utc("2026-12-01T05:00:00Z"), &zone("America/New_York"));
        assert_eq!((winter.hour, winter.offset.as_str()), (0, "UTC−05:00"));
        assert_eq!(winter.twelve_hour(), (12, false));

        let fixed = Zone::new(TimeZone::fixed(jiff::tz::offset(5)));
        let local = local_time(utc("2026-10-04T21:30:00Z"), &fixed);
        assert_eq!(
            local.zone, "UTC+05:00",
            "a zone without a name shows its offset"
        );
    }

    #[test]
    fn typed_text_reads_back_as_the_same_time() {
        let z = zone("Europe/Lisbon");
        let at = utc("2026-10-25T17:45:00Z");
        let local = local_time(at, &z);
        let (hour12, after_noon) = local.twelve_hour();
        let times = [
            format!("{:02}:{:02}", local.hour, local.minute),
            format!(
                "{hour12}:{:02} {}",
                local.minute,
                if after_noon { "PM" } else { "AM" }
            ),
        ];
        for order in [DateOrder::DayFirst, DateOrder::MonthFirst] {
            for time in &times {
                assert_eq!(
                    publish_time(&date_text(&local, order), time, order, &z),
                    Ok(at)
                );
            }
        }
    }

    #[test]
    fn a_review_starts_tomorrow_at_the_next_whole_hour() {
        let z = zone("America/Sao_Paulo");
        assert_eq!(
            default_publish_time(utc("2026-10-03T21:40:12Z"), &z),
            utc("2026-10-04T22:00:00Z")
        );
        assert_eq!(
            default_publish_time(utc("2026-10-03T21:00:00Z"), &z),
            utc("2026-10-04T22:00:00Z")
        );
    }

    #[test]
    fn the_network_gets_utc_to_the_second() {
        let at = utc("2026-10-04T21:30:00Z") + Duration::from_millis(250);
        assert_eq!(Rfc3339(at).to_string(), "2026-10-04T21:30:00Z");
        assert_eq!(
            parse_rfc3339("2026-10-04T21:30:00.000Z"),
            Some(utc("2026-10-04T21:30:00Z"))
        );
        assert_eq!(
            parse_rfc3339("2026-10-04T18:30:00-03:00"),
            Some(utc("2026-10-04T21:30:00Z"))
        );
        assert_eq!(parse_rfc3339("tomorrow"), None);
    }
}
