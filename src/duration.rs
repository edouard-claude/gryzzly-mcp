//! Durations, in the shapes a human types them.
//!
//! Gryzzly stores a declaration's duration in seconds. Nobody says "nine
//! thousand seconds", so the tools accept `2h30`, `90m`, `1.5h` or `2:30` and
//! convert once, here.

use crate::error::{Error, Result};

/// The longest duration a single declaration may carry: one full day.
pub const MAX_SECONDS: u32 = 24 * 3600;

/// Parse a human duration into seconds.
///
/// Accepted: `2h`, `2h30`, `2h30m`, `90m`, `1.5h`, `2:30`, `45min`, `3600s`,
/// and a bare number, read as minutes.
pub fn parse(input: &str) -> Result<u32> {
    let raw = input.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return Err(Error::invalid("duration is empty"));
    }

    // 2:30 — hours and minutes, like a clock.
    if let Some((hours, minutes)) = raw.split_once(':') {
        let hours: f64 = parse_number(hours, &raw)?;
        let minutes: f64 = parse_number(minutes, &raw)?;
        if minutes >= 60.0 {
            return Err(Error::invalid(format!(
                "duration {input:?}: {minutes} is not a number of minutes"
            )));
        }
        return finish(hours * 3600.0 + minutes * 60.0, input);
    }

    let mut total = 0.0_f64;
    let mut number = String::new();
    let mut seen_unit = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '0'..='9' | '.' | ',' => number.push(if c == ',' { '.' } else { c }),
            'h' => {
                total += parse_number(&number, &raw)? * 3600.0;
                number.clear();
                seen_unit = true;
            }
            'm' => {
                // Swallow the "in" of "min", and the "s" of "mins".
                while chars.peek().is_some_and(|n| matches!(n, 'i' | 'n' | 's')) {
                    chars.next();
                }
                total += parse_number(&number, &raw)? * 60.0;
                number.clear();
                seen_unit = true;
            }
            's' => {
                while chars.peek().is_some_and(|n| matches!(n, 'e' | 'c')) {
                    chars.next();
                }
                total += parse_number(&number, &raw)?;
                number.clear();
                seen_unit = true;
            }
            ' ' | '\t' => {}
            other => {
                return Err(Error::invalid(format!(
                    "duration {input:?}: unexpected {other:?}. Use forms like 2h, 2h30, 90m, 1.5h or 2:30."
                )));
            }
        }
    }

    if !number.is_empty() {
        // Trailing digits are minutes, whether they follow an hour marker
        // ("2h30") or stand alone ("90").
        total += parse_number(&number, &raw)? * 60.0;
    } else if !seen_unit {
        return Err(Error::invalid(format!("duration {input:?} has no value")));
    }

    finish(total, input)
}

fn parse_number(raw: &str, whole: &str) -> Result<f64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(Error::invalid(format!(
            "duration {whole:?} is missing a number"
        )));
    }
    trimmed
        .parse::<f64>()
        .map_err(|_| Error::invalid(format!("duration {whole:?}: {trimmed:?} is not a number")))
}

fn finish(seconds: f64, input: &str) -> Result<u32> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(Error::invalid(format!(
            "duration {input:?} is not a duration"
        )));
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bounded below by the sign check and above by MAX_SECONDS"
    )]
    let seconds = seconds.round() as i64;
    if seconds == 0 {
        return Err(Error::invalid(format!("duration {input:?} rounds to zero")));
    }
    let seconds = u32::try_from(seconds)
        .map_err(|_| Error::invalid(format!("duration {input:?} is longer than a day")))?;
    if seconds > MAX_SECONDS {
        return Err(Error::invalid(format!(
            "duration {input:?} is longer than 24h, which Gryzzly refuses for one declaration"
        )));
    }
    Ok(seconds)
}

/// Render seconds the way the tools report them back: `7h30`, `45m`, `8h`.
pub fn format(seconds: u32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let rest = seconds % 60;
    match (hours, minutes, rest) {
        (0, 0, s) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (0, m, s) => format!("{m}m{s:02}s"),
        (h, 0, 0) => format!("{h}h"),
        (h, m, _) => format!("{h}h{m:02}"),
    }
}

/// Render seconds as decimal hours, which is what a timesheet total wants.
pub fn hours(seconds: u32) -> f64 {
    f64::from(seconds) / 3600.0
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

    #[test]
    fn the_forms_a_human_types() {
        assert_eq!(parse("2h").unwrap(), 7200);
        assert_eq!(parse("2h30").unwrap(), 9000);
        assert_eq!(parse("2h30m").unwrap(), 9000);
        assert_eq!(parse("90m").unwrap(), 5400);
        assert_eq!(parse("45min").unwrap(), 2700);
        assert_eq!(parse("1.5h").unwrap(), 5400);
        assert_eq!(parse("1,5h").unwrap(), 5400);
        assert_eq!(parse("2:30").unwrap(), 9000);
        assert_eq!(parse("90").unwrap(), 5400);
        assert_eq!(parse("3600s").unwrap(), 3600);
        assert_eq!(parse(" 7 h 30 ").unwrap(), 27000);
    }

    #[test]
    fn nonsense_is_refused_with_examples() {
        let err = parse("two hours").unwrap_err().to_string();
        assert!(err.contains("2h30"), "{err}");
        assert!(parse("").is_err());
        assert!(parse("0m").is_err());
    }

    #[test]
    fn a_duration_longer_than_a_day_is_refused() {
        let err = parse("25h").unwrap_err().to_string();
        assert!(err.contains("24h"), "{err}");
        assert_eq!(parse("24h").unwrap(), MAX_SECONDS);
    }

    #[test]
    fn minutes_above_sixty_in_clock_form_are_refused() {
        assert!(parse("2:90").is_err());
    }

    #[test]
    fn formatting_round_trips_the_common_cases() {
        assert_eq!(format(9000), "2h30");
        assert_eq!(format(7200), "2h");
        assert_eq!(format(2700), "45m");
        assert_eq!(format(30), "30s");
        assert_eq!(format(27000), "7h30");
        assert!((hours(9000) - 2.5).abs() < f64::EPSILON);
    }
}
