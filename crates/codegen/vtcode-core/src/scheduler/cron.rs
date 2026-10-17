use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, Timelike};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub(crate) struct ParsedCron {
    minute: CronField,
    hour: CronField,
    day_of_month: CronField,
    month: CronField,
    day_of_week: CronField,
}

impl ParsedCron {
    pub(crate) fn parse(expression: &str) -> Result<Self> {
        let parts = expression.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 5 {
            bail!("Cron expressions require exactly 5 fields");
        }

        Ok(Self {
            minute: CronField::parse(parts[0], 0, 59, false)?,
            hour: CronField::parse(parts[1], 0, 23, false)?,
            day_of_month: CronField::parse(parts[2], 1, 31, false)?,
            month: CronField::parse(parts[3], 1, 12, false)?,
            day_of_week: CronField::parse(parts[4], 0, 7, true)?,
        })
    }

    pub(crate) fn next_after(&self, after: DateTime<Local>) -> Result<Option<DateTime<Local>>> {
        let mut candidate = after
            .with_second(0)
            .and_then(|value| value.with_nanosecond(0))
            .ok_or_else(|| anyhow!("Failed to normalize cron timestamp"))?
            + ChronoDuration::minutes(1);
        let horizon = candidate + ChronoDuration::days(366 * 5);

        while candidate <= horizon {
            if self.matches(candidate) {
                return Ok(Some(candidate));
            }
            candidate += ChronoDuration::minutes(1);
        }

        Ok(None)
    }

    pub(crate) fn matches(&self, value: DateTime<Local>) -> bool {
        let month = value.month();
        let dom = value.day();
        let minute = value.minute();
        let hour = value.hour();
        let dow = value.weekday().num_days_from_sunday();

        if !self.minute.contains(minute) || !self.hour.contains(hour) || !self.month.contains(month) {
            return false;
        }

        let dom_matches = self.day_of_month.contains(dom);
        let dow_matches = self.day_of_week.contains(dow);

        if self.day_of_month.is_wildcard && self.day_of_week.is_wildcard {
            return true;
        }
        if self.day_of_month.is_wildcard {
            return dow_matches;
        }
        if self.day_of_week.is_wildcard {
            return dom_matches;
        }

        dom_matches || dow_matches
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CronField {
    values: BTreeSet<u32>,
    is_wildcard: bool,
}

impl CronField {
    pub(crate) fn parse(raw: &str, min: u32, max: u32, is_day_of_week: bool) -> Result<Self> {
        if raw.contains(['L', 'W', '?']) {
            bail!("Unsupported cron syntax in field '{raw}'");
        }
        if raw.chars().any(|ch| ch.is_ascii_alphabetic()) {
            bail!("Named cron aliases are not supported in '{raw}'");
        }

        let mut values = BTreeSet::new();
        let mut is_wildcard = false;
        for segment in raw.split(',') {
            let segment = segment.trim();
            if segment.is_empty() {
                bail!("Cron field contains an empty segment");
            }
            if segment == "*" {
                is_wildcard = true;
                values.extend(min..=max);
                continue;
            }

            let (base, step) = if let Some((left, right)) = segment.split_once('/') {
                let step = right
                    .parse::<u32>()
                    .with_context(|| format!("Invalid step value in '{segment}'"))?;
                if step == 0 {
                    bail!("Step value must be greater than zero");
                }
                (left, Some(step))
            } else {
                (segment, None)
            };

            let mut base_values = if base == "*" {
                is_wildcard = true;
                (min..=max).collect::<Vec<_>>()
            } else if let Some((left, right)) = base.split_once('-') {
                let start = parse_cron_number(left, min, max, is_day_of_week)?;
                let end = parse_cron_number(right, min, max, is_day_of_week)?;
                if start > end {
                    bail!("Invalid descending range '{base}'");
                }
                (start..=end).collect::<Vec<_>>()
            } else {
                let start = parse_cron_number(base, min, max, is_day_of_week)?;
                if let Some(step) = step {
                    let mut values = Vec::new();
                    let mut value = start;
                    while value <= max {
                        values.push(value);
                        match value.checked_add(step) {
                            Some(next) => value = next,
                            None => break,
                        }
                    }
                    values
                } else {
                    vec![start]
                }
            };

            if let Some(step) = step
                && (base == "*" || base.contains('-'))
            {
                let mut stepped = Vec::new();
                for (index, value) in base_values.iter().enumerate() {
                    if (index as u32).is_multiple_of(step) {
                        stepped.push(*value);
                    }
                }
                base_values = stepped;
            }

            values.extend(base_values);
        }

        Ok(Self { values, is_wildcard })
    }

    fn contains(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
}

pub(crate) fn parse_cron_number(raw: &str, min: u32, max: u32, is_day_of_week: bool) -> Result<u32> {
    let mut value = raw.parse::<u32>().with_context(|| format!("Invalid cron value '{raw}'"))?;
    if is_day_of_week && value == 7 {
        value = 0;
    }
    if !(min..=max).contains(&value) {
        bail!("Cron value '{raw}' is out of range");
    }
    Ok(value)
}
