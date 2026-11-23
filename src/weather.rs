//! Weather and city lookup, kept off the UI thread.
pub(crate) mod qmodel;
mod qsource;
mod icon;
mod indices;
mod hourly;
mod signals;
use crate::paint::{self, Card, H};
use crate::theme::{self, W};
use chrono::Datelike;
use eframe::egui::{self, Color32};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REFRESH: u64 = 900;
const RELOCATE: u64 = 3 * 3600;
const STALE: u64 = 10800;
pub(crate) const MAX_CITIES: usize = 5;
const FORECAST_DAYS: usize = 7;
pub(crate) const WIDTH: f32 = 46.0;
// 暂停预警与分钟降水，保留实现以便问题排查后恢复。
pub(crate) const SIGNALS_ENABLED: bool = false;

// ---------- 页面大图标颜色 ----------
const SUN_C: Color32 = Color32::from_rgb(255, 198, 88);
const RAIN_C: Color32 = Color32::from_rgb(116, 176, 255);
const WARN_C: Color32 = Color32::from_rgb(255, 166, 77);
// Match the visible gray used by the reference weather cards on a black island.
const WEATHER_CARD_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(53, 53, 53, 53);

#[derive(Clone, Debug, PartialEq)]
struct Place {
    name: String,
    region: String,
    lat: f64,
    lon: f64,
}

impl Place {
    fn label(&self) -> String {
        if self.region.is_empty() || self.region == self.name {
            self.name.clone()
        } else {
            format!("{} · {}", self.region, self.name)
        }
    }

    fn same_location(&self, other: &Self) -> bool {
        (self.lat - other.lat).abs() < 0.0001 && (self.lon - other.lon).abs() < 0.0001
    }

    fn json(&self) -> Value {
        json!({"name":self.name,"region":self.region,"lat":self.lat,"lon":self.lon})
    }

    fn parse(v: &Value) -> Option<Self> {
        let lat = v.get("lat")?.as_f64()?;
        let lon = v.get("lon")?.as_f64()?;
        let name = v.get("name")?.as_str()?.trim();
        if name.is_empty()
            || !lat.is_finite()
            || !lon.is_finite()
            || !(-90.0..=90.0).contains(&lat)
            || !(-180.0..=180.0).contains(&lon)
        {
            return None;
        }
        Some(Self {
            name: name.to_owned(),
            region: v
                .get("region")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            lat,
            lon,
        })
    }
}

fn default_place() -> Place {
    Place {
        name: "深圳".into(),
        region: "广东".into(),
        lat: 22.5455,
        lon: 114.0683,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Clear,
    PartlyCloudy,
    Cloudy,
    Fog,
    Rain,
    Snow,
    Thunder,
}
impl Kind {
    fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Clear),
            1 | 2 => Some(Self::PartlyCloudy),
            3 => Some(Self::Cloudy),
            45 | 48 => Some(Self::Fog),
            51..=67 | 80..=82 => Some(Self::Rain),
            71..=77 | 85 | 86 => Some(Self::Snow),
            95..=99 => Some(Self::Thunder),
            _ => None,
        }
    }
}

fn describe(code: i64) -> &'static str {
    match code {
        0 => "晴",
        1 => "晴间多云",
        2 => "多云",
        3 => "阴",
        45 | 48 => "雾",
        51 | 53 => "毛毛雨",
        55 => "强毛毛雨",
        56 | 57 | 66 | 67 => "冻雨",
        61 => "小雨",
        63 => "中雨",
        65 => "大雨",
        71 => "小雪",
        73 => "中雪",
        75 => "大雪",
        77 => "米雪",
        80 => "阵雨",
        81 => "强阵雨",
        82 => "暴雨",
        85 => "阵雪",
        86 => "强阵雪",
        95 => "雷阵雨",
        96 | 99 => "雷暴伴冰雹",
        97 | 98 => "强雷阵雨",
        _ => "未知天气",
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Reading {
    pub temp: i32,
    pub kind: Kind,
    pub day: bool,
    code: i64,
    at: u64,
}
pub(crate) fn preview() -> Reading {
    Reading {
        temp: 23,
        kind: Kind::PartlyCloudy,
        day: true,
        code: 1,
        at: now(),
    }
}

#[derive(Clone, Debug)]
struct DailyForecast {
    date: String,
    code: Option<i64>,
    temp_max: Option<i32>,
    temp_min: Option<i32>,
    rain: Option<u32>,
}

impl DailyForecast {
    fn parse(item: &Value) -> Option<Self> {
        let date = item.get("date")?.as_str()?;
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
        Some(Self {
            date: date.to_owned(),
            code: item
                .get("code")
                .and_then(Value::as_i64)
                .filter(|v| Kind::from_code(*v).is_some()),
            temp_max: item
                .get("temp_max")
                .and_then(Value::as_i64)
                .filter(|v| (-100..=100).contains(v))
                .map(|v| v as i32),
            temp_min: item
                .get("temp_min")
                .and_then(Value::as_i64)
                .filter(|v| (-100..=100).contains(v))
                .map(|v| v as i32),
            rain: item
                .get("rain")
                .and_then(Value::as_u64)
                .filter(|v| *v <= 100)
                .map(|v| v as u32),
        })
    }

    fn json(&self) -> Value {
        json!({"date":self.date,"code":self.code,"temp_max":self.temp_max,"temp_min":self.temp_min,"rain":self.rain})
    }
}

#[derive(Clone, Debug)]
struct SunTimes {
    timezone: String,
    day_start: i64,
    day_end: i64,
    sunrise: i64,
    sunset: i64,
}
impl SunTimes {
    fn parse(value: &Value) -> Option<Self> {
        let timezone = value.get("timezone")?.as_str()?;
        timezone.parse::<chrono_tz::Tz>().ok()?;
        let result = Self {
            timezone: timezone.to_owned(),
            day_start: value.get("day_start")?.as_i64()?,
            day_end: value.get("day_end")?.as_i64()?,
            sunrise: value.get("sunrise")?.as_i64()?,
            sunset: value.get("sunset")?.as_i64()?,
        };
        (result.day_start <= result.sunrise
            && result.sunrise < result.sunset
            && result.sunset <= result.day_end)
            .then_some(result)
    }

    fn json(&self) -> Value {
        json!({"timezone":self.timezone,"day_start":self.day_start,"day_end":self.day_end,"sunrise":self.sunrise,"sunset":self.sunset})
    }

    fn clock(&self, timestamp: i64) -> String {
        self.timezone
            .parse::<chrono_tz::Tz>()
            .ok()
            .and_then(|zone| {
                chrono::DateTime::from_timestamp(timestamp, 0)
                    .map(|at| at.with_timezone(&zone).format("%H:%M").to_string())
            })
            .unwrap_or_else(|| "--:--".into())
    }

    fn progress(&self, timestamp: i64) -> Option<f32> {
        (self.sunrise..=self.sunset)
            .contains(&timestamp)
            .then(|| (timestamp - self.sunrise) as f32 / (self.sunset - self.sunrise) as f32)
    }
}

#[derive(Clone, Debug)]
struct Snapshot {
    place: Place,
    reading: Reading,
    located_at: u64,
    forecast: Vec<DailyForecast>,
    humidity: Option<i32>,
    wind_speed: Option<f64>,
    feels: Option<i32>,
    timezone: Option<String>,
    sun: Option<SunTimes>,
}
impl Snapshot {
    fn parse(value: &Value) -> Option<Self> {
        let place = Place::parse(value.get("place")?)?;
        let r = value.get("reading")?;
        let code = r.get("code")?.as_i64()?;
        let temp = r.get("temp")?.as_i64()?;
        if !(-100..=100).contains(&temp) {
            return None;
        }
        let reading = Reading {
            temp: temp as i32,
            kind: Kind::from_code(code)?,
            code,
            day: r.get("day")?.as_bool()?,
            at: r.get("at")?.as_u64()?,
        };
        let forecast = value
            .get("forecast")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(DailyForecast::parse)
            .take(FORECAST_DAYS)
            .collect();
        Some(Self {
            place,
            reading,
            located_at: value.get("located_at").and_then(Value::as_u64).unwrap_or(0),
            forecast,
            humidity: value
                .get("humidity")
                .and_then(Value::as_i64)
                .filter(|n| (0..=100).contains(n))
                .map(|n| n as i32),
            wind_speed: value
                .get("wind_speed")
                .and_then(Value::as_f64)
                .filter(|n| n.is_finite() && *n >= 0.0),
            feels: value
                .get("feels")
                .and_then(Value::as_i64)
                .filter(|n| (-100..=100).contains(n))
                .map(|n| n as i32),
            timezone: value
                .get("timezone")
                .and_then(Value::as_str)
                .filter(|s| s.parse::<chrono_tz::Tz>().is_ok())
                .map(str::to_owned),
            sun: value.get("sun").and_then(SunTimes::parse),
        })
    }

    fn json(&self) -> Value {
        json!({
            "place":self.place.json(), "located_at":self.located_at,
            "reading":{"temp":self.reading.temp,"code":self.reading.code,"day":self.reading.day,"at":self.reading.at},
            "forecast":self.forecast.iter().map(DailyForecast::json).collect::<Vec<_>>(),
            "humidity":self.humidity,"wind_speed":self.wind_speed,"feels":self.feels,
            "timezone":self.timezone,"sun":self.sun.as_ref().map(SunTimes::json)
        })
    }
}

#[derive(Clone, Debug)]
struct CacheEntry {
    automatic: bool,
    snapshot: Snapshot,
}
impl CacheEntry {
    fn matches(&self, place: Option<&Place>) -> bool {
        match place {
            None => self.automatic,
            Some(p) => !self.automatic && self.snapshot.place.same_location(p),
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

type WeatherResult = (u64, Result<Snapshot, String>);
type SearchResult = (u64, Result<Vec<Place>, String>);

#[derive(Default)]
struct AmbientClock {
    entered: Option<f64>,
    last: Option<f64>,
    time: f64,
}

impl AmbientClock {
    fn advance(&mut self, now: f64, enabled: bool, hovered: bool) -> bool {
        let entered = *self.entered.get_or_insert(now);
        let moving = enabled && (now - entered < 6.0 || hovered);
        if moving {
            if let Some(last) = self.last {
                let delta = now - last;
                if (0.0..=0.2).contains(&delta) { self.time += delta; }
            }
        }
        self.last = moving.then_some(now);
        moving
    }

    fn leave(&mut self) {
        self.entered = None;
        self.last = None;
    }
}

#[derive(Default)]
struct SnowAccumulation {
    place: Option<Place>,
    seconds: f64,
    last: Option<f64>,
}

impl SnowAccumulation {
    fn update(&mut self, snapshot: Option<&Snapshot>, now: f64, active: bool) {
        let Some(snapshot) = snapshot.filter(|s| matches!(background_weather(s.reading.code), Some(BackgroundWeather::Snow))) else {
            *self = Self::default();
            return;
        };
        if !self.place.as_ref().is_some_and(|p| p.same_location(&snapshot.place)) {
            *self = Self { place: Some(snapshot.place.clone()), ..Self::default() };
        }
        if active {
            if let Some(last) = self.last {
                let delta = now - last;
                if (0.0..=0.2).contains(&delta) {
                    self.seconds = (self.seconds + delta).min(20.0);
                }
            }
            self.last = Some(now);
        } else {
            self.last = None;
        }
    }
}

pub(crate) struct State {
    cache_path: Option<PathBuf>,
    manual: bool,
    place: Option<Place>,
    located_at: u64,
    snapshot: Option<Snapshot>,
    saved: Vec<Place>,
    cache: Vec<CacheEntry>,
    last_attempt: Option<Instant>,
    failures: u32,
    network: Option<bool>,
    enabled: bool,
    generation: Arc<AtomicU64>,
    request: Option<mpsc::Receiver<WeatherResult>>,
    search_generation: Arc<AtomicU64>,
    search: Option<mpsc::Receiver<SearchResult>>,
    query: String,
    candidates: Vec<Place>,
    city_choice: Option<Place>,
    status: String,
    search_status: String,
    solar_open: bool,
    solar_at: f64,
    solar_tap: Option<(f32,f32,f64)>,
    picker_open: bool,
    ambient: AmbientClock,
    snow: SnowAccumulation,
    cloud_textures: Option<Option<[egui::TextureHandle; 2]>>,
    source: qsource::Source,
    indices: indices::State,
    signals: signals::State,
    hourly: hourly::State,
    #[cfg(test)]
    manual_signal_refreshes: [usize; 2],
}

impl Default for State {
    fn default() -> Self {
        let cache_path = crate::model::config_dir().join("weather.json");
        let value = std::fs::read(&cache_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .unwrap_or(Value::Null);
        let mut state = Self::from_value(&value);
        state.cache_path = Some(cache_path);
        state
    }
}

impl State {
    fn from_value(v: &Value) -> Self {
        let saved_mode = v.get("manual").and_then(Value::as_bool);
        let manual = saved_mode.unwrap_or(!v.is_null());
        let place = v
            .get("place")
            .and_then(Place::parse)
            .or_else(|| (saved_mode.is_none() && !v.is_null()).then(default_place));
        let snapshot = Snapshot::parse(v)
            .filter(|s| place.as_ref().is_some_and(|p| p.same_location(&s.place)));
        let mut saved = Vec::new();
        for p in v
            .get("saved_places")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Place::parse)
        {
            if saved.len() < MAX_CITIES && !saved.iter().any(|old: &Place| old.same_location(&p)) {
                saved.push(p);
            }
        }
        if manual {
            if let Some(p) = place.as_ref() {
                if !saved.iter().any(|old| old.same_location(p)) {
                    if saved.len() == MAX_CITIES {
                        saved.pop();
                    }
                    saved.insert(0, p.clone());
                }
            }
        }
        let mut cache = Vec::new();
        for item in v
            .get("cached")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(automatic), Some(snapshot)) = (
                item.get("automatic").and_then(Value::as_bool),
                Snapshot::parse(item),
            ) {
                let entry = CacheEntry {
                    automatic,
                    snapshot,
                };
                if !cache.iter().any(|old: &CacheEntry| {
                    old.matches(if automatic {
                        None
                    } else {
                        Some(&entry.snapshot.place)
                    })
                }) {
                    cache.push(entry);
                }
            }
            if cache.len() >= MAX_CITIES + 1 {
                break;
            }
        }
        if let Some(snapshot) = &snapshot {
            let key = if manual { Some(&snapshot.place) } else { None };
            cache.retain(|entry| !entry.matches(key));
            cache.insert(
                0,
                CacheEntry {
                    automatic: !manual,
                    snapshot: snapshot.clone(),
                },
            );
            cache.truncate(MAX_CITIES + 1);
        }
        Self {
            cache_path: None,
            manual,
            place,
            located_at: v.get("located_at").and_then(Value::as_u64).unwrap_or(0),
            snapshot,
            saved,
            cache,
            last_attempt: None,
            failures: 0,
            network: None,
            enabled: false,
            generation: Arc::new(AtomicU64::new(0)),
            request: None,
            search_generation: Arc::new(AtomicU64::new(0)),
            search: None,
            query: String::new(),
            candidates: Vec::new(),
            city_choice: None,
            status: String::new(),
            search_status: String::new(),
            solar_open: false,
            solar_at: 0.0,
            solar_tap: None,
            picker_open: false,
            ambient: AmbientClock::default(),
            snow: SnowAccumulation::default(),
            cloud_textures: None,
            source: qsource::Source::load(),
            indices: indices::State::load(),
            signals: signals::State::default(),
            hourly: hourly::State::load(),
            #[cfg(test)]
            manual_signal_refreshes: [0; 2],
        }
    }

    fn save(&self) -> bool {
        let Some(path) = self.cache_path.as_ref() else {
            return true;
        };
        let mut value = self.snapshot.as_ref().map(Snapshot::json).unwrap_or_else(|| {
            json!({"place":self.place.as_ref().map(Place::json),"located_at":self.located_at,"reading":null,"forecast":[]})
        });
        value["manual"] = json!(self.manual);
        value["saved_places"] = json!(self.saved.iter().map(Place::json).collect::<Vec<_>>());
        value["cached"] = json!(self
            .cache
            .iter()
            .map(|entry| {
                let mut v = entry.snapshot.json();
                v["automatic"] = json!(entry.automatic);
                v
            })
            .collect::<Vec<_>>());
        let (Some(parent), Ok(bytes)) = (path.parent(), serde_json::to_vec(&value)) else {
            return false;
        };
        std::fs::create_dir_all(parent)
            .and_then(|_| crate::model::atomic_write(path, &bytes))
            .is_ok()
    }

    fn current_snapshot(&self) -> Option<&Snapshot> {
        if self.source.is_active() {self.source.snapshot.as_ref()} else {self.snapshot.as_ref()}
    }

    pub fn reading(&self) -> Option<&Reading> {
        self.current_snapshot()
            .filter(|_| self.enabled)
            .map(|s| &s.reading)
    }

    pub(crate) fn cloud_background(&self) -> bool {
        self.reading().is_some_and(|reading| matches!(reading.code, 2 | 3))
    }

    fn cloud_textures(&mut self, ctx: &egui::Context) -> Option<&[egui::TextureHandle; 2]> {
        self.cloud_textures.get_or_insert_with(|| {
            let load = |name, bytes: &[u8]| {
                let image = image::load_from_memory(bytes).ok()?.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
                let texture = ctx.load_texture(name, image, egui::TextureOptions::LINEAR_REPEAT);
                crate::skin_paint::mark_monochrome(texture.id());
                Some(texture)
            };
            Some([
                load("weather-cloud-near", include_bytes!("../assets/weather/cloud-near.png"))?,
                load("weather-cloud-far", include_bytes!("../assets/weather/cloud-far.png"))?,
            ])
        }).as_ref()
    }

    pub fn fresh_reading(&self) -> Option<&Reading> {
        self.reading()
            .filter(|r| now().checked_sub(r.at).is_some_and(|age| age < STALE))
    }

    pub fn stale(&self) -> bool {
        let limit=if self.source.is_active(){3600}else{REFRESH * 2};
        self.reading().is_some_and(|r| now().checked_sub(r.at).is_none_or(|age| age >= limit))
            || (self.source.is_active() && self.source.runtime.current_refresh_failed())
            || (!self.source.is_active() && self.failures >= 2)
    }

    fn invalidate(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.request = None;
        self.last_attempt = None;
        self.failures = 0;
    }

    fn cancel_search(&mut self) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        self.search = None;
        self.candidates.clear();
        self.city_choice = None;
    }

    fn select(&mut self, place: Option<Place>) {
        if self.manual == place.is_some()
            && (place.is_none()
                || self
                    .place
                    .as_ref()
                    .zip(place.as_ref())
                    .is_some_and(|(a, b)| a.same_location(b)))
        {
            self.picker_open = false;
            return;
        }
        self.invalidate();
        self.manual = place.is_some();
        self.snapshot = self
            .cache
            .iter()
            .find(|entry| entry.matches(place.as_ref()))
            .map(|entry| entry.snapshot.clone());
        self.place = place.or_else(|| self.snapshot.as_ref().map(|s| s.place.clone()));
        self.located_at = self.snapshot.as_ref().map(|s| s.located_at).unwrap_or(0);
        self.status = if self.snapshot.is_some() {
            "已显示缓存"
        } else {
            "等待获取天气"
        }
        .into();
        self.solar_open = false;
        self.picker_open = false;
        if !self.save() {
            self.status = "城市已切换，但无法保存设置".into();
        }
    }

    fn add_city(&mut self, place: Place) {
        if self.saved.iter().any(|p| p.same_location(&place)) {
            self.search_status = "该城市已保存，可从列表中切换。".into();
        } else if self.saved.len() >= MAX_CITIES {
            self.search_status = format!("最多保存 {MAX_CITIES} 个城市，请先删除一个。");
        } else {
            self.saved.push(place);
            self.search_status = if self.save() {
                "城市已保存，请在列表中选择。"
            } else {
                "城市已添加，但无法保存设置。"
            }
            .into();
        }
    }

    fn remove_city(&mut self, place: &Place) {
        self.saved.retain(|p| !p.same_location(place));
        self.cache
            .retain(|entry| entry.automatic || !entry.snapshot.place.same_location(place));
        if self.manual && self.place.as_ref().is_some_and(|p| p.same_location(place)) {
            self.select(None);
        } else {
            if !self.save() {
                self.status = "城市已移除，但无法保存设置".into();
            }
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled {
            self.enabled = enabled;
            self.invalidate();
            self.cancel_search();
            self.picker_open = false;
            self.solar_open = false;
            self.indices.leave();
            self.status = (if enabled && self.snapshot.is_some() {
                "已显示缓存"
            } else if enabled {
                "等待获取天气"
            } else {
                "天气已关闭"
            })
            .into();
        }
    }

    fn retry_seconds(&self) -> u64 {
        if self.failures == 0 {
            0
        } else {
            (120u64 << self.failures.saturating_sub(1).min(4)).min(1800)
        }
    }

    fn can_refresh(&self) -> bool {
        if self.source.is_active() {return self.enabled && self.network != Some(false) && self.source.can_refresh();}
        self.enabled
            && self.network != Some(false)
            && self.request.is_none()
            && self.last_attempt.is_none_or(|at| {
                at.elapsed() >= Duration::from_secs(if self.failures > 0 { 120 } else { 30 })
            })
    }

    fn start_refresh(&mut self, ctx: &egui::Context) {
        if self.source.is_active() {if self.can_refresh(){self.source.refresh(ctx);}return;}
        if !self.can_refresh() {
            return;
        }
        self.last_attempt = Some(Instant::now());
        self.status = "正在获取天气…".into();
        if cfg!(test) {
            return;
        }
        let id = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let generation = self.generation.clone();
        let manual = self.manual;
        let place = self.place.clone();
        let located_at = self.located_at;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.request = Some(rx);
        std::thread::spawn(move || {
            let result = fetch(manual, place, located_at, &generation, id);
            let _ = tx.send((id, result));
            ctx.request_repaint();
        });
    }

    fn receive_weather(&mut self, result: WeatherResult) {
        if !self.enabled || result.0 != self.generation.load(Ordering::Relaxed) {
            return;
        }
        match result.1 {
            Ok(snapshot) => {
                self.place = Some(snapshot.place.clone());
                self.located_at = snapshot.located_at;
                let key = if self.manual {
                    Some(&snapshot.place)
                } else {
                    None
                };
                self.cache.retain(|entry| !entry.matches(key));
                self.cache.insert(
                    0,
                    CacheEntry {
                        automatic: !self.manual,
                        snapshot: snapshot.clone(),
                    },
                );
                self.cache.truncate(MAX_CITIES + 1);
                self.snapshot = Some(snapshot);
                self.failures = 0;
                self.status = "天气已更新".into();
                if !self.save() {
                    self.status = "天气已更新，但缓存保存失败".into();
                }
            }
            Err(message) => {
                self.failures = self.failures.saturating_add(1);
                self.status = message;
            }
        }
    }

    pub fn tick(&mut self, enabled: bool, network: Option<bool>, ctx: &egui::Context) {
        self.set_enabled(enabled);
        self.source.tick(enabled,network,ctx);
        self.sync_indices(network, ctx);
        let snapshot=self.current_snapshot().cloned();
        self.hourly.sync(enabled,snapshot.as_ref(),&self.source.config.host,&self.source.config.credential,self.source.is_active(),network);
        self.hourly.poll(ctx);
        self.signals.sync(enabled,snapshot.as_ref(),&self.source.config.host,&self.source.config.credential,self.source.is_active(),network);
        self.signals.poll(ctx);
        if self.source.is_active() {self.network=network;return;}
        if !enabled {
            self.network = network;
            return;
        }
        let received = self.request.as_ref().map(|r| r.try_recv());
        match received {
            Some(Ok(result)) => {
                self.request = None;
                self.receive_weather(result);
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.request = None;
                self.failures = self.failures.saturating_add(1);
                self.status = "天气请求已中断".into();
            }
            _ => {}
        }
        let searched = self.search.as_ref().map(|r| r.try_recv());
        match searched {
            Some(Ok((id, result))) => {
                self.search = None;
                if id == self.search_generation.load(Ordering::Relaxed) {
                    match result {
                        Ok(places) => {
                            self.search_status = if places.is_empty() {
                                "没有找到该城市"
                            } else {
                                "选择地区以添加到城市列表"
                            }
                            .into();
                            self.candidates = places;
                        }
                        Err(message) => {
                            self.search_status = message;
                            self.candidates.clear();
                        }
                    }
                }
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.search = None;
                self.search_status = "城市搜索已中断".into();
            }
            _ => {}
        }
        let recovered = self.network == Some(false) && network == Some(true);
        self.network = network;
        if network == Some(false) && self.request.is_none() {
            self.status = if self.snapshot.is_some() {
                "网络未连接，显示本地缓存"
            } else {
                "网络未连接，等待恢复"
            }
            .into();
        } else if recovered && self.failures == 0 && self.snapshot.is_some() {
            self.status = "已显示缓存".into();
        }
        let needs_update = self.snapshot.as_ref().is_none_or(|s| {
            now()
                .checked_sub(s.reading.at)
                .is_none_or(|age| age >= REFRESH)
                || self.last_attempt.is_none() && !forecast_is_current(s, now())
        });
        let due = if self.failures > 0 {
            recovered
                || self
                    .last_attempt
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(self.retry_seconds()))
        } else {
            needs_update
                && self
                    .last_attempt
                    .is_none_or(|at| at.elapsed() >= Duration::from_secs(30))
        };
        if due && self.request.is_none() && network != Some(false) && !cfg!(test) {
            if recovered {
                self.last_attempt = None;
            }
            self.start_refresh(ctx);
        }
    }

    fn sync_indices(&mut self, network: Option<bool>, ctx: &egui::Context) {
        let snapshot = self.current_snapshot().cloned();
        self.indices.sync(self.enabled, snapshot.as_ref(), &self.source.config.host,
            &self.source.config.credential, self.source.is_active(), network);
        self.indices.poll(ctx);
    }

    fn refresh_indices(&mut self, snapshot: &Snapshot, ctx: &egui::Context, manual: bool) {
        if cfg!(test) { return; }
        let gate = indices::State::new_gate();
        let client = self.source.indices_client(gate.clone());
        self.indices.ensure(snapshot, client, gate, ctx, manual);
    }

    fn refresh_signal(&mut self, snapshot:&Snapshot, ctx:&egui::Context, kind:signals::Kind, manual:bool) {
        if !SIGNALS_ENABLED {return;}
        #[cfg(test)]
        {if manual {self.manual_signal_refreshes[kind.index()]+=1;}let _=(snapshot,ctx);return;}
        #[cfg(not(test))]
        {
        let gate=indices::State::new_gate();
        let client=self.source.indices_client(gate.clone());
        self.signals.ensure(snapshot,client,gate,ctx,kind,manual);
        }
    }

    pub(crate) fn set_page_visible(&mut self,visible:bool) {self.signals.set_visible(visible && SIGNALS_ENABLED);if !visible {self.hourly.leave();self.snow=SnowAccumulation::default();}}

    pub(crate) fn scroll_hourly(&mut self,delta:f32)->bool {self.hourly.scroll(delta)}
    pub(crate) fn hourly_open(&self)->bool {self.hourly.open}

    fn refresh_hourly(&mut self,snapshot:&Snapshot,ctx:&egui::Context,manual:bool) {
        let gate=indices::State::new_gate();let client=self.source.indices_client(gate.clone());
        self.hourly.ensure(snapshot,client,gate,ctx,manual);
    }

    fn start_search(&mut self, ctx: &egui::Context) {
        let query = self.query.trim().to_owned();
        if query.is_empty() {
            self.search_status = "请输入城市名".into();
            return;
        }
        self.cancel_search();
        let id = self.search_generation.load(Ordering::Relaxed);
        let generation = self.search_generation.clone();
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.search = Some(rx);
        self.search_status = "正在搜索城市…".into();
        std::thread::spawn(move || {
            let result = search_places(&query, &generation, id);
            let _ = tx.send((id, result));
            ctx.request_repaint();
        });
    }

    pub(crate) fn settings_closed(&mut self) {self.source.close_draft();}

    fn settings_hero(&mut self, ui: &mut egui::Ui) {
        let ctx=ui.ctx().clone();
        let width=ui.available_width();
        let (rect,hero)=ui.allocate_exact_size(egui::vec2(width,150.0),egui::Sense::hover());
        let c=Card::new(ui,rect.min,rect);
        let enabled=crate::bar_layout::animations_enabled() && ui.style().animation_time>0.0;
        let loading=self.request.is_some() || (self.source.is_active()&&self.source.runtime.loading());
        let active=enabled && (hero.hovered() || loading);
        let now=ui.input(|i|i.time);
        let key=egui::Id::unique("weather-settings-hero-clock");
        let (mut time,last)=ctx.data(|d|d.get_temp::<(f64,f64)>(key)).unwrap_or((0.0,now));
        if active {let elapsed=now-last;if (0.0..=0.2).contains(&elapsed){time+=elapsed;}ctx.request_repaint_after(Duration::from_millis(33));}
        ctx.data_mut(|d|d.insert_temp(key,(time,now)));
        let snapshot=self.current_snapshot().cloned();
        let (top,bottom)=match snapshot.as_ref().map(|s|(s.reading.kind,s.reading.day)) {
            Some((Kind::Clear|Kind::PartlyCloudy,true))=>(Color32::from_rgb(70,146,236),Color32::from_rgb(38,94,186)),
            Some((Kind::Clear|Kind::PartlyCloudy,false))=>(Color32::from_rgb(44,52,104),Color32::from_rgb(20,24,54)),
            Some((Kind::Rain|Kind::Thunder,_))=>(Color32::from_rgb(70,104,152),Color32::from_rgb(36,54,90)),
            Some((Kind::Snow,_))=>(Color32::from_rgb(112,146,196),Color32::from_rgb(66,94,140)),
            Some(_)=>(Color32::from_rgb(104,122,152),Color32::from_rgb(60,72,98)),
            None=>(Color32::from_rgb(86,94,112),Color32::from_rgb(52,58,72)),
        };
        c.vgrad(0.0,0.0,width,150.0,14.0,top,bottom);
        c.glow(width-80.0,64.0,62.0,Color32::from_white_alpha(30));
        let soft=Color32::from_white_alpha(205);let faint=Color32::from_white_alpha(160);
        let Some(snapshot)=snapshot else {
            icon::icon_at(&c,width-74.0,68.0,70.0,icon::Kind::Cloud,Some(time as f32));
            c.text(22.0,40.0,"天气",15.0,W::Semibold,Color32::WHITE,H::Left);
            c.text(22.0,84.0,if self.request.is_some() || self.source.runtime.loading(){"正在获取天气…"}else{"尚无天气数据"},22.0,W::Bold,Color32::WHITE,H::Left);
            c.text(22.0,108.0,"选择定位方式并应用位置后，这里会显示当前天气。",11.5,W::Regular,soft,H::Left);
            return;
        };
        icon::icon_at(&c,width-74.0,66.0,76.0,icon::kind(snapshot.reading.code,snapshot.reading.day),Some(time as f32));
        c.circle(26.0,28.0,3.4,Color32::WHITE);c.poly(&[(23.1,29.5),(28.9,29.5),(26.0,34.0)],Color32::WHITE);c.circle(26.0,28.0,1.3,top);
        let place=c.fit(&snapshot.place.name,15.0,W::Semibold,(width-200.0).max(40.0));
        let place_w=c.text(35.0,35.0,&place,15.0,W::Semibold,Color32::WHITE,H::Left);
        if !snapshot.place.region.is_empty(){c.text(43.0+place_w,35.0,&c.fit(&snapshot.place.region,11.5,W::Regular,(width-220.0-place_w).max(40.0)),11.5,W::Regular,faint,H::Left);}
        let temp=format!("{}°",snapshot.reading.temp);
        let temp_w=c.text(22.0,88.0,&temp,46.0,W::Bold,Color32::WHITE,H::Left)+4.0;
        let condition=if self.source.is_active(){self.source.condition.as_deref().unwrap_or_else(||describe(snapshot.reading.code))}else{describe(snapshot.reading.code)};
        c.text(28.0+temp_w,70.0,&c.fit(condition,14.0,W::Semibold,(width-temp_w-120.0).max(35.0)),14.0,W::Semibold,Color32::WHITE,H::Left);
        if let Some(today)=snapshot.forecast.first(){if let (Some(high),Some(low))=(today.temp_max,today.temp_min){c.text(28.0+temp_w,89.0,&format!("最高 {high}°  最低 {low}°"),11.5,W::Regular,soft,H::Left);}}
        c.fill(12.0,108.0,width-24.0,32.0,10.0,Color32::from_black_alpha(46));
        let stats=[snapshot.feels.map(|v|format!("体感 {v}°")),snapshot.humidity.map(|v|format!("湿度 {v}%")),snapshot.wind_speed.map(|v|format!("风速 {v:.0} km/h")),snapshot.sun.as_ref().map(|sun|format!("日出 {} · 日落 {}",sun.clock(sun.sunrise),sun.clock(sun.sunset)))];
        let mut x=24.0;let limit=width-150.0;
        for text in stats.into_iter().flatten(){let w=c.text_w(&text,11.5,W::Regular);if x+w>limit{break;}if x>24.0{c.fill(x-9.0,117.0,1.0,14.0,0.5,Color32::from_white_alpha(50));}c.text(x,128.5,&text,11.5,W::Regular,soft,H::Left);x+=w+18.0;}
        let ready=self.can_refresh();let (bx,by)=(width-32.0,124.0);
        let response=ui.interact(c.rect(bx-13.0,by-13.0,26.0,26.0),ui.make_persistent_id("weather-hero-refresh"),if ready{egui::Sense::click()}else{egui::Sense::hover()}).on_hover_text("刷新天气");
        let hover=ctx.animate_bool_with_time(response.id,ready&&response.hovered(),0.12);
        c.circle(bx,by,13.0,Color32::from_white_alpha((34.0+40.0*hover) as u8));
        let spin=if loading && enabled {ctx.request_repaint_after(Duration::from_millis(33));time as f32*6.0}else{0.0};
        paint::reset(&c.scaled(bx,by,0.95),bx,by,spin,if ready||loading{Color32::WHITE}else{faint});
        c.text(bx-20.0,128.5,&format!("更新于 {}{}",format_local_time(snapshot.reading.at),if self.stale(){" · 缓存"}else{""}),10.0,W::Regular,faint,H::Right);
        if response.clicked(){self.start_refresh(ui.ctx());}
    }

    pub fn settings_ui(&mut self, ui: &mut egui::Ui, enabled: bool) {
        self.set_enabled(enabled);
        if !enabled {ui.small("开启天气后可设置城市。");return;}
        let applied=self.source.selected==self.source.config.provider;
        if applied {
            self.settings_hero(ui);
            ui.add_space(5.0);
            ui.horizontal_wrapped(|ui|{
                ui.label(egui::RichText::new("天气数据来自").size(11.5).color(ui.visuals().weak_text_color()));
                let (name,url)=if self.source.is_active(){("QWeather","https://www.qweather.com/")}else{("Open-Meteo","https://open-meteo.com/")};
                ui.hyperlink_to(name,url);
                ui.label(egui::RichText::new("· 离线时保留上次天气，温度转灰提示").size(11.5).color(ui.visuals().weak_text_color()));
            });
        }
        let was_qweather=self.source.is_active();
        let selected_before=self.source.selected;
        self.source.ui(ui);
        if self.source.selected!=selected_before && applied {ui.ctx().request_discard("天气来源切换");}
        if was_qweather!=self.source.is_active() {self.invalidate();self.cancel_search();self.picker_open=false;self.solar_open=false;}
        if self.source.selected==crate::qweather::Provider::Qweather || self.source.selected!=self.source.config.provider {return;}
        crate::extras::settings_label(ui,"城市");
        crate::extras::settings_card(ui,|ui|{
            let mut selected:Option<Option<Place>>=None;
            let mut removed=None;
            let auto_cache=self.cache.iter().find(|entry|entry.automatic);
            let auto_detail=if !self.manual {self.place.as_ref().map(|p|format!("按网络出口识别 · 当前为 {}",p.label())).unwrap_or_else(||"按网络出口识别所在城市".into())} else {"按网络出口识别所在城市".into()};
            let (clicked,_)=settings_city_row(ui,"自动定位",&auto_detail,auto_cache.map(|entry|entry.snapshot.reading.temp),!self.manual,false);
            if clicked {selected=Some(None);}
            for place in &self.saved {
                let current=self.manual && self.place.as_ref().is_some_and(|p|p.same_location(place));
                let temp=self.cache.iter().find(|entry|entry.matches(Some(place))).map(|entry|entry.snapshot.reading.temp);
                let (clicked,delete)=settings_city_row(ui,&place.name,&place.region,temp,current,true);
                if clicked{selected=Some(Some(place.clone()));}if delete{removed=Some(place.clone());}
            }
            if let Some(place)=removed {self.remove_city(&place);} else if let Some(place)=selected {self.select(place);}
            ui.small(format!("最多保存 {MAX_CITIES} 个城市。点列表或岛上地点切换；只有当前城市会自动更新。"));
        });
        crate::extras::settings_label(ui,"添加城市");
        crate::extras::settings_card(ui,|ui|{
            ui.horizontal(|ui|{
                ui.label("城市名称");
                let width=(ui.available_width()-48.0-ui.spacing().item_spacing.x).max(100.0);
                let input=ui.add_sized([width,32.0],egui::TextEdit::singleline(&mut self.query).hint_text("输入城市名称搜索"));
                if input.changed(){self.cancel_search();self.search_status.clear();}
                if ui.add_sized([40.0,32.0],egui::Button::new("搜索")).clicked() || (input.lost_focus()&&ui.input(|i|i.key_pressed(egui::Key::Enter))) {self.start_search(ui.ctx());}
            });
            ui.small("输入中英文城市名，搜索后选择具体地区，再点添加。");
            if !self.search_status.is_empty(){ui.small(&self.search_status);}
            if !self.candidates.is_empty(){
                egui::ScrollArea::vertical().id_salt("weather-city-results").max_height(160.0).show(ui,|ui|{
                    for place in &self.candidates {if ui.selectable_label(self.city_choice.as_ref()==Some(place),place.label()).clicked(){self.city_choice=Some(place.clone());}}
                });
            }
            if ui.add_enabled(self.city_choice.is_some(),egui::Button::new("添加")).clicked(){
                if let Some(place)=self.city_choice.clone(){self.add_city(place);self.query.clear();self.cancel_search();}
            }
        });
        crate::extras::settings_label(ui,"更新与数据");
        crate::extras::settings_card(ui,|ui|{
            ui.horizontal(|ui|{ui.label("自动更新");ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.weak("每 15 分钟");});});
            ui.separator();
            ui.horizontal(|ui|{ui.label("离线时");ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.weak("保留上次天气，温度转灰提示");});});
            ui.separator();
            ui.horizontal(|ui|{ui.label("数据来源");ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.hyperlink_to("Open-Meteo · CC BY 4.0","https://open-meteo.com/");});});
            if self.current_snapshot().is_none() && ui.add_enabled(self.can_refresh(),egui::Button::new("刷新天气")).clicked(){self.start_refresh(ui.ctx());}
            if !self.status.is_empty(){ui.small(&self.status);}
        });
    }

    pub fn description(&self) -> String {
        match self.reading() {
            Some(r) => {
                let snapshot=self.current_snapshot();
                let label=snapshot.map(|s|s.place.label()).unwrap_or_default();
                let condition=if self.source.is_active(){self.source.condition.as_deref().unwrap_or_else(||describe(r.code))}else{describe(r.code)};
                format!("{} · {} · {}°C\n更新于 {}{} · {}",label,condition,r.temp,format_local_time(r.at),if self.stale(){" · 缓存"}else{""},if self.source.is_active(){"QWeather"}else{"Open-Meteo"})
            },
            None => if self.source.is_active(){self.source.runtime.error.map(|e|e.text().to_string()).unwrap_or_else(||"请选择和风城市".into())}else{self.status.clone()},
        }
    }

}

fn format_local_time(timestamp: u64) -> String {
    i64::try_from(timestamp)
        .ok()
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "--:--".into())
}

fn active(g: &AtomicU64, id: u64) -> Result<(), String> {
    if g.load(Ordering::Relaxed) == id {
        Ok(())
    } else {
        Err("天气请求已取消".into())
    }
}
fn weather_http_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().try_proxy_from_env(false).timeout(Duration::from_secs(12)).redirects(0).build()
}
fn get(agent: &ureq::Agent, url: &str, g: &AtomicU64, id: u64) -> Result<String, String> {
    active(g, id)?;
    if !url.starts_with("https://") {return Err("天气地址无效".into());}
    let result=agent.get(url).call();
    let response=match result {
        Ok(response)=>response,
        Err(ureq::Error::Status(status,_)) if status<500 => return Err("天气服务拒绝请求".into()),
        Err(_)=>{active(g,id)?;crate::quota::http_agent().get(url).call().map_err(|_|"无法连接天气服务，请检查网络或代理")?}
    };
    use std::io::Read;
    let mut text=String::new();response.into_reader().take(1024*1024).read_to_string(&mut text).map_err(|_|"天气服务返回数据无效")?;
    active(g,id)?;Ok(text)
}
fn json_get(agent: &ureq::Agent, url: &str, g: &AtomicU64, id: u64) -> Result<Value, String> {
    serde_json::from_str(&get(agent, url, g, id)?).map_err(|_| "天气服务返回数据无效".into())
}
fn cities() -> impl Iterator<Item = (Place, String)> {
    include_str!("../assets/cities-cn.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut c = line.split('\t');
            let name = c.next()?;
            let aliases = c.next()?;
            let region = c.next()?;
            let lat = c.next()?.parse::<f64>().ok()?;
            let lon = c.next()?.parse::<f64>().ok()?;
            let _rank = c.next()?;
            let ascii = c.next().unwrap_or("");
            Some((
                Place {
                    name: name.into(),
                    region: region.into(),
                    lat,
                    lon,
                },
                format!("{aliases},{ascii}"),
            ))
        })
}
fn shorten(s: &str) -> &str {
    for suffix in [
        "壮族自治区",
        "回族自治区",
        "维吾尔自治区",
        "特别行政区",
        "自治区",
        "自治州",
        "自治县",
        "地区",
        "新区",
        "省",
        "市",
        "县",
        "区",
        "盟",
        "旗",
    ] {
        if let Some(base) = s.strip_suffix(suffix) {
            if base.chars().count() >= 2 {
                return base;
            }
        }
    }
    s
}
fn search_places(query: &str, g: &AtomicU64, id: u64) -> Result<Vec<Place>, String> {
    active(g, id)?;
    let q = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>();
    let q = q.strip_prefix("中国").unwrap_or(&q);
    let mut exact = Vec::new();
    let mut partial = Vec::new();
    for (place, aliases) in cities() {
        let plain = shorten(q);
        let remainder = q
            .strip_prefix(place.region.as_str())
            .unwrap_or(q)
            .trim_start_matches(|c| c == '省' || c == '市');
        let remainder = [
            "壮族自治区",
            "回族自治区",
            "维吾尔自治区",
            "特别行政区",
            "自治区",
        ]
        .iter()
        .find_map(|prefix| remainder.strip_prefix(*prefix))
        .unwrap_or(remainder);
        let remainder = shorten(remainder);
        let matched = place.name.eq_ignore_ascii_case(plain)
            || aliases
                .split(',')
                .any(|a| !a.is_empty() && a.eq_ignore_ascii_case(plain))
            || (!place.region.is_empty()
                && q.starts_with(&place.region)
                && place.name == remainder);
        if matched {
            exact.push(place);
        } else if place.name.contains(plain)
            || (!place.region.is_empty()
                && q.starts_with(&place.region)
                && place.name.contains(remainder))
        {
            partial.push(place);
        }
    }
    if !exact.is_empty() || !partial.is_empty() {
        exact.extend(partial);
        exact.truncate(30);
        return Ok(exact);
    }
    let mut url = url::Url::parse("https://geocoding-api.open-meteo.com/v1/search")
        .map_err(|_| "城市搜索地址无效")?;
    url.query_pairs_mut()
        .append_pair("name", query)
        .append_pair("count", "10")
        .append_pair("language", "zh")
        .append_pair("format", "json");
    let v = json_get(&crate::quota::http_agent(), url.as_str(), g, id)?;
    Ok(v.get("results").and_then(Value::as_array).into_iter().flatten().filter_map(|v|{
        Place::parse(&json!({"name":v.get("name")?,"region":v.get("admin1").or_else(||v.get("country")).and_then(Value::as_str).unwrap_or(""),"lat":v.get("latitude")?,"lon":v.get("longitude")?}))
    }).collect())
}
fn locate(agent: &ureq::Agent, g: &AtomicU64, id: u64) -> Result<Place, String> {
    use std::io::Read;
    let urls=["https://ipwho.is/?lang=zh-CN","https://free.freeipapi.com/api/v1/json"];
    let proxy=crate::quota::http_agent();
    let mut located=None;
    for client in [agent,&proxy] {
        for (i,url) in urls.iter().enumerate(){
            active(g,id)?;
            let value=client.get(url).call().ok().and_then(|r|serde_json::from_reader::<_,Value>(r.into_reader().take(1024*1024)).ok());
            let Some(v)=value else {continue;};
            if v.get("success").and_then(Value::as_bool)==Some(false){continue;}
            let (lat,lon,name,region)=if i==0 {("latitude","longitude","city","region")}else{("latitude","longitude","cityName","regionName")};
            if let Some(p)=Place::parse(&json!({"name":v[name].as_str().unwrap_or("当前位置"),"region":v[region].as_str().unwrap_or(""),"lat":v[lat],"lon":v[lon]})){located=Some(p);break;}
        }
        if located.is_some(){break;}
    }
    let mut place=located.ok_or("网络定位失败，请手动选择城市")?;
    // Prefer the nearby Chinese name over an IP service's romanized city.
    if let Some((city, _)) = cities()
        .filter(|(c, a)| {
            a.split(',').any(|s| s.eq_ignore_ascii_case(&place.name)) || c.name == place.name
        })
        .map(|(c, a)| {
            let d = (c.lat - place.lat).powi(2) + (c.lon - place.lon).powi(2);
            (c, (a, d))
        })
        .filter(|(_, (_, d))| *d < 0.52)
        .min_by(|a, b| a.1 .1.total_cmp(&b.1 .1))
    {
        place.name = city.name;
        place.region = city.region;
    }
    Ok(place)
}
fn fetch(
    manual: bool,
    place: Option<Place>,
    located_at: u64,
    g: &AtomicU64,
    id: u64,
) -> Result<Snapshot, String> {
    let agent = weather_http_agent();
    let current = now();
    let (place, located_at) =
        if !manual && (place.is_none() || current.saturating_sub(located_at) >= RELOCATE) {
            match locate(&agent, g, id) {
                Ok(p) => (p, current),
                Err(e) => {
                    active(g, id)?;
                    if let Some(p) = place {
                        (p, located_at)
                    } else {
                        return Err(e);
                    }
                }
            }
        } else {
            (place.ok_or("请选择天气城市")?, located_at)
        };
    active(g, id)?;
    let mut url =
        url::Url::parse("https://api.open-meteo.com/v1/forecast").map_err(|_| "天气地址无效")?;
    url.query_pairs_mut()
        .append_pair("latitude", &place.lat.to_string())
        .append_pair("longitude", &place.lon.to_string())
        .append_pair("current", "temperature_2m,weather_code,is_day,relative_humidity_2m,wind_speed_10m,apparent_temperature")
        .append_pair("daily", "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,sunrise,sunset")
        .append_pair("forecast_days", "7")
        .append_pair("timezone", "auto");
    let value = json_get(&agent, url.as_str(), g, id)?;
    parse_response(place, located_at, &value, now())
}

fn number(value: &Value, min: f64, max: f64) -> Option<f64> {
    value
        .as_f64()
        .filter(|n| n.is_finite() && (min..=max).contains(n))
}

fn parse_response(
    place: Place,
    located_at: u64,
    value: &Value,
    at: u64,
) -> Result<Snapshot, String> {
    let current = value.get("current").ok_or("天气数据不完整")?;
    let temp = number(&current["temperature_2m"], -100.0, 100.0).ok_or("天气温度无效")?;
    let code = current["weather_code"].as_i64().ok_or("天气代码缺失")?;
    let kind = Kind::from_code(code).ok_or("天气代码不受支持")?;
    let day = match current["is_day"].as_i64() {
        Some(1) => true,
        Some(0) => false,
        _ => return Err("天气昼夜数据缺失".into()),
    };
    let daily = &value["daily"];
    let mut forecast = Vec::new();
    for index in 0..FORECAST_DAYS {
        let Some(date) = daily["time"][index].as_str() else {
            continue;
        };
        if chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
            continue;
        }
        let code = daily["weather_code"][index]
            .as_i64()
            .filter(|v| Kind::from_code(*v).is_some());
        let temp_max =
            number(&daily["temperature_2m_max"][index], -100.0, 100.0).map(|n| n.round() as i32);
        let temp_min =
            number(&daily["temperature_2m_min"][index], -100.0, 100.0).map(|n| n.round() as i32);
        let rain = daily["precipitation_probability_max"][index]
            .as_u64()
            .filter(|n| *n <= 100)
            .map(|n| n as u32);
        forecast.push(DailyForecast {
            date: date.into(),
            code,
            temp_max,
            temp_min,
            rain,
        });
    }
    let timezone = value["timezone"]
        .as_str()
        .filter(|zone| zone.parse::<chrono_tz::Tz>().is_ok())
        .map(str::to_owned);
    Ok(Snapshot {
        place,
        reading: Reading {
            temp: temp.round() as i32,
            kind,
            day,
            code,
            at,
        },
        located_at,
        forecast,
        humidity: current["relative_humidity_2m"]
            .as_i64()
            .filter(|n| (0..=100).contains(n))
            .map(|n| n as i32),
        wind_speed: number(&current["wind_speed_10m"], 0.0, 500.0),
        feels: number(&current["apparent_temperature"], -100.0, 100.0).map(|n| n.round() as i32),
        timezone,
        sun: parse_sun_times(value),
    })
}

fn parse_sun_times(value: &Value) -> Option<SunTimes> {
    use chrono::TimeZone;
    let timezone = value["timezone"].as_str()?;
    let zone = timezone.parse::<chrono_tz::Tz>().ok()?;
    let date =
        chrono::NaiveDate::parse_from_str(value["daily"]["time"][0].as_str()?, "%Y-%m-%d").ok()?;
    let local = |time: chrono::NaiveDateTime| {
        zone.from_local_datetime(&time)
            .earliest()
            .map(|t| t.timestamp())
    };
    let day_start = local(date.and_hms_opt(0, 0, 0)?)?;
    let day_end = local(date.succ_opt()?.and_hms_opt(0, 0, 0)?)?;
    let sunrise = local(
        chrono::NaiveDateTime::parse_from_str(
            value["daily"]["sunrise"][0].as_str()?,
            "%Y-%m-%dT%H:%M",
        )
        .ok()?,
    )?;
    let sunset = local(
        chrono::NaiveDateTime::parse_from_str(
            value["daily"]["sunset"][0].as_str()?,
            "%Y-%m-%dT%H:%M",
        )
        .ok()?,
    )?;
    (day_start <= sunrise && sunrise < sunset && sunset <= day_end).then(|| SunTimes {
        timezone: timezone.into(),
        day_start,
        day_end,
        sunrise,
        sunset,
    })
}

fn place_date(timezone: Option<&str>, timestamp: u64) -> Option<chrono::NaiveDate> {
    i64::try_from(timestamp)
        .ok()
        .and_then(|t| chrono::DateTime::from_timestamp(t, 0))
        .and_then(|time| {
            timezone
                .and_then(|z| z.parse::<chrono_tz::Tz>().ok())
                .map(|z| time.with_timezone(&z).date_naive())
        })
}

fn forecast_is_current(snapshot: &Snapshot, timestamp: u64) -> bool {
    let today = place_date(snapshot.timezone.as_deref(), timestamp);
    let first = snapshot
        .forecast
        .first()
        .and_then(|day| chrono::NaiveDate::parse_from_str(&day.date, "%Y-%m-%d").ok());
    today.is_some() && today == first
}

fn forecast_label(date: &str, timezone: Option<&str>, timestamp: u64) -> String {
    let Some(date) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok() else {
        return "—".into();
    };
    if let Some(today) = place_date(timezone, timestamp) {
        if date == today {
            return "今天".into();
        }
        if today.succ_opt() == Some(date) {
            return "明天".into();
        }
    }
    format!(
        "周{}",
        ["一", "二", "三", "四", "五", "六", "日"][date.weekday().num_days_from_monday() as usize]
    )
}

/// Compact weather glyphs use distinct accents and inherit the island's skin palette.
pub(crate) fn draw(c: &Card, x: f32, y: f32, reading: &Reading) {
    draw_with_stale(c, x, y, reading, false);
}

pub(crate) fn draw_with_stale(c: &Card, x: f32, y: f32, reading: &Reading, stale: bool) {
    let color = if stale { theme::GRAY } else { theme::WHITE };
    let ix = x - WIDTH / 2.0 + 9.0;
    icon::icon_at(c, ix, y, 15.0, icon::kind(reading.code, reading.day), None);
    let text = format!("{}°", reading.temp);
    let available = WIDTH - 21.0;
    let measured = c.text_w(&text, 11.0, W::Semibold);
    let size = if measured > available {
        11.0 * available / measured
    } else {
        11.0
    };
    c.text(
        x - WIDTH / 2.0 + 21.0,
        y + 5.0,
        &text,
        size,
        W::Semibold,
        color,
        H::Left,
    );
}

fn settings_city_row(ui:&mut egui::Ui,title:&str,detail:&str,temp:Option<i32>,selected:bool,removable:bool)->(bool,bool) {
    let width=ui.available_width();
    let (rect,response)=ui.allocate_exact_size(egui::vec2(width,48.0),egui::Sense::click());
    let ctx=ui.ctx();
    let accent=crate::settings_theme::color(ctx,Color32::from_rgb(96,164,255),Color32::from_rgb(34,108,220));
    let base=crate::settings_theme::color(ctx,Color32::from_rgb(45,45,48),Color32::WHITE);
    let fill=if selected{paint::mix(base,accent,0.14)}else if response.hovered(){crate::settings_theme::color(ctx,Color32::from_rgb(52,52,56),Color32::from_rgb(242,244,248))}else{base};
    let painter=ui.painter().with_clip_rect(rect);
    painter.rect(rect,8.0,fill,egui::Stroke::new(if selected{1.2}else{0.7},if selected{accent}else{crate::settings_theme::color(ctx,Color32::from_rgb(65,65,71),Color32::from_rgb(226,229,234))}),egui::StrokeKind::Inside);
    let center=egui::pos2(rect.left()+18.0,rect.center().y);
    painter.circle_stroke(center,7.0,egui::Stroke::new(1.2,if selected{accent}else{ui.visuals().weak_text_color()}));
    if selected{painter.circle_filled(center,3.5,accent);}
    let max_text=(width-if removable{130.0}else{90.0}).max(30.0);
    let card=Card::new(ui,rect.min,rect);
    let title=card.fit(title,12.5,W::Semibold,max_text);
    let detail=card.fit(detail,10.0,W::Regular,max_text);
    card.text(34.0,20.0,&title,12.5,W::Semibold,if selected{accent}else{ui.visuals().text_color()},H::Left);
    card.text(34.0,35.0,&detail,10.0,W::Regular,ui.visuals().weak_text_color(),H::Left);
    if let Some(temp)=temp {card.text(width-if removable{55.0}else{12.0},29.0,&format!("{temp}°"),12.0,W::Semibold,ui.visuals().text_color(),H::Right);}
    let removed=if removable {let r=egui::Rect::from_center_size(egui::pos2(rect.right()-21.0,rect.center().y),egui::vec2(32.0,30.0));let hit=ui.interact(r,response.id.with("remove"),egui::Sense::click()).on_hover_text("删除城市");painter.text(r.center(),egui::Align2::CENTER_CENTER,"×",egui::FontId::proportional(17.0),ui.visuals().weak_text_color());hit.clicked()}else{false};
    (response.clicked()&&!removed,removed)
}

fn tile(c: &Card, x: f32, y: f32, label: &str, value: &str, glyph: impl Fn(&Card, f32, f32)) {
    tile_with_value_size(c,x,y,label,value,11.5,glyph);
}

fn tile_with_value_size(c: &Card, x: f32, y: f32, label: &str, value: &str, value_size:f32, glyph: impl Fn(&Card, f32, f32)) {
    c.fill(x, y, 84.0, 34.0, 9.0, WEATHER_CARD_HIGHLIGHT);
    glyph(c, x + 14.0, y + 17.0);
    c.text(x + 27.0, y + 14.0, label, 9.0, W::Regular, theme::GRAY, H::Left);
    c.text(x + 27.0, y + 28.5, &c.fit(value, value_size, W::Semibold, 53.0), value_size, W::Semibold, theme::WHITE, H::Left);
}

fn thermo_glyph(c: &Card, x: f32, y: f32) {
    c.stroke(x-2.0,y-6.5,4.0,9.0,2.0,1.1,WARN_C);
    c.circle(x,y+3.6,2.9,WARN_C);c.seg((x,y-2.5),(x,y+2.0),1.4,WARN_C);
}
fn warning_severity(value:&str)->&str {match value {"extreme"=>"特别严重","severe"=>"严重","moderate"=>"中等","minor"=>"一般",other=>other}}
fn life_index_caution(kind:&str,category:&str)->bool {
    match kind {
        "3" => matches!(category,"寒冷"|"炎热"),
        "9"|"7" => matches!(category,"较易发"|"易发"|"极易发"),
        "16"|"5" => matches!(category,"强"|"很强"|"极强"),
        _ => false,
    }
}
fn life_index_glyph(c:&Card, x:f32, y:f32, kind:&str, ink:Color32) {
    match kind {
        "3" => {
            c.path(&[(x-5.0,y-9.0),(x-10.0,y-6.0),(x-13.0,y-1.0),(x-9.0,y+1.0),(x-7.0,y-2.0),(x-7.0,y+10.0),(x+7.0,y+10.0),(x+7.0,y-2.0),(x+9.0,y+1.0),(x+13.0,y-1.0),(x+10.0,y-6.0),(x+5.0,y-9.0)],1.5,ink);
            c.arc(x,y-9.0,5.0,0.0,std::f32::consts::PI,1.5,ink);
        },
        "9" => {
            use std::f32::consts::PI;
            c.seg((x-7.5,y+0.5),(x+0.5,y-7.5),1.5,ink);
            c.arc(x+4.0,y-4.0,5.0,-3.0*PI/4.0,PI/4.0,1.5,ink);
            c.seg((x+7.5,y-0.5),(x-0.5,y+7.5),1.5,ink);
            c.arc(x-4.0,y+4.0,5.0,PI/4.0,5.0*PI/4.0,1.5,ink);
            c.seg((x-3.5,y-3.5),(x+3.5,y+3.5),1.4,ink);
        },
        "16" => {
            c.ring(x,y-1.0,4.2,1.5,ink);
            for a in 0..8 {let t=a as f32*std::f32::consts::TAU/8.0;c.seg((x+t.cos()*7.0,y-1.0+t.sin()*7.0),(x+t.cos()*10.0,y-1.0+t.sin()*10.0),1.4,ink);}
        },
        _ => {
            c.ring(x,y,3.0,1.4,ink);
            for (dx,dy) in [(0.0,-6.0),(6.0,0.0),(0.0,6.0),(-6.0,0.0)] {c.ring(x+dx,y+dy,3.0,1.3,ink);}
            c.seg((x,y+9.0),(x,y+12.0),1.3,ink);
        }
    }
}
fn sunrise_glyph(c:&Card,x:f32,y:f32,rise:f32) {
    c.clipped(x-8.0,y-12.0,16.0,14.5).ring(x,y+2.5-2.4*rise,4.2,1.4,SUN_C);
    c.seg((x-6.5,y+2.5),(x+6.5,y+2.5),1.3,SUN_C);
    for a in [-2.36_f32,-1.57,-0.79] {c.seg((x+a.cos()*5.6,y+2.5+a.sin()*5.6),(x+a.cos()*7.2,y+2.5+a.sin()*7.2),1.1,SUN_C);}
}

#[derive(Clone, Copy)]
enum BackgroundWeather {
    Clear,
    PartlyCloudy,
    Cloudy(bool),
    Fog,
    Rain(usize),
    Snow,
    Thunder,
}

fn background_weather(code: i64) -> Option<BackgroundWeather> {
    match code {
        0 => Some(BackgroundWeather::Clear),
        1 => Some(BackgroundWeather::PartlyCloudy),
        2 => Some(BackgroundWeather::Cloudy(false)),
        3 => Some(BackgroundWeather::Cloudy(true)),
        45 | 48 => Some(BackgroundWeather::Fog),
        51 | 53 | 56 | 61 | 66 | 80 => Some(BackgroundWeather::Rain(32)),
        55 | 57 | 63 | 67 | 81 => Some(BackgroundWeather::Rain(52)),
        65 | 82 => Some(BackgroundWeather::Rain(76)),
        71..=77 | 85 | 86 => Some(BackgroundWeather::Snow),
        95..=99 => Some(BackgroundWeather::Thunder),
        _ => None,
    }
}

fn paint_cloud_textures(c: &Card, textures: &[egui::TextureHandle; 2], time: f64, overcast: bool) {
    let rect = c.rect(0.0, 0.0, crate::app::PW, crate::app::WEATHER_H);
    let rows = crate::shape::sections(rect, 0.0, 0.5, crate::app::RAD * c.s.min(1.0));
    for (i, texture) in textures.iter().enumerate() {
        let speed = if i == 0 { 0.009 } else { 0.005 };
        let offset = (time * speed).rem_euclid(1.0) as f32;
        let opacity = match (overcast, i) {
            (true, 0) => 0.42,
            (true, _) => 0.24,
            (false, 0) => 0.28,
            (false, _) => 0.17,
        };
        let color = paint::alpha(Color32::WHITE, opacity);
        let mut mesh = egui::Mesh::with_texture(texture.id());
        for pair in rows.windows(2) {
            let (ya, la, ra) = pair[0];
            let (yb, lb, rb) = pair[1];
            let base = mesh.vertices.len() as u32;
            for (x, y) in [(la, ya), (ra, ya), (rb, yb), (lb, yb)] {
                let uv = egui::pos2((x - rect.left()) / rect.width() + offset,
                    (y - rect.top()) / rect.height());
                mesh.vertices.push(egui::epaint::Vertex { pos: egui::pos2(x, y), uv, color });
            }
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        c.painter.add(egui::Shape::mesh(mesh));
    }
}

/// Stable, bounded particles avoid keeping per-frame state or allocating textures.
fn paint_weather_background(c: &Card, kind: BackgroundWeather, time: f64, snow_amount: f32, day: bool, textures: Option<&[egui::TextureHandle; 2]>) {
    let width = crate::app::PW;
    let height = crate::app::WEATHER_H;
    let c = c.clipped(0.0, 0.0, width, height);
    let sample = |i: usize, salt: u32| {
        let mut n = (i as u32).wrapping_add(1).wrapping_mul(0x9e37_79b9) ^ salt;
        n ^= n >> 16;
        n = n.wrapping_mul(0x7feb_352d);
        n ^= n >> 15;
        (n & 0xffff) as f32 / 65536.0
    };
    if matches!(kind, BackgroundWeather::Thunder) {
        // A single soft pulse every 13 seconds, with no hard flash or lightning shape.
        let phase = (time.rem_euclid(13.0) as f32 / 0.65).clamp(0.0, 1.0);
        let flash = (std::f32::consts::PI * phase).sin().max(0.0);
        c.fill(0.0, 0.0, width, height, 0.0, paint::alpha(Color32::WHITE, 0.045 * flash));
    }
    match kind {
        BackgroundWeather::Clear | BackgroundWeather::PartlyCloudy | BackgroundWeather::Cloudy(_) => {
            if !matches!(kind, BackgroundWeather::Cloudy(_)) {
                let light = if day { Color32::from_rgb(255, 210, 125) } else { Color32::from_rgb(182, 202, 235) };
                let pulse = 0.88 + 0.12 * (time * 0.45).sin() as f32;
                let strength = if matches!(kind, BackgroundWeather::PartlyCloudy) { 0.75 } else { 1.0 };
                c.glow(width * 0.78, 8.0, 180.0, paint::alpha(light, 0.12 * pulse * strength));
                if day {
                    for i in 0..3 {
                        let shift = (time * 0.17 + i as f64).sin() as f32 * 6.0;
                        let x = width * 0.60 + i as f32 * 49.0 + shift;
                        c.poly(&[(width * 0.78, 0.0), (x, height), (x + 22.0, height)],
                            paint::alpha(light, 0.025 * pulse * strength));
                    }
                }
            }
            if let (BackgroundWeather::Cloudy(overcast), Some(textures)) = (kind, textures) {
                paint_cloud_textures(&c, textures, time, overcast);
            } else if !matches!(kind, BackgroundWeather::Clear) {
                let opacity = if matches!(kind, BackgroundWeather::Cloudy(_)) { 0.075 } else { 0.045 };
                for i in 0..3 {
                    let phase = time * (0.12 + i as f64 * 0.025) + i as f64 * 1.8;
                    let x = 35.0 + i as f32 * 135.0 + phase.sin() as f32 * 20.0;
                    let y = 55.0 + i as f32 * 46.0;
                    let cloud = paint::alpha(Color32::from_rgb(216, 225, 236), opacity);
                    c.glow(x, y, 90.0, cloud);
                    c.glow(x + 38.0, y - 8.0, 80.0, cloud);
                }
            }
        }
        BackgroundWeather::Fog => {
            for i in 0..5 {
                let phase = time * (0.16 + i as f64 * 0.025) + i as f64 * 1.4;
                let x = -65.0 + phase.sin() as f32 * 35.0 + (i % 2) as f32 * 45.0;
                let y = 36.0 + i as f32 * 37.0;
                let alpha = 0.04 + 0.015 * (time * 0.3 + i as f64).sin() as f32;
                c.fill(x, y, width * 0.78, 14.0, 7.0, paint::alpha(Color32::from_rgb(220, 228, 235), alpha));
                c.fill(x + 145.0, y + 15.0, width * 0.62, 9.0, 4.5, paint::alpha(Color32::from_rgb(220, 228, 235), alpha * 0.7));
            }
        }
        BackgroundWeather::Rain(_) | BackgroundWeather::Thunder => {
            let count = match kind { BackgroundWeather::Rain(count) => count, _ => 68 };
            for i in 0..count {
                let a = sample(i, 0x1234);
                let b = sample(i, 0x9876);
                let speed = 135.0 + b * 85.0;
                let ground = height - 2.0;
                let splash_duration = 0.46;
                let travel = ground + 18.0 + speed * splash_duration;
                let y = ((time * speed as f64 + (a * travel) as f64)
                    .rem_euclid(travel as f64) as f32) - 18.0;
                let x0 = sample(i, 0x4567) * (width + 36.0) - 18.0;
                let len = 8.0 + b * 8.0;
                let color = paint::alpha(Color32::from_rgb(205, 224, 242), 0.16 + b * 0.10);
                if y < ground {
                    let x = x0 - y * 0.12;
                    c.seg((x + len * 0.12, y - len), (x, y), 0.8, color);
                } else {
                    let progress = ((y - ground) / (speed * splash_duration)).clamp(0.0, 1.0);
                    let x = x0 - ground * 0.12;
                    let fade = (1.0 - progress).sqrt();
                    let color = paint::alpha(Color32::from_rgb(205, 224, 242), (0.48 + b * 0.16) * fade);
                    let rise = (std::f32::consts::PI * progress).sin();
                    for (side, spread, arc, radius) in [
                        (-1.0_f32, 7.0, 6.5, 1.1),
                        (1.0, 7.0, 6.5, 1.1),
                        (-1.0, 3.7, 9.0, 0.85),
                        (1.0, 3.7, 9.0, 0.85),
                    ] {
                        c.circle(x + side * (0.8 + progress * spread), ground - rise * arc, radius, color);
                    }
                    let ripple = (2.0 + progress * 6.0) * (1.0 - 0.25 * b);
                    c.seg((x - ripple, ground), (x + ripple, ground), 1.1,
                        paint::alpha(Color32::from_rgb(205, 224, 242), 0.42 * fade));
                }
            }
        }
        BackgroundWeather::Snow => {
            let depth = |x: f32| {
                snow_amount * (3.2 + 0.55 * (x * 0.055).sin() + 0.25 * (x * 0.13).sin())
            };
            // Short convex strips form a gently uneven snowbank beneath the page content.
            for i in 0..70 {
                let x = i as f32 * width / 70.0;
                let next = (i + 1) as f32 * width / 70.0;
                c.poly(&[(x, height), (x, height - depth(x)), (next, height - depth(next)), (next, height)],
                    paint::alpha(Color32::WHITE, 0.42));
            }
            for i in 0..48 {
                let a = sample(i, 0x1357);
                let b = sample(i, 0x2468);
                let speed = 14.0 + b * 19.0;
                let sway = ((time * (0.55 + b as f64 * 0.35) + a as f64 * 6.0).sin() as f32) * (3.0 + b * 4.0);
                let x = sample(i, 0x7654) * width + sway;
                let ground = height - depth(x);
                let travel = height + 6.0;
                let y = ((time * speed as f64 + (a * travel) as f64)
                    .rem_euclid(travel as f64) as f32) - 6.0;
                if y < ground {
                    c.circle(x, y, 0.7 + b * 1.25, paint::alpha(Color32::WHITE, 0.20 + b * 0.22));
                }
            }
        }
    }
}

fn paint_week(c: &Card, snapshot: &Snapshot, icon_time: Option<f32>) {
    c.seg(
        (25.0, 122.0),
        (394.0, 122.0),
        0.8,
        Color32::from_white_alpha(24),
    );
    let span = 365.0 / FORECAST_DAYS as f32;
    let x_at = |i: usize| 27.0 + span * (i as f32 + 0.5);
    let mut low = i32::MAX;
    let mut high = i32::MIN;
    for day in &snapshot.forecast {
        if let Some(v) = day.temp_min {
            low = low.min(v);
            high = high.max(v);
        }
        if let Some(v) = day.temp_max {
            low = low.min(v);
            high = high.max(v);
        }
    }
    let curve_y = |value: i32| 205.0 - (value - low) as f32 / (high - low).max(1) as f32 * 12.0;
    for i in 0..FORECAST_DAYS {
        let x = x_at(i);
        let Some(day) = snapshot.forecast.get(i) else {
            c.text(x, 171.0, "—", 12.0, W::Regular, theme::DIM, H::Center);
            continue;
        };
        let label = forecast_label(&day.date, snapshot.timezone.as_deref(), now());
        if label == "今天" {
            c.fill(
                x - 24.0,
                125.0,
                48.0,
                93.0,
                9.0,
                WEATHER_CARD_HIGHLIGHT,
            );
        }
        c.text(x, 136.0, &label, 9.5, W::Semibold, theme::WHITE, H::Center);
        let date = day.date.get(5..10).unwrap_or("--/--").replace('-', "/");
        c.text(x, 147.0, &date, 8.5, W::Regular, theme::DIM, H::Center);
        if let Some(code) = day.code {
            icon::icon_at(c, x, 160.0, 17.0, icon::kind(code, true), icon_time);
        } else {
            c.text(x, 165.0, "—", 10.0, W::Regular, theme::DIM, H::Center);
        }
        if let Some(rain) = day.rain.filter(|rain| *rain >= 20) {
            c.text(
                x,
                177.0,
                &format!("{rain}%"),
                8.5,
                W::Semibold,
                RAIN_C,
                H::Center,
            );
        }
        c.text(
            x,
            188.0,
            &day.temp_max
                .map(|v| format!("{v}°"))
                .unwrap_or_else(|| "—".into()),
            10.5,
            W::Semibold,
            theme::WHITE,
            H::Center,
        );
        c.text(
            x,
            217.0,
            &day.temp_min
                .map(|v| format!("{v}°"))
                .unwrap_or_else(|| "—".into()),
            10.0,
            W::Regular,
            theme::GRAY,
            H::Center,
        );
    }
    if low <= high {
        for i in 0..snapshot.forecast.len().min(FORECAST_DAYS) {
            let day = &snapshot.forecast[i];
            if i > 0 {
                let previous = &snapshot.forecast[i - 1];
                for (a, b, color) in [
                    (previous.temp_max, day.temp_max, SUN_C),
                    (previous.temp_min, day.temp_min, RAIN_C),
                ] {
                    if let (Some(a), Some(b)) = (a, b) {
                        c.seg(
                            (x_at(i - 1), curve_y(a)),
                            (x_at(i), curve_y(b)),
                            1.3,
                            paint::alpha(color, 0.85),
                        );
                    }
                }
            }
            for (value, color) in [(day.temp_max, SUN_C), (day.temp_min, RAIN_C)] {
                if let Some(value) = value {
                    c.circle(x_at(i), curve_y(value), 2.0, color);
                }
            }
        }
    }
}

fn paint_solar(c: &Card, snapshot: &Snapshot, timestamp: i64, t:f32, slide:f32) {
    c.fill(36.0, 38.0, 74.0, 20.0, 10.0, Color32::from_white_alpha(16));
    c.text(
        45.0,
        52.0,
        "‹  返回天气",
        10.0,
        W::Semibold,
        theme::WHITE,
        H::Left,
    );
    c.text(
        382.0,
        52.0,
        &c.fit(&snapshot.place.label(), 10.0, W::Regular, 190.0),
        10.0,
        W::Regular,
        theme::GRAY,
        H::Right,
    );
    let Some(sun) = snapshot.sun.as_ref() else {
        c.text(
            209.5,
            104.0,
            "暂无日出日落数据",
            15.0,
            W::Semibold,
            theme::WHITE,
            H::Center,
        );
        c.text(
            209.5,
            126.0,
            "等待天气更新后查看",
            10.5,
            W::Regular,
            theme::GRAY,
            H::Center,
        );
        return;
    };
    let valid_today = (sun.day_start..sun.day_end).contains(&timestamp);
    let (title, main, note) = if !valid_today {
        (
            "日照数据待更新",
            sun.clock(sun.sunrise),
            "显示缓存中的日出时间".to_owned(),
        )
    } else if timestamp < sun.sunrise {
        let minutes = (sun.sunrise - timestamp + 59) / 60;
        (
            "距离日出",
            format!("{:02}:{:02}", minutes / 60, minutes % 60),
            "小时 : 分钟".to_owned(),
        )
    } else if timestamp < sun.sunset {
        let minutes = (sun.sunset - timestamp + 59) / 60;
        (
            "距离日落",
            format!("{:02}:{:02}", minutes / 60, minutes % 60),
            "小时 : 分钟".to_owned(),
        )
    } else {
        (
            "今日已日落",
            sun.clock(sun.sunset),
            "白昼已经结束".to_owned(),
        )
    };
    c.text(40.0, 80.0, title, 10.5, W::Regular, theme::GRAY, H::Left);
    c.text(39.0, 110.0, &main, 27.0, W::Bold, theme::WHITE, H::Left);
    c.text(40.0, 127.0, &note, 9.5, W::Regular, theme::DIM, H::Left);
    if let Some(progress) = sun.progress(timestamp).filter(|_| valid_today) {
        c.text(
            40.0,
            150.0,
            &format!("日照已过 {:.0}%", progress * 100.0),
            10.0,
            W::Semibold,
            SUN_C,
            H::Left,
        );
    }
    let point = |p: f32| {
        (
            190.0 + p * 190.0,
            126.0 - (std::f32::consts::PI * p).sin() * 57.0,
        )
    };
    let track: Vec<_> = (0..=60).map(|i| point(i as f32 / 60.0)).collect();
    let eased=1.0-(1.0-slide.clamp(0.0,1.0)).powi(3);
    let shown=sun.progress(timestamp).filter(|_|valid_today).map(|p|p*eased);
    let mut sky=egui::Mesh::default();
    for (i,&(x,y)) in track.iter().enumerate() {
        let lit=shown.is_some_and(|p|i as f32/60.0<=p);
        sky.colored_vertex(c.pos(x,y),if lit{paint::alpha(SUN_C,0.26)}else{Color32::from_white_alpha(4)});
        sky.colored_vertex(c.pos(x,126.0),Color32::TRANSPARENT);
        if i>0{let n=i as u32*2;sky.add_triangle(n-2,n-1,n);sky.add_triangle(n-1,n+1,n);}
    }
    c.painter.add(egui::Shape::mesh(sky));
    let screen:Vec<_>=track.iter().map(|&(x,y)|c.pos(x,y)).collect();
    c.painter.extend(egui::Shape::dashed_line(&screen,egui::Stroke::new(1.2*c.s,Color32::from_white_alpha(70)),3.0*c.s,3.0*c.s));
    c.seg(
        (180.0, 126.0),
        (388.0, 126.0),
        0.8,
        Color32::from_white_alpha(36),
    );
    if let Some(shown)=shown {
        let lit:Vec<_>=(0..=60).map(|i|point(shown*i as f32/60.0)).collect();
        c.path(&lit,2.0,SUN_C);
        let (x,y)=point(shown);
        if slide<0.4 {let k=slide/0.4;let (rx,ry)=point(0.0);c.ring(rx,ry,3.0+9.0*(1.0-(1.0-k).powi(3)),1.2*(1.0-k)+0.2,paint::alpha(SUN_C,0.8*(1.0-k)));}
        let moving=1.0-eased;
        for k in 1..7 {let back=shown-k as f32*0.028;if back<=0.0{break;}let (px,py)=point(back);let fade=1.0-k as f32/7.0;icon::toned(c,||c.circle(px,py,2.2*fade+0.4,paint::alpha(Color32::from_rgb(255,224,120),0.75*fade*moving)));}
        let pulse=1.0+0.08*(t*2.2).sin();let settle=((slide-0.82)/0.18).clamp(0.0,1.0);let pop=1.0+0.2*(std::f32::consts::PI*settle).sin();
        icon::toned(c,||{c.glow(x,y,26.0*pulse,paint::alpha(SUN_C,0.22));icon::sun_at(c,x,y,8.0*pop,t*0.5,true);});
    } else if valid_today {
        let night=c.faded(eased);
        icon::toned(c,||{
            for (i,&(x,y,r)) in [(214.0,82.0,2.4),(246.0,64.0,1.6),(322.0,60.0,2.0),(352.0,86.0,1.5),(300.0,104.0,1.3),(230.0,112.0,1.2)].iter().enumerate() {
                icon::sparkle(&night,x,y,r,paint::alpha(Color32::from_rgb(246,236,196),0.35+0.35*(t*1.8+i as f32*1.3).sin().abs()));
            }
            night.glow(285.0,92.0+10.0*(1.0-eased),22.0,paint::alpha(Color32::from_rgb(150,160,255),0.2));
            icon::moon_at(&night,285.0,92.0+10.0*(1.0-eased),10.0,t,false);
        });
    }
    for (x, label, value) in [
        (190.0, "日出", sun.clock(sun.sunrise)),
        (
            285.0,
            "白昼中点",
            sun.clock(sun.sunrise + (sun.sunset - sun.sunrise) / 2),
        ),
        (380.0, "日落", sun.clock(sun.sunset)),
    ] {
        c.text(x, 145.0, label, 9.5, W::Regular, theme::GRAY, H::Center);
        c.text(x, 159.0, &value, 10.5, W::Semibold, theme::WHITE, H::Center);
    }
    let day_minutes = (sun.sunset - sun.sunrise) / 60;
    let night_minutes = ((sun.day_end - sun.day_start) / 60 - day_minutes).max(0);
    c.text(
        40.0,
        183.0,
        &format!("白昼 {}小时{:02}分", day_minutes / 60, day_minutes % 60),
        10.0,
        W::Regular,
        theme::GRAY,
        H::Left,
    );
    c.text(
        382.0,
        183.0,
        &format!("夜晚 {}小时{:02}分", night_minutes / 60, night_minutes % 60),
        10.0,
        W::Regular,
        theme::GRAY,
        H::Right,
    );
    let start = (sun.sunrise - sun.day_start) as f32 / (sun.day_end - sun.day_start) as f32;
    let end = (sun.sunset - sun.day_start) as f32 / (sun.day_end - sun.day_start) as f32;
    c.fill(40.0, 190.0, 342.0, 7.0, 3.5, Color32::from_rgb(38, 50, 92));
    c.fill(
        40.0 + 342.0 * start,
        190.0,
        342.0 * (end - start),
        7.0,
        3.5,
        SUN_C,
    );
    if valid_today {
        let pos = (timestamp - sun.day_start) as f32 / (sun.day_end - sun.day_start) as f32 * eased;
        c.circle(40.0 + 342.0 * pos, 193.5, 3.5, theme::WHITE);
        c.text(
            (40.0 + 342.0 * pos).clamp(88.0, 334.0),
            211.0,
            &format!("现在 {}", sun.clock(timestamp)),
            8.5,
            W::Semibold,
            theme::WHITE,
            H::Center,
        );
    }
    c.text(40.0, 211.0, "0:00", 8.5, W::Regular, theme::DIM, H::Left);
    c.text(382.0, 211.0, "24:00", 8.5, W::Regular, theme::DIM, H::Right);
}

impl State {
    fn paint_picker(&mut self, c: &Card) -> bool {
        let mut manage = false;
        let rows: Vec<Option<Place>> = std::iter::once(None)
            .chain(self.saved.iter().cloned().map(Some))
            .collect();
        let x = 39.0;
        let y = 38.0;
        let row_h = 22.0;
        let height = 11.0 + row_h * (rows.len() as f32 + 1.0);
        let backdrop = c.hit(35.0, 34.0, 350.0, 180.0, "weather-picker-backdrop");
        if backdrop.clicked() {
            self.picker_open = false;
        }
        c.fill(x, y, 240.0, height, 10.0, Color32::from_rgb(35, 39, 47));
        c.stroke(
            x,
            y,
            240.0,
            height,
            10.0,
            0.8,
            Color32::from_white_alpha(35),
        );
        let mut choice = None;
        for (i, place) in rows.iter().enumerate() {
            let ry = y + 5.0 + i as f32 * row_h;
            let hit = c
                .hit(x + 4.0, ry, 232.0, row_h, ("weather-city", i))
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if hit.hovered() {
                c.fill(
                    x + 4.0,
                    ry,
                    232.0,
                    row_h,
                    6.0,
                    Color32::from_white_alpha(20),
                );
            }
            let active = match place {
                None => !self.manual,
                Some(place) => {
                    self.manual && self.place.as_ref().is_some_and(|p| p.same_location(place))
                }
            };
            c.circle(
                x + 13.0,
                ry + 11.0,
                3.0,
                if active { theme::CYAN } else { theme::DIM },
            );
            let label = place
                .as_ref()
                .map(Place::label)
                .unwrap_or_else(|| "自动定位".into());
            c.text(
                x + 23.0,
                ry + 15.0,
                &c.fit(&label, 10.0, W::Regular, 145.0),
                10.0,
                W::Regular,
                theme::WHITE,
                H::Left,
            );
            if let Some(entry) = self
                .cache
                .iter()
                .find(|entry| entry.matches(place.as_ref()))
            {
                c.text(
                    x + 226.0,
                    ry + 15.0,
                    &format!("{}°", entry.snapshot.reading.temp),
                    10.0,
                    W::Semibold,
                    theme::GRAY,
                    H::Right,
                );
            }
            if hit.clicked() {
                choice = Some(place.clone());
            }
        }
        let my = y + 5.0 + rows.len() as f32 * row_h;
        c.seg(
            (x + 10.0, my),
            (x + 230.0, my),
            0.8,
            Color32::from_white_alpha(30),
        );
        c.text(
            x + 23.0,
            my + 15.0,
            "管理城市…",
            10.0,
            W::Semibold,
            theme::CYAN,
            H::Left,
        );
        if c.hit(x + 4.0, my, 232.0, row_h, "weather-manage-cities")
            .clicked()
        {
            manage = true;
        }
        if c.live && c.ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.picker_open = false;
        }
        if let Some(place) = choice {
            self.select(place);
        }
        if manage {
            self.picker_open = false;
        }
        manage
    }

    fn paint_forecast_view(&mut self,c:&Card,snapshot:&Snapshot,env:&crate::app::Env) {
                let animated = c.live
                    && crate::bar_layout::animations_enabled()
                    && c.ui.ctx().style_of(c.ui.ctx().theme()).animation_time > 0.0;
                let hovered = c.ui.input(|i| {
                    i.pointer.hover_pos().is_some_and(|p| c.rect(35.0, 34.0, 350.0, 180.0).contains(p))
                });
                if c.live && self.ambient.advance(env.t, animated, hovered) {
                    c.ui.ctx().request_repaint_after(Duration::from_millis(33));
                }
                let icon_time = animated.then_some((env.t % 6000.0) as f32);
                if animated {
                    c.ui.ctx().request_repaint_after(Duration::from_millis(33));
                }
                c.glow(65.0, 72.0, 43.0, paint::alpha(SUN_C, 0.08));
                icon::icon_at(c, 65.0, 72.0, 44.0, icon::kind(snapshot.reading.code, snapshot.reading.day), icon_time);
                c.text(
                    97.0,
                    82.0,
                    &format!("{}°", snapshot.reading.temp),
                    33.0,
                    W::Bold,
                    if self.stale() {
                        theme::GRAY
                    } else {
                        theme::WHITE
                    },
                    H::Left,
                );
                c.text(
                    98.0,
                    100.0,
                    self.source.condition.as_deref().filter(|_|self.source.is_active()).unwrap_or_else(||describe(snapshot.reading.code)),
                    11.5,
                    W::Semibold,
                    theme::WHITE,
                    H::Left,
                );
                tile(
                    c,
                    212.0,
                    38.0,
                    "体感",
                    &snapshot
                        .feels
                        .map(|v| format!("{v}°"))
                        .unwrap_or_else(|| "—".into()),
                    thermo_glyph,
                );
                tile(
                    c,
                    300.0,
                    38.0,
                    "湿度",
                    &snapshot
                        .humidity
                        .map(|v| format!("{v}%"))
                        .unwrap_or_else(|| "—".into()),
                    |c,x,y| paint::droplet(c,x,y,11.0,RAIN_C),
                );
                let index_hit=c.hit(212.0,77.0,84.0,34.0,"weather-indices-open")
                    .on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("查看生活指数");
                let ctx=c.ui.ctx();
                let index_lift=if animated {ctx.animate_bool_with_time(egui::Id::unique(("weather-index-lift",c.live)),index_hit.hovered(),0.25)}else{0.0};
                let index_press=if animated {ctx.animate_bool_with_time(egui::Id::unique(("weather-index-press",c.live)),index_hit.is_pointer_button_down_on(),0.08)}else{0.0};
                let index_entry=c.scaled(254.0,94.0,1.0-0.05*index_press);
                index_entry.fill(212.0,77.0,84.0,34.0,9.0,WEATHER_CARD_HIGHLIGHT);
                if index_lift>0.0 {index_entry.fill(212.0,77.0,84.0,34.0,9.0,Color32::from_white_alpha((12.0*index_lift) as u8));}
                index_entry.circle(226.0,94.0,7.0,paint::alpha(theme::CYAN,0.18));
                life_index_glyph(&index_entry.scaled(226.0,94.0,0.48),226.0,94.0,"3",theme::CYAN);
                let summary=self.indices.items().iter().find(|i|i.kind=="3").map(|i|i.category.clone()).unwrap_or_else(||"查看详情".into());
                c.text(239.0,91.0,"生活指数",9.0,W::Regular,theme::GRAY,H::Left);
                c.text(239.0,105.5,&c.fit(&summary,10.5,W::Semibold,44.0),10.5,W::Semibold,theme::WHITE,H::Left);
                let nudge=(self.ambient.time as f32*3.0).sin()*1.2;
                index_entry.path(&[(287.0+nudge,90.0),(290.0+nudge,94.0),(287.0+nudge,98.0)],1.3,paint::alpha(theme::CYAN,0.9));
                if index_hit.clicked() && !self.picker_open {self.indices.open=true;self.indices.selected=None;self.picker_open=false;}
                if animated && ((index_lift>0.0&&index_lift<1.0)||(index_press>0.0&&index_press<1.0)){ctx.request_repaint();}
                let hit=c.hit(300.0,77.0,84.0,34.0,"weather-solar-open")
                    .on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("查看日照详情");
                let lift=if animated {ctx.animate_bool_with_time(egui::Id::unique(("weather-entry-lift",c.live)),hit.hovered(),0.25)}else{0.0};
                let press=if animated {ctx.animate_bool_with_time(egui::Id::unique(("weather-entry-press",c.live)),hit.is_pointer_button_down_on(),0.08)}else{0.0};
                let entry=c.scaled(342.0,94.0,1.0-0.05*press);
                entry.fill(300.0,77.0,84.0,34.0,9.0,paint::alpha(SUN_C,0.13+0.07*lift));
                sunrise_glyph(&entry,314.0,93.0,lift);
                let rise = snapshot
                    .sun
                    .as_ref()
                    .map(|sun| sun.clock(sun.sunrise))
                    .unwrap_or_else(|| "--:--".into());
                let set = snapshot
                    .sun
                    .as_ref()
                    .map(|sun| sun.clock(sun.sunset))
                    .unwrap_or_else(|| "--:--".into());
                c.text(
                    327.0,
                    91.0,
                    &format!("日出 {rise}"),
                    9.0,
                    W::Regular,
                    theme::GRAY,
                    H::Left,
                );
                c.text(
                    327.0,
                    105.0,
                    &format!("日落 {set}"),
                    9.0,
                    W::Semibold,
                    theme::WHITE,
                    H::Left,
                );
                entry.path(&[(375.0+nudge,90.0),(378.0+nudge,94.0),(375.0+nudge,98.0)],1.3,paint::alpha(SUN_C,0.9));
                if hit.clicked() && !self.picker_open {self.solar_open=true;self.solar_at=env.t;self.picker_open=false;}
                if animated && ((lift>0.0&&lift<1.0)||(press>0.0&&press<1.0)){ctx.request_repaint();}
                if SIGNALS_ENABLED {
                let alerts=self.signals.alerts();
                let alert_label=if self.signals.loading(signals::Kind::Alert) {"预警 · 检查中".to_owned()}
                    else if self.signals.stale(signals::Kind::Alert) {"预警 · 无法确认".to_owned()}
                    else if alerts.is_empty() {"预警 · 暂无".to_owned()}
                    else {format!("预警 {}条 {} {}",alerts.len(),warning_severity(&alerts[0].severity),alerts[0].headline)};
                for (x,w,label,kind) in [(27.0,177.0,alert_label,signals::Kind::Alert),(213.0,165.0,"分钟降水  ›".to_owned(),signals::Kind::Rain)] {
                    let hit=c.hit(x,124.0,w,22.0,("weather-signal",kind.index())).on_hover_cursor(egui::CursorIcon::PointingHand);
                    c.fill(x,124.0,w,22.0,7.0,if hit.hovered(){Color32::from_white_alpha(65)}else{WEATHER_CARD_HIGHLIGHT});
                    let ink=if kind==signals::Kind::Alert && !alerts.is_empty() && !self.signals.stale(kind){WARN_C}else{theme::WHITE};
                    if kind==signals::Kind::Rain {paint::droplet(c,x+11.0,135.0,10.0,RAIN_C);}
                    let left=if kind==signals::Kind::Rain{22.0}else{9.0};
                    c.text(x+left,140.0,&c.fit(&label,9.5,W::Semibold,w-left-9.0),9.5,W::Semibold,ink,H::Left);
                    if hit.clicked() && !self.picker_open {self.signals.view=Some(kind);self.signals.selected=None;self.signals.text_page=0;self.signals.list_page=0;if kind==signals::Kind::Rain && self.signals.stale(kind) {self.refresh_signal(snapshot,ctx,kind,true);}}
                }
                }
                paint_week(&c.shifted(0.0,if SIGNALS_ENABLED {28.0}else{0.0}), snapshot, icon_time);
    }

    fn paint_signal_empty(&mut self,c:&Card,kind:signals::Kind,snapshot:&Snapshot,message:&str) {
        let loading=self.signals.loading(kind);
        let ready=self.signals.can_refresh(kind);
        let message=if loading {"正在获取…"}else{message};
        c.text(210.0,120.0,&c.fit(message,10.0,W::Regular,320.0),10.0,W::Regular,theme::GRAY,H::Center);
        let hit=c.hit(165.0,136.0,90.0,29.0,("signal-empty-refresh",kind.index()));
        let hit=if ready {hit.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("手动刷新，计入和风请求额度")}
            else {hit.on_hover_text(if loading {"正在更新"}else if self.network==Some(false){"网络未连接"}else{"请稍后再试"})};
        c.fill(165.0,136.0,90.0,29.0,8.0,if ready && hit.hovered(){Color32::from_white_alpha(65)}else{WEATHER_CARD_HIGHLIGHT});
        let label=if loading {"更新中"}else if ready {"手动刷新"}else{"暂不可刷新"};
        c.centered_text(210.0,150.5,label,10.5,W::Semibold,if ready{theme::WHITE}else{theme::DIM},H::Center);
        if ready && hit.clicked(){self.refresh_signal(snapshot,c.ui.ctx(),kind,true);}
    }

    fn paint_signal_view(&mut self,c:&Card,kind:signals::Kind,snapshot:&Snapshot) {
        let title=if self.signals.credits_open {"数据归因"}else if kind==signals::Kind::Alert {"天气预警"}else{"分钟降水"};
        c.text(40.0,47.0,"‹",18.0,W::Regular,theme::WHITE,H::Left);
        c.text(58.0,47.0,title,13.0,W::Bold,theme::WHITE,H::Left);
        if c.hit(36.0,32.0,105.0,22.0,"signal-back").on_hover_cursor(egui::CursorIcon::PointingHand).clicked(){self.signals.back();return;}
        if self.signals.stale(kind) {
            let label=self.signals.fetched(kind).map(|at|format!("缓存 {}",format_local_time(at.max(0) as u64))).unwrap_or_else(||"数据待更新".into());
            c.text(378.0,47.0,&c.fit(&label,9.0,W::Regular,150.0),9.0,W::Regular,WARN_C,H::Right);
        }
        if self.signals.credits_open {
            let mut lines=Vec::new();let mut line=String::new();
            for ch in self.signals.credits(kind).join("\n").chars(){if ch=='\n'{lines.push(std::mem::take(&mut line));continue;}let mut next=line.clone();next.push(ch);if c.text_w(&next,10.0,W::Regular)>330.0 && !line.is_empty(){lines.push(std::mem::take(&mut line));}line.push(ch);}
            if !line.is_empty(){lines.push(line);}let pages=lines.len().max(1).div_ceil(9);self.signals.text_page=self.signals.text_page.min(pages-1);
            for (n,line) in lines.iter().skip(self.signals.text_page*9).take(9).enumerate(){c.text(43.0,70.0+n as f32*17.0,line,10.0,W::Regular,theme::WHITE,H::Left);}
            if pages>1 {c.text(210.0,235.0,&format!("{} / {}",self.signals.text_page+1,pages),9.0,W::Regular,theme::GRAY,H::Center);
                if self.signals.text_page>0 {c.text(90.0,235.0,"‹ 上一页",10.0,W::Regular,theme::CYAN,H::Center);if c.hit(60.0,218.0,65.0,22.0,"credit-prev").clicked(){self.signals.text_page-=1;}}
                if self.signals.text_page+1<pages {c.text(320.0,235.0,"下一页 ›",10.0,W::Regular,theme::CYAN,H::Center);if c.hit(285.0,218.0,70.0,22.0,"credit-next").clicked(){self.signals.text_page+=1;}}
            }
            return;
        }
        let data=self.signals.data(kind).cloned();
        if kind==signals::Kind::Alert {
            let alerts=self.signals.alerts();
            if let Some(alert)=self.signals.selected.and_then(|i|alerts.get(i)) {
                c.text(43.0,67.0,&c.fit(&alert.headline,11.0,W::Bold,330.0),11.0,W::Bold,WARN_C,H::Left);
                let meta=format!("{}  {}",alert.sender,alert.issued);
                c.text(43.0,83.0,&c.fit(&meta,9.0,W::Regular,330.0),9.0,W::Regular,theme::GRAY,H::Left);
                let text=format!("{}\n{}\n{}",if alert.expire_text.is_empty(){"有效期未提供".to_owned()}else{format!("有效期至 {}",alert.expire_text)},alert.description,alert.instruction);
                let mut lines=Vec::new();let mut line=String::new();
                for ch in text.chars(){if ch=='\n'{lines.push(std::mem::take(&mut line));continue;}let mut next=line.clone();next.push(ch);if c.text_w(&next,10.0,W::Regular)>330.0 && !line.is_empty(){lines.push(std::mem::take(&mut line));}line.push(ch);}
                if !line.is_empty(){lines.push(line);}
                let pages=lines.len().max(1).div_ceil(7);self.signals.text_page=self.signals.text_page.min(pages-1);
                for (n,line) in lines.iter().skip(self.signals.text_page*7).take(7).enumerate(){c.text(43.0,103.0+n as f32*17.0,line,10.0,W::Regular,theme::WHITE,H::Left);}
                if pages>1 {c.text(210.0,235.0,&format!("{} / {}",self.signals.text_page+1,pages),9.0,W::Regular,theme::GRAY,H::Center);
                    if self.signals.text_page>0 {c.text(90.0,235.0,"‹ 上一页",10.0,W::Regular,theme::CYAN,H::Center);if c.hit(60.0,218.0,65.0,22.0,"alert-prev").clicked(){self.signals.text_page-=1;}}
                    if self.signals.text_page+1<pages {c.text(320.0,235.0,"下一页 ›",10.0,W::Regular,theme::CYAN,H::Center);if c.hit(285.0,218.0,70.0,22.0,"alert-next").clicked(){self.signals.text_page+=1;}}
                }
            } else if !alerts.is_empty() {
                if self.signals.stale(kind){c.text(44.0,65.0,"上次取得的预警，当前状态无法确认",9.0,W::Regular,WARN_C,H::Left);}
                let pages=alerts.len().div_ceil(3);self.signals.list_page=self.signals.list_page.min(pages-1);
                for (n,a) in alerts.iter().enumerate().skip(self.signals.list_page*3).take(3){let y=70.0+(n%3) as f32*46.0;
                    let hit=c.hit(41.0,y,338.0,41.0,("alert-item",n)).on_hover_cursor(egui::CursorIcon::PointingHand);
                    c.fill(41.0,y,338.0,41.0,8.0,if hit.hovered(){Color32::from_white_alpha(65)}else{WEATHER_CARD_HIGHLIGHT});
                    c.text(53.0,y+16.0,&c.fit(&a.headline,10.0,W::Semibold,310.0),10.0,W::Semibold,WARN_C,H::Left);
                    let severity=warning_severity(&a.severity);
                    c.text(53.0,y+32.0,&c.fit(&format!("{}  {}  {}",a.sender,a.issued,severity),9.0,W::Regular,310.0),9.0,W::Regular,theme::GRAY,H::Left);
                    if hit.clicked(){self.signals.selected=Some(n);self.signals.text_page=0;}
                }
                if pages>1 {c.text(210.0,225.0,&format!("{} / {}",self.signals.list_page+1,pages),9.0,W::Regular,theme::GRAY,H::Center);
                    if self.signals.list_page>0 && c.hit(60.0,208.0,80.0,22.0,"alert-list-prev").clicked(){self.signals.list_page-=1;}
                    if self.signals.list_page+1<pages && c.hit(280.0,208.0,80.0,22.0,"alert-list-next").clicked(){self.signals.list_page+=1;}
                    if self.signals.list_page>0 {c.text(100.0,225.0,"‹ 上一页",9.0,W::Regular,theme::CYAN,H::Center);}
                    if self.signals.list_page+1<pages {c.text(320.0,225.0,"下一页 ›",9.0,W::Regular,theme::CYAN,H::Center);}
                }
            } else {
                let label=if matches!(data,Some(signals::Data::Alerts(_))) && !self.signals.stale(kind){"当前暂无天气预警"}else{self.signals.status(kind)}.to_owned();
                self.paint_signal_empty(c,kind,snapshot,&label);
            }
        } else if let Some(signals::Data::Rain{summary,updated,points})=data {
            if self.signals.stale(kind) {self.paint_signal_empty(c,kind,snapshot,"预报已过期，请刷新");}
            else {
                let time_label=|t:i64|chrono::DateTime::from_timestamp(t,0).map(|d|snapshot.timezone.as_deref().and_then(|z|z.parse::<chrono_tz::Tz>().ok()).map(|z|d.with_timezone(&z).format("%m-%d %H:%M").to_string()).unwrap_or_else(||d.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())).unwrap_or_default();
                let updated=chrono::DateTime::parse_from_rfc3339(&updated).ok().map(|t|time_label(t.timestamp())).unwrap_or_else(||"--".into());
                c.text(42.0,71.0,&c.fit(&format!("更新时：{summary}"),12.0,W::Semibold,335.0),12.0,W::Semibold,theme::WHITE,H::Left);
                c.text(42.0,89.0,&c.fit(&format!("更新于 {updated} · mm / 5分钟"),9.0,W::Regular,330.0),9.0,W::Regular,theme::GRAY,H::Left);
                let points:Vec<_>=points.iter().filter(|p|p.time+300>now() as i64).take(24).collect();
                if points.is_empty(){self.paint_signal_empty(c,kind,snapshot,"预报时段已结束，请刷新");}else{
                    let max=points.iter().map(|p|p.amount).fold(0.0_f32,f32::max);let scale=max.max(0.1);
                    c.text(375.0,108.0,&format!("最高 {max:.1} mm"),9.0,W::Regular,theme::GRAY,H::Right);
                    let first=points[0].time;let span=(points.last().unwrap().time-first+300).max(300) as f32;
                    for p in &points {let x=44.0+(p.time-first) as f32/span*330.0;let h=(p.amount/scale*82.0).max(if p.amount>0.0{2.0}else{0.0});if h>0.0 {c.fill(x,201.0-h,(300.0/span*300.0).clamp(2.0,16.0),h,2.0,if p.kind=="snow"{Color32::from_rgb(180,210,255)}else{RAIN_C});}}
                    c.seg((43.0,202.0),(375.0,202.0),0.8,theme::GRAY);
                    c.text(43.0,220.0,&time_label(points[0].time),9.0,W::Regular,theme::GRAY,H::Left);c.text(375.0,220.0,&time_label(points.last().unwrap().time+300),9.0,W::Regular,theme::GRAY,H::Right);
                    if max==0.0 {c.text(210.0,155.0,"已返回时段无降水",11.0,W::Regular,theme::GRAY,H::Center);}
                }
            }
        } else {
            let label=if matches!(data,Some(signals::Data::Unavailable)) && !self.signals.stale(kind){"暂无数据，所在地区可能未覆盖"}else{self.signals.status(kind)}.to_owned();
            self.paint_signal_empty(c,kind,snapshot,&label);
        }
        let credits=self.signals.credits(kind);
        if !credits.is_empty(){c.text(25.0,257.0,"数据归因 ⓘ",9.0,W::Regular,theme::GRAY,H::Left);if c.hit(21.0,245.0,85.0,13.0,"signal-attributions").on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(credits.join("\n")).clicked(){self.signals.credits_open=true;self.signals.text_page=0;}}
    }

    fn paint_indices_view(&mut self, c:&Card) {
        c.fill(36.0,38.0,74.0,20.0,10.0,Color32::from_white_alpha(16));
        c.text(45.0,52.0,if self.indices.selected.is_some(){"‹  返回指数"}else{"‹  返回天气"},10.0,W::Semibold,theme::WHITE,H::Left);
        let title=if self.indices.stale(){"生活指数 · 缓存"}else{"生活指数"};
        c.text(382.0,52.0,title,10.0,W::Regular,theme::GRAY,H::Right);
        if c.hit(36.0,38.0,74.0,20.0,"weather-indices-back").on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            if self.indices.selected.take().is_some() {self.indices.detail_page=0;} else {self.indices.leave();}
        }
        let items=self.indices.items();
        if let Some(index)=self.indices.selected.and_then(|n|items.get(n)) {
            c.fill(41.0,58.0,338.0,145.0,10.0,WEATHER_CARD_HIGHLIGHT);
            c.text(54.0,78.0,&index.name,13.0,W::Semibold,theme::WHITE,H::Left);
            c.text(364.0,78.0,&index.category,11.0,W::Semibold,theme::CYAN,H::Right);
            let text=if index.text.is_empty(){"暂无详细建议"}else{&index.text};
            let mut lines=Vec::new();let mut line=String::new();
            for ch in text.chars() {
                if ch=='\n' {lines.push(std::mem::take(&mut line));continue;}
                let mut next=line.clone();next.push(ch);
                if c.text_w(&next,11.0,W::Regular)>305.0 && !line.is_empty() {lines.push(std::mem::take(&mut line));}
                line.push(ch);
            }
            if !line.is_empty(){lines.push(line);}
            let pages=lines.len().max(1).div_ceil(4);
            self.indices.detail_page=self.indices.detail_page.min(pages-1);
            for (n,line) in lines.iter().skip(self.indices.detail_page*4).take(4).enumerate() {
                c.text(54.0,101.0+n as f32*18.0,line,11.0,W::Regular,theme::WHITE,H::Left);
            }
            if pages>1 {
                c.text(210.0,190.0,&format!("{} / {}",self.indices.detail_page+1,pages),9.0,W::Regular,theme::GRAY,H::Center);
                if self.indices.detail_page>0 && c.hit(90.0,173.0,50.0,22.0,"index-detail-prev").clicked(){self.indices.detail_page-=1;}
                if self.indices.detail_page+1<pages && c.hit(280.0,173.0,50.0,22.0,"index-detail-next").clicked(){self.indices.detail_page+=1;}
                if self.indices.detail_page>0 {c.text(115.0,190.0,"‹ 上一页",9.0,W::Regular,theme::CYAN,H::Center);}
                if self.indices.detail_page+1<pages {c.text(305.0,190.0,"下一页 ›",9.0,W::Regular,theme::CYAN,H::Center);}
            }
        } else {
            for (slot,(kind,label)) in [("3","穿衣"),("9","感冒"),("16","防晒"),("7","花粉过敏")].iter().enumerate() {
                let x=41.0+(slot%2) as f32*173.0;let y=62.0+(slot/2) as f32*70.0;
                let found=items.iter().position(|i|i.kind==*kind || (*kind=="16" && i.kind=="5"));
                let hit=found.map(|_|c.hit(x,y,165.0,64.0,("weather-index",slot)).on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("查看详细建议"));
                let background=if hit.as_ref().is_some_and(|h|h.hovered()){Color32::from_rgb(27,29,33)}else{Color32::from_rgb(21,23,26)};
                c.fill(x,y,165.0,64.0,10.0,background);
                c.stroke(x,y,165.0,64.0,10.0,1.0,Color32::from_rgb(58,61,68));
                let ink=found.map(|i|if life_index_caution(&items[i].kind,&items[i].category){WARN_C}else{theme::CYAN}).unwrap_or(theme::DIM);
                life_index_glyph(c,x+30.0,y+32.0,kind,ink);
                c.seg((x+54.0,y+17.0),(x+54.0,y+47.0),1.0,Color32::from_rgb(68,74,80));
                let title=if found.is_some_and(|i|items[i].kind=="5") {"紫外线"}else{label};
                c.text(x+65.0,y+27.0,title,10.5,W::Regular,theme::GRAY,H::Left);
                let category=found.map(|i|items[i].category.as_str()).unwrap_or("暂无数据");
                c.text(x+65.0,y+46.0,&c.fit(category,12.0,W::Semibold,92.0),12.0,W::Semibold,if found.is_some(){theme::WHITE}else{theme::DIM},H::Left);
                if hit.is_some_and(|h|h.clicked()){self.indices.selected=found;self.indices.detail_page=0;}
            }
            if items.is_empty() {c.text(210.0,204.0,&c.fit(self.indices.status(),9.0,W::Regular,310.0),9.0,W::Regular,theme::GRAY,H::Center);}
        }
        c.text(25.0,crate::app::WEATHER_H-3.0,"生活指数 · QWeather ⓘ",8.0,W::Regular,theme::GRAY,H::Left);
        let credit=if self.indices.credits().is_empty(){"生活指数由和风天气提供".to_owned()}else{format!("生活指数由和风天气提供\n{}",self.indices.credits().join("\n"))};
        c.hit(21.0,crate::app::WEATHER_H-15.0,150.0,14.0,"indices-attribution").on_hover_text(credit);
    }

    fn paint_hourly_view(&mut self,c:&Card,snapshot:&Snapshot,env:&crate::app::Env,animated:bool) {
        c.text(41.0,49.0,"‹  返回天气",10.0,W::Semibold,theme::WHITE,H::Left);
        c.text(135.0,49.0,"未来 12 小时",9.0,W::Regular,theme::GRAY,H::Left);
        c.text(379.0,49.0,&c.fit(&snapshot.place.label(),10.0,W::Regular,175.0),10.0,W::Regular,theme::GRAY,H::Right);
        c.text(42.0,76.0,&format!("{}°",snapshot.reading.temp),19.0,W::Bold,theme::WHITE,H::Left);
        let description=self.source.condition.as_deref().filter(|_|self.source.is_active()).unwrap_or_else(||describe(snapshot.reading.code));
        let summary=if let Some(feels)=snapshot.feels{format!("{description} · 体感 {feels}°")}else{description.to_owned()};
        c.text(94.0,74.0,&c.fit(&summary,10.0,W::Regular,150.0),10.0,W::Regular,theme::GRAY,H::Left);
        if self.hourly.stale() {
            let status=self.hourly.fetched().map(|t|format!("{}缓存 {}",if self.network==Some(false){"离线 · "}else if self.hourly.error().is_some(){"请求失败 · "}else{""},format_local_time(t.max(0) as u64))).unwrap_or_else(||self.hourly.status().to_owned());
            c.text(379.0,75.0,&c.fit(&status,9.0,W::Regular,130.0),9.0,W::Regular,WARN_C,H::Right);
            if self.hourly.error().is_some() || self.network==Some(false){c.hit(250.0,60.0,130.0,19.0,"hourly-stale-status").on_hover_text(self.hourly.status());}
        }
        c.seg((41.0,85.0),(379.0,85.0),0.7,Color32::from_white_alpha(24));
        let hours=self.hourly.hours();
        if hours.is_empty() {
            c.text(210.0,145.0,&c.fit(self.hourly.status(),11.0,W::Regular,310.0),11.0,W::Regular,theme::GRAY,H::Center);
        } else {
            let desired=self.hourly.offset.min(hours.len().saturating_sub(6) as f32);
            let offset=if animated {c.ui.ctx().animate_value_with_time(egui::Id::unique(("hourly-offset",c.live)),desired,0.18)}else{desired};
            let viewport=c.clipped(41.0,88.0,338.0,116.0);
            let low=hours.iter().filter_map(|h|h.temp).fold(f32::INFINITY,f32::min);
            let high=hours.iter().filter_map(|h|h.temp).fold(f32::NEG_INFINITY,f32::max);
            let y_at=|temp:f32| 193.0-(temp-low)/(high-low).max(1.0)*29.0;
            let x_at=|i:usize| 69.0+(i as f32-offset)*56.0;
            let progress=if animated {((env.t-self.hourly.entered) as f32/0.65).clamp(0.0,1.0)}else{1.0};
            for (i,h) in hours.iter().enumerate() {
                let x=x_at(i);
                if let Some(local)=h.local(snapshot.timezone.as_deref()) {
                    let today=place_date(snapshot.timezone.as_deref(),now()).or_else(||chrono::DateTime::from_timestamp(now() as i64,0).map(|t|t.with_timezone(local.offset()).date_naive()));
                    let label=if today.is_some_and(|d|local.date_naive()!=d){local.format("%m/%d %H时").to_string()}else{local.format("%H:%M").to_string()};
                    viewport.text(x,103.0,&label,8.0,W::Regular,theme::GRAY,H::Center);
                }
                if let Some(code)=h.code.as_deref(){let mapped=match code {"350"=>80,"351"=>81,_=>i64::from(crate::qweather::icon_code(code))};icon::icon_at(&viewport,x,120.0,17.0,icon::kind(mapped,h.is_day(snapshot.timezone.as_deref())),None);}
                else {viewport.text(x,125.0,"—",10.0,W::Regular,theme::DIM,H::Center);}
                viewport.text(x,147.0,&h.temp.map(|t|format!("{}°",t.round() as i32)).unwrap_or_else(||"—".into()),10.5,W::Semibold,theme::WHITE,H::Center);
            }
            let segments=hours.len().saturating_sub(1).max(1) as f32;
            for (i,pair) in hours.windows(2).enumerate(){
                let (Some(a),Some(b))=(pair[0].temp,pair[1].temp) else{continue;};
                if pair[1].at-pair[0].at!=3600 {continue;}
                let allowed=(progress*segments-i as f32).clamp(0.0,1.0);if allowed<=0.0{continue;}
                let mut points=Vec::new();
                for j in 0..=12 {let t=(j as f32/12.0).min(allowed);let smooth=t*t*(3.0-2.0*t);points.push((x_at(i)+(x_at(i+1)-x_at(i))*t,y_at(a)+(y_at(b)-y_at(a))*smooth));if t>=allowed{break;}}
                viewport.path(&points,1.7,paint::mix(theme::CYAN,SUN_C,i as f32/segments));
            }
            for (i,h) in hours.iter().enumerate(){if (i as f32)<=progress*segments {if let Some(t)=h.temp{viewport.circle(x_at(i),y_at(t),2.3,paint::mix(theme::CYAN,SUN_C,i as f32/segments));}}}
            if hours.len()>6 {
                c.text(210.0,215.0,"滚动查看更多",8.0,W::Regular,theme::DIM,H::Center);
                c.fill(133.0,211.0,40.0,2.0,1.0,Color32::from_white_alpha(35));
                c.fill(133.0+offset/(hours.len()-6) as f32*24.0,211.0,16.0,2.0,1.0,theme::CYAN);
            }
            if animated && (progress<1.0||(offset-desired).abs()>0.01){c.ui.ctx().request_repaint_after(Duration::from_millis(16));}
        }
        c.text(25.0,crate::app::WEATHER_H-3.0,"小时预报 · QWeather ⓘ",8.0,W::Regular,theme::GRAY,H::Left);
        let credit=if self.hourly.credits().is_empty(){"小时预报由和风天气提供".to_owned()}else{format!("小时预报由和风天气提供\n{}",self.hourly.credits().join("\n"))};
        c.hit(21.0,crate::app::WEATHER_H-15.0,155.0,14.0,"hourly-attribution").on_hover_text(credit);
        if c.live {c.ui.ctx().request_repaint_after(Duration::from_secs(60));}
    }

    fn paint_qweather_picker(&mut self, c:&Card)->bool {
        let x=39.0; let y=38.0; let row=22.0;
        let cities=self.source.config.saved.clone();
        let height=11.0+row*(cities.len() as f32+1.0);
        if c.hit(35.0,34.0,350.0,180.0,"qweather-picker-backdrop").clicked(){self.picker_open=false;}
        c.fill(x,y,240.0,height,10.0,Color32::from_rgb(35,39,47));
        let mut selected=None;
        for (i,city) in cities.iter().enumerate(){
            let yy=y+5.0+i as f32*row;
            let hit=c.hit(x+4.0,yy,232.0,row,("qweather-city",i)).on_hover_cursor(egui::CursorIcon::PointingHand);
            if hit.hovered(){c.fill(x+4.0,yy,232.0,row,6.0,Color32::from_white_alpha(20));}
            c.circle(x+13.0,yy+11.0,3.0,if self.source.config.location.as_ref()==Some(city){theme::CYAN}else{theme::DIM});
            c.text(x+23.0,yy+15.0,&c.fit(&format!("{} · {}",city.position.region,city.position.label),10.0,W::Regular,190.0),10.0,W::Regular,theme::WHITE,H::Left);
            if hit.clicked(){selected=Some(city.clone());}
        }
        let yy=y+5.0+cities.len() as f32*row;
        c.text(x+23.0,yy+15.0,"管理城市…",10.0,W::Semibold,theme::CYAN,H::Left);
        let manage=c.hit(x+4.0,yy,232.0,row,"qweather-manage").clicked();
        if let Some(city)=selected{self.source.choose(city);self.picker_open=false;}
        if manage || (c.live&&c.ui.input(|i|i.key_pressed(egui::Key::Escape))){self.picker_open=false;}
        manage
    }

    /// 返回是否需要打开城市设置。
    pub(crate) fn page_ui(
        &mut self,
        c: &Card,
        env: &crate::app::Env,
        cmds: &mut Vec<crate::app::Cmd>,
    ) -> bool {
        let snapshot = self.current_snapshot().cloned().filter(|_| self.enabled);
        let animated = c.live && crate::bar_layout::animations_enabled()
            && c.ui.ctx().style_of(c.ui.ctx().theme()).animation_time > 0.0;
        let forecast_visible = snapshot.is_some() && !self.hourly.open && !self.indices.open
            && self.signals.view.filter(|_| SIGNALS_ENABLED).is_none();
        if c.live {
            self.snow.update(snapshot.as_ref(), env.t, animated && forecast_visible && !self.solar_open);
        }
        let solar_mix = if forecast_visible && animated {
            c.ui.ctx().animate_bool_with_time(egui::Id::unique(("weather-solar-blend", c.live)), self.solar_open, 0.24)
        } else if self.solar_open { 1.0 } else { 0.0 };
        if animated && forecast_visible && solar_mix < 1.0 {
            if let Some(kind) = snapshot.as_ref().and_then(|s| background_weather(s.reading.code)) {
                let day = snapshot.as_ref().is_some_and(|s| s.reading.day);
                let snow_amount = (self.snow.seconds / 20.0) as f32;
                let textures = if matches!(kind, BackgroundWeather::Cloudy(_)) { self.cloud_textures(c.ui.ctx()) } else { None };
                paint_weather_background(&c.faded((1.0 - solar_mix).powi(2)), kind, env.t, snow_amount, day, textures);
                c.ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        }
        let w = c.text(19.0, 23.0, "当地", 14.0, W::Bold, theme::WHITE, H::Left);
        c.text(19.0 + w, 23.0, "天气", 14.0, W::Bold, theme::CYAN, H::Left);
        let refresh = c
            .hit(300.0, 5.0, 54.0, 25.0, "weather-refresh")
            .on_hover_text(if self.network == Some(false) {
                "网络未连接"
            } else if self.hourly.open {
                "刷新小时预报，计入和风请求额度"
            } else {
                "重新获取当前城市的天气"
            });
        let ready = if self.hourly.open {self.hourly.can_refresh()}else if let Some(kind)=self.signals.view {self.signals.can_refresh(kind)}else if self.indices.open {self.indices.can_refresh()}else{self.can_refresh()};
        let label = if (self.hourly.open && self.hourly.loading()) || self.signals.view.is_some_and(|kind|self.signals.loading(kind)) || (self.indices.open && self.indices.loading()) || (!self.hourly.open && self.signals.view.is_none() && !self.indices.open && (self.request.is_some() || (self.source.is_active() && self.source.runtime.loading()))) {
            "更新中"
        } else if ready {
            "刷新"
        } else {
            "稍后刷新"
        };
        c.text(
            327.0,
            23.0,
            label,
            10.5,
            W::Regular,
            if ready { theme::WHITE } else { theme::DIM },
            H::Center,
        );
        if refresh.clicked() && ready {
            if self.hourly.open {
                if let Some(snapshot)=self.current_snapshot().cloned(){self.refresh_hourly(&snapshot,c.ui.ctx(),true);}
            } else if let Some(kind)=self.signals.view {
                if let Some(snapshot)=self.current_snapshot().cloned(){self.refresh_signal(&snapshot,c.ui.ctx(),kind,true);}
            } else if self.indices.open {
                if let Some(snapshot)=self.current_snapshot().cloned(){self.refresh_indices(&snapshot,c.ui.ctx(),true);}
            } else {self.start_refresh(c.ui.ctx());}
        }

        c.text(
            405.0,
            crate::app::WEATHER_H-3.0,
            if self.source.is_active(){"QWeather"}else{"Open-Meteo"},
            8.0,
            W::Regular,
            theme::DIM,
            H::Right,
        );
        let source_tip=if self.source.is_active() && !self.source.attributions.is_empty() {
            format!("天气数据来自和风天气\n{}",self.source.attributions.join("\n"))
        } else if self.source.is_active() {"天气数据来自和风天气".to_owned()} else {"天气数据来自 Open-Meteo，按 CC BY 4.0 授权".to_owned()};
        if c.hit(349.0, crate::app::WEATHER_H-13.0, 58.0, 12.0, "weather-source")
            .on_hover_text(source_tip)
            .clicked()
        {
            cmds.push(crate::app::Cmd::OpenUrl(if self.source.is_active(){"https://www.qweather.com/"}else{"https://open-meteo.com/"}.into()));
        }
        if let Some(reading) = self.reading() {
            c.text(
                344.0,
                crate::app::WEATHER_H-3.0,
                &format_local_time(reading.at),
                8.0,
                W::Regular,
                if self.stale() || self.failures > 0 {
                    WARN_C
                } else {
                    theme::DIM
                },
                H::Right,
            );
        }

        let picker_was_open=self.picker_open;
        let hourly_was_open=self.hourly.open;
        let body_hit=if snapshot.is_some() && !self.hourly.open && !self.solar_open && !self.indices.open && self.signals.view.is_none() && !self.picker_open {
            Some(c.hit(36.0,34.0,347.0,184.0,"weather-hourly-open").on_hover_text("点击查看小时天气"))
        } else {None};
        if let Some(snapshot) = snapshot.as_ref() {
            if SIGNALS_ENABLED && c.live {self.refresh_signal(snapshot,c.ui.ctx(),signals::Kind::Alert,false);if self.signals.view==Some(signals::Kind::Rain){self.refresh_signal(snapshot,c.ui.ctx(),signals::Kind::Rain,false);}c.ui.ctx().request_repaint_after(Duration::from_secs(60));}
            if self.hourly.open {
                if self.hourly.animate_on_data {self.hourly.entered=env.t;self.hourly.animate_on_data=false;}
                let hit=c.hit_drag(36.0,34.0,347.0,184.0,"weather-hourly-body");
                if hit.dragged(){self.hourly.scroll(-hit.drag_delta().x/(56.0*c.s));c.ui.ctx().request_repaint();}
                self.paint_hourly_view(c,snapshot,env,animated);
                if hit.clicked(){self.hourly.leave();}
            } else if let Some(kind)=self.signals.view.filter(|_|SIGNALS_ENABLED) {
                self.paint_signal_view(c,kind,snapshot);
            } else if self.indices.open {
                self.refresh_indices(snapshot,c.ui.ctx(),false);
                self.paint_indices_view(c);
            } else {
            let was_solar_open=self.solar_open;
            if was_solar_open && c.hit(36.0,34.0,347.0,210.0,"weather-solar-back")
                .on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("返回天气") .clicked() {
                self.solar_open=false;self.solar_at=env.t;
            }
            let mix=solar_mix;
            if mix < 1.0 {
                // Keep text at a stable font size while cross-fading. Scaling the card
                // creates a new glyph size on every frame and stalls the font atlas.
                let foreground=c.faded((1.0-mix).powi(2));
                let foreground=if self.solar_open || was_solar_open {foreground.ghost()} else {foreground};
                self.paint_forecast_view(&foreground,snapshot,env);
            }
            if mix > 0.0 {
                let solar=c.faded(mix.powi(2)).ghost();
                let hovered=c.ui.input(|i|i.pointer.hover_pos().is_some_and(|p|c.rect(35.0,34.0,350.0,180.0).contains(p)));
                if c.live && self.solar_open && self.ambient.advance(env.t,animated,hovered) {c.ui.ctx().request_repaint_after(Duration::from_millis(33));}
                let slide=if animated && self.solar_open {((env.t-self.solar_at) as f32/1.4).clamp(0.0,1.0)}else{1.0};
                paint_solar(&solar,snapshot,now() as i64,self.ambient.time as f32,slide);
                if animated && self.solar_open && slide<1.0 {c.ui.ctx().request_repaint_after(Duration::from_millis(33));}
            }
            if let Some((x,y,at))=self.solar_tap {
                let k=((env.t-at) as f32/0.38).clamp(0.0,1.0);
                if animated && k<1.0 {
                    let e=1.0-(1.0-k).powi(3);let fade=1.0-k;
                    let color=if self.solar_open{SUN_C}else{theme::CYAN};
                    c.ring(x,y,4.0+13.0*e,0.3+1.1*fade,paint::alpha(color,0.7*fade));
                    c.circle(x,y,3.2*(1.0-e),paint::alpha(color,0.85*fade));
                    c.ui.ctx().request_repaint_after(Duration::from_millis(33));
                } else {self.solar_tap=None;}
            }
            if animated && mix>0.0 && mix<1.0 {c.ui.ctx().request_repaint();}
            if self.solar_open != was_solar_open {c.ui.ctx().request_repaint();}
            }
        } else {
            let title = if !self.enabled {
                "天气已关闭"
            } else if self.request.is_some() {
                "正在获取当地天气…"
            } else if self.failures > 0 {
                "天气暂时取不到"
            } else {
                "还没有天气数据"
            };
            c.text(
                209.5,
                93.0,
                title,
                14.0,
                W::Semibold,
                theme::WHITE,
                H::Center,
            );
            let qerror=self.source.runtime.error.map(|e|e.text());
            let detail = if self.source.is_active() {qerror.unwrap_or("请配置和风天气并选择城市")} else if self.status.is_empty() {"选择城市或点击刷新"} else {&self.status};
            c.text(
                209.5,
                113.0,
                &c.fit(detail, 10.0, W::Regular, 300.0),
                10.0,
                W::Regular,
                theme::GRAY,
                H::Center,
            );
        }
        let mut city_hit_rect=None;
        if self.enabled && !hourly_was_open && !self.hourly.open && !self.solar_open && !self.indices.open && self.signals.view.is_none() {
            let label = if self.source.is_active() {self.source.config.location.as_ref().map(|l|format!("{} · {}",l.position.region,l.position.label)).unwrap_or_else(||"选择和风城市".into())} else {self.place.as_ref().map(Place::label).unwrap_or_else(|| "自动定位".into())};
            let (x, y) = if snapshot.is_some() {
                (98.0, 103.0)
            } else {
                (130.0, 136.0)
            };
            let text = c.fit(&label, 10.0, W::Regular, 92.0);
            let text_width = c.text_w(&text, 10.0, W::Regular);
            let width = (text_width + 25.0).min(118.0);
            let trigger = c
                .hit(x - 4.0, y, width, 17.0, "weather-place")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("切换城市");
            city_hit_rect=Some(trigger.rect);
            if trigger.hovered() {
                c.fill(x - 4.0, y, width, 17.0, 8.0, Color32::from_white_alpha(15));
            }
            c.ring(x + 2.0, y + 6.0, 2.7, 1.0, theme::GRAY);
            c.path(
                &[(x - 1.0, y + 7.0), (x + 2.0, y + 11.0), (x + 5.0, y + 7.0)],
                1.0,
                theme::GRAY,
            );
            c.text(
                x + 9.0,
                y + 13.0,
                &text,
                10.0,
                W::Regular,
                theme::GRAY,
                H::Left,
            );
            let arrow = x + 14.0 + text_width;
            c.path(
                &[
                    (arrow - 2.5, y + 7.0),
                    (arrow, y + 9.5),
                    (arrow + 2.5, y + 7.0),
                ],
                1.0,
                theme::GRAY,
            );
            if trigger.clicked() {
                self.picker_open = !self.picker_open;
            }
        }
        if let Some(hit)=body_hit {
            let excluded=hit.interact_pointer_pos().is_some_and(|p|c.rect(212.0,38.0,172.0,73.0).contains(p) || city_hit_rect.is_some_and(|rect|rect.contains(p)));
            if hit.clicked() && !excluded && !picker_was_open && !self.picker_open && !self.solar_open && !self.indices.open {
                if let Some(snapshot)=snapshot.as_ref(){self.hourly.open=true;self.hourly.entered=env.t;self.hourly.offset=0.0;self.refresh_hourly(snapshot,c.ui.ctx(),false);}
            }
        }
        if self.picker_open && c.live {
            if self.source.is_active() {return self.paint_qweather_picker(c);}
            return self.paint_picker(c);
        }
        false
    }

    pub(crate) fn restart_ambient(&mut self) {
        self.ambient.leave();
    }

    pub(crate) fn leave_page(&mut self) {
        self.restart_ambient();
        self.snow = SnowAccumulation::default();
        self.picker_open = false;
        self.solar_open = false;
        self.solar_tap = None;
        self.indices.leave();
        self.signals.set_visible(false);
        self.hourly.leave();
    }

    #[cfg(test)]
    pub(crate) fn test_from_value(value: &Value) -> Self {
        let mut state = Self::from_value(value);
        state.enabled = true;
        state
    }

    #[cfg(test)]
    pub(crate) fn test_select_qweather(&mut self) {self.source.selected=crate::qweather::Provider::Qweather;}

    #[cfg(test)]
    pub(crate) fn test_solar_open(&self) -> bool {
        self.solar_open
    }

    #[cfg(test)]
    pub(crate) fn test_picker_open(&self) -> bool {
        self.picker_open
    }

    #[cfg(test)]
    pub(crate) fn test_show_solar(&mut self) {
        self.solar_open = true;
    }

    #[cfg(test)]
    pub(crate) fn test_seed_indices(&mut self) {
        if let Some(snapshot)=self.current_snapshot().cloned() {
            self.indices.test_set(&snapshot,vec![
                indices::Index{kind:"3".into(),name:"穿衣指数".into(),category:"较冷".into(),text:"建议穿外套保暖".into()},
                indices::Index{kind:"9".into(),name:"感冒指数".into(),category:"较易发".into(),text:"注意保暖".into()},
                indices::Index{kind:"16".into(),name:"防晒指数".into(),category:"较弱".into(),text:"适当防晒".into()},
                indices::Index{kind:"7".into(),name:"花粉过敏指数".into(),category:"不易发".into(),text:"花粉浓度较低".into()},
            ]);
        }
    }

    #[cfg(test)]
    pub(crate) fn test_show_indices(&mut self) {self.indices.open=true;}

    #[cfg(test)]
    pub(crate) fn test_seed_partial_indices(&mut self) {
        if let Some(snapshot)=self.current_snapshot().cloned() {
            self.indices.test_set(&snapshot,vec![indices::Index{kind:"3".into(),name:"穿衣指数".into(),category:"较冷".into(),text:"建议穿外套保暖".into()}]);
        }
    }

    #[cfg(test)]
    pub(crate) fn test_indices_open(&self)->bool {self.indices.open}

    #[cfg(test)]
    pub(crate) fn test_seed_signals(&mut self) {
        if let Some(snapshot)=self.current_snapshot().cloned() {
            self.signals.seed(&snapshot,signals::Kind::Alert,json!({"metadata":{"attributions":["和风天气"]},"alerts":[{"id":"one","headline":"暴雨黄色预警","senderName":"气象台","issuedTime":"2026-11-19T10:00+08:00","description":"请注意防范暴雨。","instruction":"减少外出。","expireTime":"2099-01-01T00:00:00Z"}]}));
            let t=chrono::Utc::now();
            self.signals.seed(&snapshot,signals::Kind::Rain,json!({"code":"200","summary":"近期有雨","updateTime":t.to_rfc3339(),"minutely":[{"fxTime":t.to_rfc3339(),"precip":"0.25","type":"rain"}],"refer":{"sources":["和风天气"]}}));
        }
    }

    #[cfg(test)]
    pub(crate) fn test_show_signal(&mut self,rain:bool) {self.signals.view=Some(if rain{signals::Kind::Rain}else{signals::Kind::Alert});}

    #[cfg(test)]
    pub(crate) fn test_seed_empty_signals(&mut self) {
        if let Some(snapshot)=self.current_snapshot().cloned() {
            self.signals.seed(&snapshot,signals::Kind::Alert,json!({"metadata":{"zeroResult":true}}));
            self.signals.seed(&snapshot,signals::Kind::Rain,json!({"code":"404"}));
        }
    }

    #[cfg(test)]
    pub(crate) fn test_manual_signal_refreshes(&self,rain:bool)->usize {
        self.manual_signal_refreshes[if rain {signals::Kind::Rain}else{signals::Kind::Alert}.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response() -> Value {
        json!({
            "timezone":"Asia/Shanghai",
            "current":{"temperature_2m":-3.6,"weather_code":71,"is_day":1,
                "relative_humidity_2m":58,"wind_speed_10m":13.9,"apparent_temperature":-8.1},
            "daily":{
                "time":["2026-11-15","2026-11-16","2026-11-17","2026-11-18","2026-11-19","2026-11-20","2026-11-21"],
                "weather_code":[71,2,61,3,0,45,95],
                "temperature_2m_max":[0,2,3,4,5,6,7],
                "temperature_2m_min":[-8,-7,-6,-5,-4,-3,-2],
                "precipitation_probability_max":[90,0,65,10,0,15,80],
                "sunrise":["2026-11-15T06:52"],"sunset":["2026-11-15T17:21"]
            }
        })
    }

    fn legacy() -> Value {
        json!({"manual":true,"place":default_place().json(),"located_at":0,
            "reading":{"temp":23,"code":2,"day":true,"at":now()},
            "forecast":[{"label":"明天","code":2,"temp_max":26,"temp_min":18}],
            "humidity":60,"wind_speed":12.0})
    }

    #[test]
    fn weather_parses_seven_days_and_preserves_missing_values() {
        let mut value = response();
        value["daily"]["weather_code"][2] = Value::Null;
        value["daily"]["precipitation_probability_max"][2] = Value::Null;
        value["daily"]["temperature_2m_min"][2] = Value::Null;
        let snapshot = parse_response(default_place(), 0, &value, 123).unwrap();
        assert_eq!(snapshot.reading.temp, -4);
        assert_eq!(snapshot.reading.kind, Kind::Snow);
        assert_eq!(snapshot.feels, Some(-8));
        assert_eq!(snapshot.forecast.len(), 7);
        assert_eq!(snapshot.forecast[0].date, "2026-11-15");
        assert_eq!(snapshot.forecast[0].rain, Some(90));
        assert_eq!(snapshot.forecast[2].code, None);
        assert_eq!(snapshot.forecast[2].rain, None);
        assert_eq!(snapshot.forecast[2].temp_min, None);
        assert!(snapshot.sun.is_some());
        assert!(forecast_is_current(
            &snapshot,
            snapshot.sun.as_ref().unwrap().sunrise as u64
        ));
        assert!(!forecast_is_current(
            &snapshot,
            (snapshot.sun.as_ref().unwrap().day_end + 1) as u64
        ));
        assert_eq!(Snapshot::parse(&snapshot.json()).unwrap().forecast.len(), 7);
        value["current"]["temperature_2m"] = Value::Null;
        assert!(parse_response(default_place(), 0, &value, 123).is_err());
    }

    #[test]
    fn ambient_clock_pauses_and_resumes_without_jumping() {
        let mut clock = AmbientClock::default();
        assert!(clock.advance(0.0, true, false));
        assert!(clock.advance(0.1, true, false));
        assert!((clock.time - 0.1).abs() < 0.0001);
        assert!(!clock.advance(6.1, true, false));
        let paused = clock.time;
        assert!(clock.advance(20.0, true, true));
        assert_eq!(clock.time, paused);
        assert!(clock.advance(20.1, true, true));
        assert!((clock.time - paused - 0.1).abs() < 0.0001);
        assert!(!clock.advance(20.2, false, true));
        let paused = clock.time;
        assert!(clock.advance(21.0, true, true));
        assert_eq!(clock.time, paused);
        clock.leave();
        assert!(clock.advance(30.0, true, false));
        assert_eq!(clock.time, paused);
    }

    #[test]
    fn weather_solar_uses_selected_timezone_and_handles_dst_and_missing_times() {
        let mut value = response();
        value["timezone"] = json!("America/New_York");
        value["daily"]["time"][0] = json!("2026-03-08");
        value["daily"]["sunrise"][0] = json!("2026-03-08T07:10");
        value["daily"]["sunset"][0] = json!("2026-03-08T19:00");
        let sun = parse_sun_times(&value).unwrap();
        assert_eq!(sun.day_end - sun.day_start, 23 * 3600);
        assert_eq!(sun.clock(sun.sunrise), "07:10");
        assert!(
            (sun.progress(sun.sunrise + (sun.sunset - sun.sunrise) / 2)
                .unwrap()
                - 0.5)
                .abs()
                < 0.001
        );
        assert_eq!(sun.progress(sun.sunset + 1), None);
        value["daily"]["sunset"][0] = Value::Null;
        assert!(parse_sun_times(&value).is_none());
    }

    #[test]
    fn weather_migrates_old_city_and_does_not_invent_forecast_dates() {
        let state = State::from_value(&legacy());
        assert!(state.manual);
        assert_eq!(state.place.as_ref().unwrap().name, "深圳");
        assert_eq!(state.saved.len(), 1);
        assert_eq!(state.snapshot.as_ref().unwrap().reading.temp, 23);
        assert!(state.snapshot.as_ref().unwrap().forecast.is_empty());
        assert!(!forecast_is_current(
            state.snapshot.as_ref().unwrap(),
            now()
        ));
        assert_eq!(state.cache.len(), 1);
        let fresh=State::from_value(&Value::Null);
        assert!(!fresh.manual && fresh.place.is_none(), "新安装默认自动定位");
    }

    #[test]
    fn weather_city_switch_restores_cache_and_rejects_old_or_disabled_results() {
        let mut state = State::from_value(&legacy());
        let path = std::env::temp_dir()
            .join(format!(
                "gua-weather-test-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
            .join("weather.json");
        state.cache_path = Some(path.clone());
        state.enabled = true;
        let old_id = state.generation.load(Ordering::Relaxed);
        let old = state.snapshot.clone().unwrap();
        let beijing = Place {
            name: "北京".into(),
            region: "北京".into(),
            lat: 39.9,
            lon: 116.4,
        };
        state.add_city(beijing.clone());
        state.select(Some(beijing.clone()));
        assert!(state.snapshot.is_none());
        state.receive_weather((old_id, Ok(old.clone())));
        assert!(state.snapshot.is_none(), "上一个城市的结果不能覆盖新城市");
        let mut new = old.clone();
        new.place = beijing.clone();
        let current_id = state.generation.load(Ordering::Relaxed);
        state.receive_weather((current_id, Ok(new)));
        assert_eq!(state.snapshot.as_ref().unwrap().place.name, "北京");
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let restored = State::from_value(&saved);
        assert_eq!(restored.saved.len(), 2);
        assert_eq!(restored.place.as_ref().unwrap().name, "北京");
        assert_eq!(restored.cache.len(), 2);
        state.select(Some(default_place()));
        assert_eq!(state.snapshot.as_ref().unwrap().place.name, "深圳");
        let current_id = state.generation.load(Ordering::Relaxed);
        state.set_enabled(false);
        state.receive_weather((current_id, Ok(old)));
        assert!(state.reading().is_none());
        assert_eq!(state.snapshot.as_ref().unwrap().place.name, "深圳");
    }

    #[test]
    fn weather_stale_cache_remains_visible_but_does_not_drive_pet_reactions() {
        let mut value = legacy();
        value["reading"]["at"] = json!(now() - STALE - 1);
        let mut state = State::from_value(&value);
        state.enabled = true;
        assert!(state.reading().is_some());
        assert!(state.stale());
        assert!(state.fresh_reading().is_none());
        state.failures = 1;
        assert_eq!(state.retry_seconds(), 120);
        state.failures = 4;
        assert_eq!(state.retry_seconds(), 960);
        state.failures = 9;
        assert_eq!(state.retry_seconds(), 1800);
        state.last_attempt = Some(Instant::now());
        assert!(!state.can_refresh());
        state.last_attempt = Some(Instant::now() - Duration::from_secs(121));
        assert!(state.can_refresh());
    }

    #[test]
    fn weather_reports_when_a_city_cannot_be_saved() {
        let root = std::env::temp_dir().join(format!(
            "gua-weather-blocked-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&root, b"not a directory").unwrap();
        let mut state = State::from_value(&legacy());
        state.cache_path = Some(root.join("weather.json"));
        state.add_city(Place {
            name: "北京".into(),
            region: "北京".into(),
            lat: 39.9,
            lon: 116.4,
        });
        assert!(state.search_status.contains("无法保存"));
        std::fs::remove_file(root).unwrap();
    }

    #[test]
    #[ignore = "需要连接 Open-Meteo，使用深圳公开坐标"]
    fn weather_live_shenzhen_probe() {
        let generation = AtomicU64::new(1);
        let snapshot = fetch(true, Some(default_place()), 0, &generation, 1).unwrap();
        assert_eq!(snapshot.forecast.len(), FORECAST_DAYS);
        assert_eq!(snapshot.place.name, "深圳");
        assert!(snapshot.sun.is_some());
    }
}
