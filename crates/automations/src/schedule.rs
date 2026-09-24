use anyhow::{Result, bail, ensure};
use chrono::{DateTime, Datelike, Days, Local, TimeZone, Utc};

#[derive(Clone, Debug)]
struct Field {
    values: Vec<u32>,
    wildcard: bool,
}

impl Field {
    fn parse(text: &str, min: u32, max: u32) -> Result<Self> {
        let mut values = Vec::new();
        for piece in text.split(',') {
            let (range, step) = match piece.split_once('/') {
                Some((range, step)) => (range, step.parse::<u32>()?),
                None => (piece, 1),
            };
            ensure!(step > 0 && step <= max + 1, "Cron 步长无效");
            let (start, end) = if range == "*" {
                (min, max)
            } else if let Some((start, end)) = range.split_once('-') {
                (start.parse()?, end.parse()?)
            } else {
                let start = range.parse()?;
                (start, if piece.contains('/') { max } else { start })
            };
            ensure!(
                start >= min && end <= max && start <= end,
                "Cron 字段超出范围 {min}–{max}"
            );
            values.extend((start..=end).step_by(step as usize));
        }
        values.sort_unstable();
        values.dedup();
        ensure!(!values.is_empty(), "Cron 字段不能为空");
        Ok(Self {
            values,
            wildcard: text.starts_with('*'),
        })
    }
    fn contains(&self, value: u32) -> bool {
        self.values.contains(&value)
    }
    fn calendar(&self, min: u32, max: u32) -> String {
        if self.values.len() == (max - min + 1) as usize {
            "*".into()
        } else {
            self.values
                .iter()
                .map(|v| format!("{v:02}"))
                .collect::<Vec<_>>()
                .join(",")
        }
    }
    fn plist_values(&self, min: u32, max: u32) -> Vec<Option<u32>> {
        if self.values.len() == (max - min + 1) as usize {
            vec![None]
        } else {
            self.values.iter().copied().map(Some).collect()
        }
    }
}

/// Converts schedules to OS triggers; no application scheduling loop is used.
#[derive(Clone, Debug)]
pub struct Schedule {
    minute: Field,
    hour: Field,
    day: Field,
    month: Field,
    weekday: Field,
}

impl Schedule {
    pub fn parse(value: &str) -> Result<Self> {
        ensure!(value.len() <= 256, "Cron 表达式过长");
        let fields: Vec<_> = value.split_whitespace().collect();
        ensure!(
            fields.len() == 5,
            "Cron 需要五个数字字段：分 时 日 月 星期；支持 *、列表、范围和步长"
        );
        let mut weekday = Field::parse(fields[4], 0, 7)?;
        weekday.values.iter_mut().for_each(|v| {
            if *v == 7 {
                *v = 0;
            }
        });
        weekday.values.sort_unstable();
        weekday.values.dedup();
        Ok(Self {
            minute: Field::parse(fields[0], 0, 59)?,
            hour: Field::parse(fields[1], 0, 23)?,
            day: Field::parse(fields[2], 1, 31)?,
            month: Field::parse(fields[3], 1, 12)?,
            weekday,
        })
    }

    fn date_matches(&self, date: chrono::NaiveDate) -> bool {
        let day = self.day.contains(date.day());
        let weekday = self.weekday.contains(date.weekday().num_days_from_sunday());
        self.month.contains(date.month())
            && if !self.day.wildcard && !self.weekday.wildcard {
                day || weekday
            } else {
                day && weekday
            }
    }

    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let start = after.with_timezone(&Local).date_naive();
        for offset in 0..(366 * 8) {
            let date = start.checked_add_days(Days::new(offset))?;
            if !self.date_matches(date) {
                continue;
            }
            let mut best = None;
            for hour in &self.hour.values {
                for minute in &self.minute.values {
                    let local = Local.from_local_datetime(&date.and_hms_opt(*hour, *minute, 0)?);
                    for value in [local.earliest(), local.latest()].into_iter().flatten() {
                        let utc = value.with_timezone(&Utc);
                        if utc > after && best.is_none_or(|current| utc < current) {
                            best = Some(utc);
                        }
                    }
                }
            }
            if best.is_some() {
                return best;
            }
        }
        None
    }

    fn date_alternatives(&self) -> Vec<(Option<&Field>, Option<&Field>)> {
        if !self.day.wildcard && !self.weekday.wildcard {
            vec![(Some(&self.day), None), (None, Some(&self.weekday))]
        } else {
            vec![(Some(&self.day), Some(&self.weekday))]
        }
    }

    pub fn systemd_calendars(&self) -> Vec<String> {
        self.date_alternatives()
            .into_iter()
            .map(|(day, weekday)| {
                let weekdays = weekday
                    .filter(|f| f.values.len() != 7)
                    .map(|field| {
                        let names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
                        format!(
                            "{} ",
                            field
                                .values
                                .iter()
                                .map(|v| names[*v as usize])
                                .collect::<Vec<_>>()
                                .join(",")
                        )
                    })
                    .unwrap_or_default();
                format!(
                    "{weekdays}*-{}-{} {}:{}:00",
                    self.month.calendar(1, 12),
                    day.map(|f| f.calendar(1, 31)).unwrap_or("*".into()),
                    self.hour.calendar(0, 23),
                    self.minute.calendar(0, 59)
                )
            })
            .collect()
    }

    pub fn launchd_calendar_xml(&self) -> Result<String> {
        let mut entries = Vec::new();
        for (day, weekday) in self.date_alternatives() {
            for minute in self.minute.plist_values(0, 59) {
                for hour in self.hour.plist_values(0, 23) {
                    for month in self.month.plist_values(1, 12) {
                        for day in day.map(|f| f.plist_values(1, 31)).unwrap_or(vec![None]) {
                            for weekday in
                                weekday.map(|f| f.plist_values(0, 6)).unwrap_or(vec![None])
                            {
                                if entries.len() >= 4096 {
                                    bail!("该 Cron 展开超过 4096 项，请简化计划");
                                }
                                let fields = [
                                    ("Minute", minute),
                                    ("Hour", hour),
                                    ("Month", month),
                                    ("Day", day),
                                    ("Weekday", weekday),
                                ]
                                .into_iter()
                                .filter_map(|(key, value)| {
                                    value.map(|value| {
                                        format!("<key>{key}</key><integer>{value}</integer>")
                                    })
                                })
                                .collect::<String>();
                                entries.push(format!("<dict>{fields}</dict>"));
                            }
                        }
                    }
                }
            }
        }
        Ok(format!("<array>{}</array>", entries.join("")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_cron_day_or_weekday_and_steps() {
        let schedule = Schedule::parse("*/15 9 1 * 1").unwrap();
        assert_eq!(
            schedule.systemd_calendars(),
            ["*-*-01 09:00,15,30,45:00", "Mon *-*-* 09:00,15,30,45:00"]
        );
        let xml = schedule.launchd_calendar_xml().unwrap();
        assert_eq!(xml.matches("<dict>").count(), 8);
        assert!(Schedule::parse("60 * * * *").is_err());
        assert!(Schedule::parse("*/0 * * * *").is_err());
        assert!(Schedule::parse("* * * * *; touch /tmp/x").is_err());
    }
    #[test]
    fn next_run_is_strictly_in_the_future() {
        let after = Utc::now();
        let next = Schedule::parse("* * * * *")
            .unwrap()
            .next_after(after)
            .unwrap();
        assert!(next > after && (next - after).num_seconds() <= 60);
        assert_eq!(
            Schedule::parse("0 9 * * 7").unwrap().systemd_calendars(),
            ["Sun *-*-* 09:00:00"]
        );
    }
}
