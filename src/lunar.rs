//! 离线农历和按农历重复的本机节日提醒。
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use tyme4rs::tyme::{lunar::LUNAR_DAY_NAMES, solar::SolarDay, Culture, Tyme};

#[derive(Clone)]
pub struct DayInfo {
    pub month: u32,
    pub day: u32,
    pub leap: bool,
    pub month_days: u32,
    pub label: String,
    pub detail: String,
}

pub fn day_info(date: NaiveDate) -> Option<DayInfo> {
    if !(2..=9998).contains(&date.year()) {
        return None;
    }
    let solar = SolarDay::new(
        date.year() as isize,
        date.month() as usize,
        date.day() as usize,
    )
    .ok()?;
    let lunar = solar.get_lunar_day();
    let month = lunar.get_lunar_month();
    let m = month.get_month() as u32;
    let d = lunar.get_day() as u32;
    let leap = month.get_month_with_leap() < 0;
    let lunar_name = format!("{}{}", month.get_name(), lunar.get_name());
    let eve = m == 12 && d == month.get_day_count() as u32 && month.next(1).get_month() == 1;
    let festival = if eve {
        Some("除夕")
    } else if leap {
        None
    } else {
        match (m, d) {
            (1, 1) => Some("春节"),
            (1, 15) => Some("元宵"),
            (5, 5) => Some("端午"),
            (7, 7) => Some("七夕"),
            (8, 15) => Some("中秋"),
            (9, 9) => Some("重阳"),
            (12, 8) => Some("腊八"),
            _ => None,
        }
    };
    let term_day = solar.get_term_day();
    let term = (term_day.get_day_index() == 0).then(|| term_day.get_solar_term().get_name());
    let ordinary = if d == 1 {
        month.get_name()
    } else {
        LUNAR_DAY_NAMES[d as usize - 1].to_string()
    };
    let label = festival
        .map(str::to_owned)
        .or_else(|| term.clone())
        .unwrap_or(ordinary);
    let mut detail = format!("农历{lunar_name}");
    if let Some(festival) = festival {
        detail.push_str(&format!(" · {festival}"));
    }
    if let Some(term) = term {
        if festival != Some(term.as_str()) {
            detail.push_str(&format!(" · {term}"));
        }
    }
    Some(DayInfo {
        month: m,
        day: d,
        leap,
        month_days: month.get_day_count() as u32,
        label,
        detail,
    })
}

#[derive(Clone)]
pub struct Birthday {
    pub uid: String,
    pub name: String,
    pub month: u32,
    pub day: u32,
    pub leap: bool,
    pub born: Option<NaiveDate>,
    // -1 不提醒，0 当天，1 前一天。
    pub reminder: i32,
    pub last_notified: Option<NaiveDate>,
}
impl Birthday {
    pub fn matches(&self, date: NaiveDate, info: &DayInfo) -> bool {
        if self.born.is_some_and(|born| date < born) {
            return false;
        }
        self.month == info.month
            && self.leap == info.leap
            && (self.day == info.day || self.day == 30 && info.day == 29 && info.month_days == 29)
    }
    pub fn occurrence(
        &self,
        date: NaiveDate,
        info: &DayInfo,
    ) -> Option<crate::local_calendar::Entry> {
        if !self.matches(date, info) {
            return None;
        }
        let start = Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
            .earliest()?
            .timestamp();
        let end = Local
            .from_local_datetime(&date.succ_opt()?.and_hms_opt(0, 0, 0)?)
            .earliest()?
            .timestamp();
        let adjusted = self.day == 30 && info.day == 29;
        Some(crate::local_calendar::Entry {
            event: crate::mail::Meeting {
                uid: self.uid.clone(),
                title: format!(
                    "{} · 节日提醒{}",
                    self.name,
                    if adjusted {
                        "（本月无三十）"
                    } else {
                        ""
                    }
                ),
                start,
                end: Some(end),
                all_day: true,
                at: date.format("%m-%d 全天").to_string(),
            },
            reminder: None,
        })
    }
}

pub struct Draft {
    pub uid: Option<String>,
    pub name: String,
    pub month: String,
    pub day: String,
    pub solar: String,
    pub solar_mode: bool,
    pub leap: bool,
    pub reminder: i32,
    pub error: String,
    pub focused: bool,
    pub confirm_delete: bool,
}
impl Draft {
    pub fn new(date: NaiveDate) -> Self {
        let info = day_info(date);
        Self {
            uid: None,
            name: String::new(),
            month: info.as_ref().map_or(1, |d| d.month).to_string(),
            day: info.as_ref().map_or(1, |d| d.day).to_string(),
            solar: date.format("%Y-%m-%d").to_string(),
            solar_mode: false,
            leap: info.is_some_and(|d| d.leap),
            reminder: 0,
            error: String::new(),
            focused: false,
            confirm_delete: false,
        }
    }
    pub fn edit(b: &Birthday) -> Self {
        Self {
            uid: Some(b.uid.clone()),
            name: b.name.clone(),
            month: b.month.to_string(),
            day: b.day.to_string(),
            solar: b
                .born
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_default(),
            solar_mode: b.born.is_some(),
            leap: b.leap,
            reminder: b.reminder,
            error: String::new(),
            focused: false,
            confirm_delete: false,
        }
    }
    pub fn entry(&self, old: Option<&Birthday>) -> Result<Birthday, String> {
        if self.name.trim().is_empty() {
            return Err("请填写提醒名称".into());
        }
        let (month, day, leap, born) = if self.solar_mode {
            let date = NaiveDate::parse_from_str(self.solar.trim(), "%Y-%m-%d")
                .map_err(|_| "公历日期格式：2026-11-03")?;
            let info = day_info(date).ok_or("日期超出支持范围")?;
            (info.month, info.day, info.leap, Some(date))
        } else {
            let m: u32 = self.month.trim().parse().map_err(|_| "农历月份应为 1–12")?;
            let d: u32 = self.day.trim().parse().map_err(|_| "农历日期应为 1–30")?;
            if !(1..=12).contains(&m) || !(1..=30).contains(&d) {
                return Err("农历月份应为 1–12，日期应为 1–30".into());
            }
            (m, d, self.leap, None)
        };
        let uid = self.uid.clone().unwrap_or_else(|| {
            format!(
                "birthday-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            )
        });
        let unchanged = old
            .is_some_and(|o| o.month == month && o.day == day && o.leap == leap && o.born == born);
        Ok(Birthday {
            uid,
            name: self.name.trim().into(),
            month,
            day,
            leap,
            born,
            reminder: self.reminder,
            last_notified: if unchanged {
                old.and_then(|o| o.last_notified)
            } else {
                None
            },
        })
    }
}

pub fn load() -> Result<Vec<Birthday>, String> {
    let path = crate::model::config_dir().join("lunar-birthdays.json");
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err("无法读取节日提醒".into()),
    };
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&bytes).map_err(|_| "节日提醒文件无法读取")?;
    rows.into_iter()
        .map(|v| {
            let uid = v["uid"]
                .as_str()
                .filter(|s| s.starts_with("birthday-"))
                .ok_or("标识无效")?
                .to_string();
            let name = v["name"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("名称无效")?
                .to_string();
            let month = v["month"]
                .as_u64()
                .filter(|n| (1..=12).contains(n))
                .ok_or("月份无效")? as u32;
            let day = v["day"]
                .as_u64()
                .filter(|n| (1..=30).contains(n))
                .ok_or("日期无效")? as u32;
            let reminder = v["reminder"]
                .as_i64()
                .filter(|n| (-1..=1).contains(n))
                .ok_or("提醒无效")? as i32;
            let parse = |k: &str| -> Result<Option<NaiveDate>, String> {
                v[k].as_str()
                    .map(|s| {
                        NaiveDate::parse_from_str(s, "%Y-%m-%d")
                            .map_err(|_| "日期无效".to_string())
                    })
                    .transpose()
            };
            Ok(Birthday {
                uid,
                name,
                month,
                day,
                leap: v["leap"].as_bool().ok_or("闰月无效")?,
                born: parse("born")?,
                reminder,
                last_notified: parse("last_notified")?,
            })
        })
        .collect()
}
pub fn save(items: &[Birthday]) -> Result<(), String> {
    let rows: Vec<_> = items.iter().map(|b| serde_json::json!({"uid":b.uid,"name":b.name,"month":b.month,"day":b.day,"leap":b.leap,"born":b.born.map(|d|d.to_string()),"reminder":b.reminder,"last_notified":b.last_notified.map(|d|d.to_string())})).collect();
    let path = crate::model::config_dir().join("lunar-birthdays.json");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法创建目录")?;
    crate::model::atomic_write(
        &path,
        &serde_json::to_vec(&rows).map_err(|_| "无法保存")?,
    )
    .map_err(|_| "保存失败，请检查磁盘和文件权限".into())
}
