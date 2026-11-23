use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Day {
    pub label: String,
    #[serde(default)]
    pub date: String,
    pub high: f32,
    pub low: f32,
    pub code: u8,
    pub rain: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SunTimes {
    pub day_start: i64,
    pub day_end: i64,
    pub sunrise: i64,
    pub sunset: i64,
    pub timezone: String,
}
impl SunTimes {
    pub fn clock(&self, at: i64) -> String {
        self.timezone
            .parse::<chrono_tz::Tz>()
            .ok()
            .and_then(|zone| {
                chrono::DateTime::from_timestamp(at, 0)
                    .map(|t| t.with_timezone(&zone).format("%H:%M").to_string())
            })
            .unwrap_or_else(|| "--:--".into())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub attributions: Vec<String>,
    pub place: String,
    #[serde(default)]
    pub position: Option<Position>,
    #[serde(default)]
    pub auto: bool,
    pub temp: f32,
    pub feels: Option<f32>,
    pub humidity: Option<u32>,
    pub wind: Option<f32>,
    pub code: u8,
    pub day: bool,
    pub high: Option<f32>,
    pub low: Option<f32>,
    #[serde(default)]
    pub sunrise: Option<String>,
    #[serde(default)]
    pub sunset: Option<String>,
    #[serde(default)]
    pub sun_times: Option<SunTimes>,
    pub days: Vec<Day>,
    pub at: i64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Position {
    pub lat: f64,
    pub lon: f64,
    pub label: String,
    #[serde(default)]
    pub region: String,
}
impl Position {
    pub fn valid(&self) -> bool {
        !self.label.trim().is_empty()
            && self.lat.is_finite()
            && self.lon.is_finite()
            && (-90.0..=90.0).contains(&self.lat)
            && (-180.0..=180.0).contains(&self.lon)
    }
}

#[cfg(test)]
impl Snapshot {
    pub fn description(&self) -> &str {
        self.condition
            .as_deref()
            .unwrap_or_else(|| super::describe(i64::from(self.code)))
    }
}
