use anyhow::{Result, bail};

use super::{Field, Schedule};

impl Field {
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

impl Schedule {
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
