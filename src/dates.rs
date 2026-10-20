//! Dates and ISO weeks, in the shapes a human says them.
//!
//! Gryzzly speaks `YYYY-MM-DD`. People say "yesterday", "monday", or
//! "2026-W38". Everything is resolved against the machine's local date, which
//! is the one the person asking is living in.

use chrono::{Datelike, Duration, Local, NaiveDate, Weekday};

use crate::error::{Error, Result};

/// Today, in the server's local timezone.
pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Parse a date: `today`, `yesterday`, `tomorrow`, a weekday name, or
/// `YYYY-MM-DD`.
///
/// A weekday name resolves to the most recent occurrence that is not in the
/// future, so "log 2h on monday" on a Wednesday means this week's Monday.
pub fn parse_date(input: &str) -> Result<NaiveDate> {
    parse_date_from(input, today())
}

/// [`parse_date`] against an explicit "today", which is what the tests use.
pub fn parse_date_from(input: &str, reference: NaiveDate) -> Result<NaiveDate> {
    let raw = input.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return Err(Error::invalid("date is empty"));
    }
    match raw.as_str() {
        "today" | "now" | "aujourd'hui" => return Ok(reference),
        "yesterday" | "hier" => return Ok(reference - Duration::days(1)),
        "tomorrow" | "demain" => return Ok(reference + Duration::days(1)),
        _ => {}
    }
    if let Some(weekday) = parse_weekday(&raw) {
        return Ok(last_weekday_on_or_before(reference, weekday));
    }
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d").map_err(|_| {
        Error::invalid(format!(
            "date {input:?} is not understood. Use YYYY-MM-DD, today, yesterday, or a weekday name."
        ))
    })
}

/// Parse a week designation into its Monday and Sunday.
///
/// Accepted: `2026-W38`, `this week`, `last week`, `next week`, or any date
/// inside the week.
pub fn parse_week(input: &str) -> Result<(NaiveDate, NaiveDate)> {
    parse_week_from(input, today())
}

/// [`parse_week`] against an explicit "today", which is what the tests use.
pub fn parse_week_from(input: &str, reference: NaiveDate) -> Result<(NaiveDate, NaiveDate)> {
    let raw = input.trim().to_ascii_lowercase();
    let monday = match raw.as_str() {
        "" | "this week" | "current" | "this" | "cette semaine" => monday_of(reference),
        "last week" | "previous" | "semaine dernière" | "semaine derniere" => {
            monday_of(reference) - Duration::weeks(1)
        }
        "next week" | "semaine prochaine" => monday_of(reference) + Duration::weeks(1),
        _ => {
            if let Some((year, week)) = parse_iso_week(&raw) {
                NaiveDate::from_isoywd_opt(year, week, Weekday::Mon).ok_or_else(|| {
                    Error::invalid(format!("week {input:?} does not exist in the ISO calendar"))
                })?
            } else {
                monday_of(parse_date_from(&raw, reference).map_err(|_| {
                    Error::invalid(format!(
                        "week {input:?} is not understood. Use YYYY-Www (2026-W38), this week, last week, or a date inside the week."
                    ))
                })?)
            }
        }
    };
    Ok((monday, monday + Duration::days(6)))
}

/// The ISO week label of a date, as `2026-W38`.
pub fn iso_week(date: NaiveDate) -> String {
    let week = date.iso_week();
    format!("{:04}-W{:02}", week.year(), week.week())
}

/// The Monday of the ISO week a date belongs to.
pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// Every date from `from` to `to`, inclusive. Empty when the range is inverted.
pub fn days_between(from: NaiveDate, to: NaiveDate) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let mut cursor = from;
    while cursor <= to {
        out.push(cursor);
        cursor += Duration::days(1);
    }
    out
}

/// True on Saturday and Sunday.
pub fn is_weekend(date: NaiveDate) -> bool {
    matches!(date.weekday(), Weekday::Sat | Weekday::Sun)
}

/// Format a date the way the API wants it.
pub fn api_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn parse_iso_week(raw: &str) -> Option<(i32, u32)> {
    let (year, week) = raw.split_once(['w', 'W'])?;
    let year: i32 = year.trim_end_matches('-').parse().ok()?;
    let week: u32 = week.trim().parse().ok()?;
    (1..=53).contains(&week).then_some((year, week))
}

fn parse_weekday(raw: &str) -> Option<Weekday> {
    let name = raw.trim();
    let english = match name {
        "monday" | "mon" => Some(Weekday::Mon),
        "tuesday" | "tue" | "tues" => Some(Weekday::Tue),
        "wednesday" | "wed" => Some(Weekday::Wed),
        "thursday" | "thu" | "thur" | "thurs" => Some(Weekday::Thu),
        "friday" | "fri" => Some(Weekday::Fri),
        "saturday" | "sat" => Some(Weekday::Sat),
        "sunday" | "sun" => Some(Weekday::Sun),
        _ => None,
    };
    english.or(match name {
        "lundi" => Some(Weekday::Mon),
        "mardi" => Some(Weekday::Tue),
        "mercredi" => Some(Weekday::Wed),
        "jeudi" => Some(Weekday::Thu),
        "vendredi" => Some(Weekday::Fri),
        "samedi" => Some(Weekday::Sat),
        "dimanche" => Some(Weekday::Sun),
        _ => None,
    })
}

fn last_weekday_on_or_before(reference: NaiveDate, weekday: Weekday) -> NaiveDate {
    let delta = i64::from(
        (reference.weekday().num_days_from_monday() + 7 - weekday.num_days_from_monday()) % 7,
    );
    reference - Duration::days(delta)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").expect("test date")
    }

    #[test]
    fn relative_days() {
        let wednesday = date("2026-09-16");
        assert_eq!(parse_date_from("today", wednesday).unwrap(), wednesday);
        assert_eq!(
            parse_date_from("yesterday", wednesday).unwrap(),
            date("2026-09-15")
        );
        assert_eq!(
            parse_date_from("tomorrow", wednesday).unwrap(),
            date("2026-09-17")
        );
    }

    #[test]
    fn a_weekday_never_resolves_into_the_future() {
        let wednesday = date("2026-09-16");
        assert_eq!(
            parse_date_from("monday", wednesday).unwrap(),
            date("2026-09-14")
        );
        assert_eq!(
            parse_date_from("friday", wednesday).unwrap(),
            date("2026-09-11"),
            "friday is still ahead this week, so it means last friday"
        );
        assert_eq!(
            parse_date_from("wednesday", wednesday).unwrap(),
            wednesday,
            "today's weekday is today"
        );
    }

    #[test]
    fn iso_dates_and_nonsense() {
        assert_eq!(parse_date("2026-02-28").unwrap(), date("2026-02-28"));
        let err = parse_date("28/02/2026").unwrap_err().to_string();
        assert!(err.contains("YYYY-MM-DD"), "{err}");
    }

    #[test]
    fn weeks_from_labels_and_from_dates() {
        let wednesday = date("2026-09-16");
        assert_eq!(
            parse_week_from("this week", wednesday).unwrap(),
            (date("2026-09-14"), date("2026-09-20"))
        );
        assert_eq!(
            parse_week_from("last week", wednesday).unwrap(),
            (date("2026-09-07"), date("2026-09-13"))
        );
        assert_eq!(
            parse_week_from("2026-W38", wednesday).unwrap(),
            (date("2026-09-14"), date("2026-09-20"))
        );
        assert_eq!(
            parse_week_from("2026-09-17", wednesday).unwrap(),
            (date("2026-09-14"), date("2026-09-20")),
            "a date inside the week designates the week"
        );
    }

    #[test]
    fn iso_week_labels_round_trip() {
        assert_eq!(iso_week(date("2026-09-16")), "2026-W38");
        // 2027-01-01 is a Friday, still in ISO week 53 of 2026.
        assert_eq!(iso_week(date("2027-01-01")), "2026-W53");
        let (monday, sunday) = parse_week("2026-W53").unwrap();
        assert_eq!(iso_week(monday), "2026-W53");
        assert_eq!(sunday - monday, Duration::days(6));
    }

    #[test]
    fn an_impossible_week_is_refused() {
        assert!(parse_week("2026-W99").is_err());
        assert!(parse_week("2026-W53").is_ok());
        assert!(parse_week("2025-W53").is_err(), "2025 has 52 ISO weeks");
    }

    #[test]
    fn day_ranges_and_weekends() {
        let days = days_between(date("2026-09-14"), date("2026-09-20"));
        assert_eq!(days.len(), 7);
        assert_eq!(days.iter().filter(|d| is_weekend(**d)).count(), 2);
        assert_eq!(
            days_between(date("2026-09-20"), date("2026-09-14")),
            Vec::<NaiveDate>::new(),
            "an inverted range has no days"
        );
    }
}
