//! Monthly budgets per provider (PRD stories 53-55, CONTEXT.md "Budget").
//! A job that would take a provider's spend this month to 80% of its budget
//! warns; one that would reach 100%, or starts once 100% is reached, needs
//! the user's confirmation instead of starting.
//!
//! Months are calendar months in UTC, the way providers bill.

use std::time::{Duration, SystemTime};

use crate::{Money, Provider};

/// A provider's monthly spending limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub provider: Provider,
    pub monthly: Money,
}

/// Where spend stands against a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BudgetLevel {
    /// Below 80%: jobs start.
    Under,
    /// From 80%: jobs start with a warning.
    Warning,
    /// At or over 100%: a job needs explicit confirmation.
    Reached,
}

impl Budget {
    /// Where the warning starts, in percent of the budget.
    pub const WARNING_PERCENT: u64 = 80;

    /// Where spend stands once a job estimated at `estimate` runs on top of
    /// `spent`. A budget of zero is reached by anything, so every job asks.
    pub fn level(&self, spent: Money, estimate: Money) -> BudgetLevel {
        let projected = u128::from(spent.saturating_add(estimate).micros());
        let limit = u128::from(self.monthly.micros());
        if projected >= limit {
            BudgetLevel::Reached
        } else if projected * 100 >= limit * u128::from(Self::WARNING_PERCENT) {
            BudgetLevel::Warning
        } else {
            BudgetLevel::Under
        }
    }

    /// How much of the budget `spent` used, in whole percent rounded down
    /// (more than 100 when over).
    pub fn percent_used(&self, spent: Money) -> u64 {
        if self.monthly.is_zero() {
            return if spent.is_zero() { 0 } else { 100 };
        }
        let percent = u128::from(spent.micros()) * 100 / u128::from(self.monthly.micros());
        u64::try_from(percent).unwrap_or(u64::MAX)
    }
}

/// A calendar month, in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month {
    year: i32,
    month: u8,
}

const SECONDS_PER_DAY: u64 = 86_400;

impl Month {
    /// `None` unless `month` is 1 to 12 and the month starts after 1970.
    pub fn new(year: i32, month: u8) -> Option<Month> {
        ((1..=12).contains(&month) && year >= 1970).then_some(Month { year, month })
    }

    /// The month `at` falls in. Times before 1970 fall in January 1970.
    pub fn of(at: SystemTime) -> Month {
        let days = at
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() / SECONDS_PER_DAY);
        let (year, month, _) = civil_from_days(days as i64);
        Month { year, month }
    }

    pub fn year(self) -> i32 {
        self.year
    }

    /// 1 to 12.
    pub fn number(self) -> u8 {
        self.month
    }

    /// Midnight UTC on its first day.
    pub fn start(self) -> SystemTime {
        let days = days_from_civil(self.year, self.month, 1);
        SystemTime::UNIX_EPOCH + Duration::from_secs(days as u64 * SECONDS_PER_DAY)
    }

    /// The start of the next month.
    pub fn end(self) -> SystemTime {
        self.next().start()
    }

    pub fn contains(self, at: SystemTime) -> bool {
        Month::of(at) == self
    }

    pub fn next(self) -> Month {
        match self.month {
            12 => Month {
                year: self.year + 1,
                month: 1,
            },
            month => Month {
                year: self.year,
                month: month + 1,
            },
        }
    }

    /// The month before, or this one for January 1970.
    pub fn previous(self) -> Month {
        match self.month {
            1 if self.year == 1970 => self,
            1 => Month {
                year: self.year - 1,
                month: 12,
            },
            month => Month {
                year: self.year,
                month: month - 1,
            },
        }
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u8;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    } as u8;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year as i32, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget(dollars: &str) -> Budget {
        Budget {
            provider: Provider::Claude,
            monthly: Money::parse(dollars).unwrap(),
        }
    }

    fn money(dollars: &str) -> Money {
        Money::parse(dollars).unwrap()
    }

    #[test]
    fn below_eighty_percent_jobs_just_run() {
        let ten = budget("10");
        assert_eq!(ten.level(Money::ZERO, Money::ZERO), BudgetLevel::Under);
        assert_eq!(
            ten.level(money("7.999999"), Money::ZERO),
            BudgetLevel::Under
        );
        assert_eq!(ten.level(money("7"), money("0.999999")), BudgetLevel::Under);
    }

    #[test]
    fn from_eighty_percent_jobs_warn() {
        let ten = budget("10");
        assert_eq!(ten.level(money("8"), Money::ZERO), BudgetLevel::Warning);
        assert_eq!(ten.level(money("7.5"), money("0.5")), BudgetLevel::Warning);
        assert_eq!(
            ten.level(money("9.999999"), Money::ZERO),
            BudgetLevel::Warning
        );
    }

    #[test]
    fn at_or_over_one_hundred_percent_jobs_need_confirmation() {
        let ten = budget("10");
        assert_eq!(ten.level(money("10"), Money::ZERO), BudgetLevel::Reached);
        assert_eq!(ten.level(money("12"), Money::ZERO), BudgetLevel::Reached);
        // A job that would cross the budget asks before it spends.
        assert_eq!(
            ten.level(money("9.50"), money("0.50")),
            BudgetLevel::Reached
        );
        assert_eq!(ten.level(Money::ZERO, money("10.01")), BudgetLevel::Reached);
    }

    #[test]
    fn a_zero_budget_makes_every_job_ask() {
        assert_eq!(
            budget("0").level(Money::ZERO, Money::ZERO),
            BudgetLevel::Reached
        );
    }

    #[test]
    fn the_warning_boundary_is_exact_for_odd_budgets() {
        // 80% of $0.000003 is $0.0000024: 2 millionths is under, 3 is over.
        let tiny = budget("0.000003");
        assert_eq!(
            tiny.level(Money::from_micros(2), Money::ZERO),
            BudgetLevel::Under
        );
        assert_eq!(
            tiny.level(Money::from_micros(3), Money::ZERO),
            BudgetLevel::Reached
        );
        let odd = budget("12.34");
        assert_eq!(
            odd.level(money("9.871999"), Money::ZERO),
            BudgetLevel::Under
        );
        assert_eq!(odd.level(money("9.872"), Money::ZERO), BudgetLevel::Warning);
    }

    #[test]
    fn percent_used_rounds_down() {
        let ten = budget("10");
        assert_eq!(ten.percent_used(money("7.999")), 79);
        assert_eq!(ten.percent_used(money("8")), 80);
        assert_eq!(ten.percent_used(money("15")), 150);
        assert_eq!(budget("0").percent_used(Money::ZERO), 0);
        assert_eq!(budget("0").percent_used(money("0.01")), 100);
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn months_are_utc_calendar_months() {
        // 2026-10-02T04:43:21Z
        let now = at(1_790_916_201);
        let october = Month::of(now);
        assert_eq!((october.year(), october.number()), (2026, 10));
        assert_eq!(october.start(), at(1_790_812_800)); // 2026-10-01T00:00:00Z
        assert_eq!(october.end(), at(1_793_491_200)); // 2026-11-01T00:00:00Z
        assert!(october.contains(october.start()));
        assert!(!october.contains(october.end()));
        assert!(october.contains(october.end() - Duration::from_millis(1)));
        assert_eq!(Month::of(october.end()), october.next());
    }

    #[test]
    fn months_step_across_years_and_leap_days() {
        let december = Month::new(2026, 12).unwrap();
        assert_eq!(december.next(), Month::new(2027, 1).unwrap());
        assert_eq!(december.next().previous(), december);
        let february = Month::new(2028, 2).unwrap();
        let days = february
            .end()
            .duration_since(february.start())
            .unwrap()
            .as_secs()
            / SECONDS_PER_DAY;
        assert_eq!(days, 29);
        let february = Month::new(2100, 2).unwrap();
        let days = february
            .end()
            .duration_since(february.start())
            .unwrap()
            .as_secs()
            / SECONDS_PER_DAY;
        assert_eq!(days, 28);
        let first = Month::new(1970, 1).unwrap();
        assert_eq!(first.previous(), first);
        assert_eq!(first.start(), SystemTime::UNIX_EPOCH);
        assert_eq!(
            Month::of(SystemTime::UNIX_EPOCH - Duration::from_secs(1)),
            first
        );
    }

    #[test]
    fn only_real_months_exist() {
        assert!(Month::new(2026, 0).is_none());
        assert!(Month::new(2026, 13).is_none());
        assert!(Month::new(1969, 12).is_none());
    }

    #[test]
    fn every_day_maps_back_to_its_month() {
        let mut month = Month::new(1999, 11).unwrap();
        for _ in 0..400 {
            assert_eq!(Month::of(month.start()), month);
            assert_eq!(Month::of(month.end() - Duration::from_secs(1)), month);
            month = month.next();
        }
    }
}
