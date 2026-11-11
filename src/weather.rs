//! Weather and city lookup, kept off the UI thread.
use crate::paint::{self, Card, H};
use crate::theme::{self, W};
use chrono::Datelike;
use eframe::egui::{self, Color32};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc, Arc,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const REFRESH: u64 = 3600;
const RETRY: u64 = 600;
const RELOCATE: u64 = 86400;
const STALE: u64 = 10800;
pub(crate) const WIDTH: f32 = 46.0;

// ---------- 页面大图标颜色 ----------
const SUN_C: Color32 = Color32::from_rgb(255, 198, 88);
const CLOUD_C: Color32 = Color32::from_rgb(206, 214, 228);
const RAIN_C: Color32 = Color32::from_rgb(116, 176, 255);
const SNOW_C: Color32 = Color32::from_rgb(228, 238, 255);
const BOLT_C: Color32 = Color32::from_rgb(255, 214, 92);
const WARN_C: Color32 = Color32::from_rgb(255, 166, 77);

#[derive(Clone, Debug)]
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
    fn json(&self) -> Value {
        json!({"name":self.name,"region":self.region,"lat":self.lat,"lon":self.lon})
    }
    fn parse(v: &Value) -> Option<Self> {
        let lat = v.get("lat")?.as_f64()?;
        let lon = v.get("lon")?.as_f64()?;
        if !lat.is_finite()
            || !lon.is_finite()
            || !(-90.0..=90.0).contains(&lat)
            || !(-180.0..=180.0).contains(&lon)
        {
            return None;
        }
        Some(Self {
            name: v.get("name")?.as_str()?.to_owned(),
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
    fn label(self) -> &'static str {
        match self {
            Self::Clear => "晴",
            Self::PartlyCloudy => "多云",
            Self::Cloudy => "阴",
            Self::Fog => "雾",
            Self::Rain => "雨",
            Self::Snow => "雪",
            Self::Thunder => "雷雨",
        }
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
pub(crate) struct DailyForecast {
    pub label: String, // "明天"、"周三"
    pub kind: Kind,
    pub temp_max: i32,
    pub temp_min: i32,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct ResultData {
    place: Place,
    reading: Reading,
    forecast: Vec<DailyForecast>,
    located_at: u64,
    humidity: Option<i32>,
    wind_speed: Option<f64>,
}
type WeatherResult = (u64, Result<ResultData, String>);
type SearchResult = (u64, Result<Vec<Place>, String>);

pub(crate) struct State {
    manual: bool,
    place: Option<Place>,
    located_at: u64,
    reading: Option<Reading>,
    forecast: Vec<DailyForecast>,
    humidity: Option<i32>,
    wind_speed: Option<f64>,
    last_attempt: Option<Instant>,
    failed: bool,
    network: Option<bool>,
    enabled: bool,
    generation: Arc<AtomicU64>,
    request: Option<mpsc::Receiver<WeatherResult>>,
    search_generation: Arc<AtomicU64>,
    search: Option<mpsc::Receiver<SearchResult>>,
    query: String,
    candidates: Vec<Place>,
    status: String,
    search_status: String,
}
impl Default for State {
    fn default() -> Self {
        let v = std::fs::read(crate::model::config_dir().join("weather.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .unwrap_or(Value::Null);
        let saved_mode = v.get("manual").and_then(Value::as_bool);
        let place = v.get("place").and_then(Place::parse).or_else(|| {
            saved_mode.is_none().then(|| Place {
                name: "深圳".into(),
                region: "广东".into(),
                lat: 22.5455,
                lon: 114.0683,
            })
        });
        let reading = v.get("reading").and_then(|r| {
            let code = r.get("code")?.as_i64()?;
            let temp = r.get("temp")?.as_i64()?;
            if !(-100..=100).contains(&temp) {
                return None;
            }
            Some(Reading {
                temp: temp as i32,
                kind: Kind::from_code(code)?,
                code,
                day: r.get("day")?.as_bool()?,
                at: r.get("at")?.as_u64()?,
            })
        });
        let reading = if place.is_some() { reading } else { None };
        // 加载持久化的预报和附加数据（向后兼容缺失）
        let forecast = v
            .get("forecast")
            .and_then(|arr| {
                let arr = arr.as_array()?;
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    let code = item.get("code")?.as_i64()?;
                    let kind = Kind::from_code(code)?;
                    out.push(DailyForecast {
                        label: item.get("label")?.as_str()?.to_owned(),
                        kind,
                        temp_max: item.get("temp_max")?.as_i64()? as i32,
                        temp_min: item.get("temp_min")?.as_i64()? as i32,
                    });
                }
                Some(out)
            })
            .unwrap_or_default();
        let humidity = v.get("humidity").and_then(Value::as_i64).map(|x| x as i32);
        let wind_speed = v.get("wind_speed").and_then(Value::as_f64);
        Self {
            manual: saved_mode.unwrap_or(true),
            place,
            located_at: v.get("located_at").and_then(Value::as_u64).unwrap_or(0),
            reading,
            forecast,
            humidity,
            wind_speed,
            last_attempt: None,
            failed: false,
            network: None,
            enabled: false,
            generation: Arc::new(AtomicU64::new(0)),
            request: None,
            search_generation: Arc::new(AtomicU64::new(0)),
            search: None,
            query: String::new(),
            candidates: Vec::new(),
            status: String::new(),
            search_status: String::new(),
        }
    }
}
impl State {
    fn save(&self) {
        let reading = self
            .reading
            .as_ref()
            .map(|r| json!({"temp":r.temp,"code":r.code,"day":r.day,"at":r.at}));
        let forecast=self.forecast.iter().map(|d|json!({"label":d.label,"code":match d.kind {Kind::Clear=>0,Kind::PartlyCloudy=>2,Kind::Cloudy=>3,Kind::Fog=>45,Kind::Rain=>61,Kind::Snow=>71,Kind::Thunder=>95},"temp_max":d.temp_max,"temp_min":d.temp_min})).collect::<Vec<_>>();
        let v = json!({"manual":self.manual,"place":self.place.as_ref().map(Place::json),"located_at":self.located_at,"reading":reading,"forecast":forecast,"humidity":self.humidity,"wind_speed":self.wind_speed});
        if let Ok(bytes) = serde_json::to_vec(&v) {
            let _ = crate::model::atomic_write(
                &crate::model::config_dir().join("weather.json"),
                &bytes,
            );
        }
    }
    pub fn reading(&self) -> Option<&Reading> {
        self.reading
            .as_ref()
            .filter(|r| self.enabled && now().checked_sub(r.at).is_some_and(|age| age < STALE))
    }
    fn invalidate(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.request = None;
        self.last_attempt = None;
        self.failed = false;
    }
    fn cancel_search(&mut self) {
        self.search_generation.fetch_add(1, Ordering::Relaxed);
        self.search = None;
        self.candidates.clear();
    }
    fn select(&mut self, place: Option<Place>) {
        self.invalidate();
        self.cancel_search();
        self.manual = place.is_some();
        self.place = place;
        self.located_at = 0;
        self.reading = None;
        self.forecast.clear();
        self.humidity = None;
        self.wind_speed = None;
        self.status = "等待获取天气".into();
        self.search_status.clear();
        self.save();
    }
    fn set_enabled(&mut self, enabled: bool) {
        if enabled != self.enabled {
            self.enabled = enabled;
            self.invalidate();
            self.cancel_search();
            self.status = (if enabled {
                "等待获取天气"
            } else {
                "天气已关闭"
            })
            .into();
        }
    }
    pub fn tick(&mut self, enabled: bool, network: Option<bool>, ctx: &egui::Context) {
        self.set_enabled(enabled);
        if !enabled {
            self.network = network;
            return;
        }
        let received = self.request.as_ref().map(|r| r.try_recv());
        if matches!(&received, Some(Err(mpsc::TryRecvError::Disconnected))) {
            self.request = None;
            self.failed = true;
            self.status = "天气请求已中断".into();
        } else if let Some(Ok(result)) = received {
            self.request = None;
            if result.0 == self.generation.load(Ordering::Relaxed) {
                match result.1 {
                    Ok(data) => {
                        self.place = Some(data.place);
                        self.located_at = data.located_at;
                        self.reading = Some(data.reading);
                        self.forecast = data.forecast;
                        self.humidity = data.humidity;
                        self.wind_speed = data.wind_speed;
                        self.failed = false;
                        self.status = "天气已更新".into();
                        self.save();
                    }
                    Err(message) => {
                        self.failed = true;
                        self.status = message;
                    }
                }
            }
        }
        let searched = self.search.as_ref().map(|r| r.try_recv());
        if matches!(&searched, Some(Err(mpsc::TryRecvError::Disconnected))) {
            self.search = None;
            self.search_status = "城市搜索已中断".into();
        } else if let Some(Ok(result)) = searched {
            self.search = None;
            if result.0 == self.search_generation.load(Ordering::Relaxed) {
                match result.1 {
                    Ok(places) => {
                        self.search_status = if places.is_empty() {
                            "没有找到该城市".into()
                        } else {
                            "请选择城市".into()
                        };
                        self.candidates = places;
                    }
                    Err(message) => {
                        self.search_status = message;
                        self.candidates.clear();
                    }
                }
            }
        }
        let recovered = self.network == Some(false) && network == Some(true);
        self.network = network;
        if network == Some(false) && self.request.is_none() && self.reading().is_none() {
            self.status = "网络未连接，等待恢复".into();
        }
        let interval = if self.failed { RETRY } else { REFRESH };
        let due = self
            .last_attempt
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(interval))
            || self.failed && recovered;
        if due && self.request.is_none() && network != Some(false) {
            self.last_attempt = Some(Instant::now());
            self.status = "正在获取天气…".into();
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
    pub fn settings_ui(&mut self, ui: &mut egui::Ui, enabled: bool) {
        self.set_enabled(enabled);
        if !enabled {
            ui.small("开启天气后可设置城市。");
            return;
        }
        let source = if self.manual {
            "固定城市"
        } else {
            "网络自动定位"
        };
        ui.label(format!(
            "城市：{}（{}）",
            self.place
                .as_ref()
                .map(Place::label)
                .unwrap_or_else(|| "尚未定位".into()),
            source
        ));
        ui.small("自动定位根据公网 IP 推测，使用代理时可能不准确。");
        ui.horizontal_wrapped(|ui| {
            ui.add_sized(
                [180.0, 30.0],
                egui::TextEdit::singleline(&mut self.query).hint_text("输入城市或区县"),
            );
            if ui.button("搜索").clicked() {
                self.start_search(ui.ctx());
            }
            if ui.button("使用自动定位").clicked() {
                self.select(None);
            }
        });
        if !self.search_status.is_empty() {
            ui.small(&self.search_status);
        }
        let mut chosen = None;
        if !self.candidates.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("weather-city-results")
                .max_height(160.0)
                .show(ui, |ui| {
                    for place in &self.candidates {
                        if ui.button(place.label()).clicked() {
                            chosen = Some(place.clone());
                        }
                    }
                });
        }
        if let Some(place) = chosen {
            self.select(Some(place));
        }
        if self.reading.is_some() && self.reading().is_none() {
            ui.small("天气已超过 3 小时，等待更新。");
        }
        if !self.status.is_empty() {
            ui.small(&self.status);
        }
    }
    pub fn description(&self) -> String {
        match self.reading() {
            Some(r) => format!(
                "{} · {} · {}°C",
                self.place.as_ref().map(Place::label).unwrap_or_default(),
                r.kind.label(),
                r.temp
            ),
            None => self.status.clone(),
        }
    }
}

fn active(g: &AtomicU64, id: u64) -> Result<(), String> {
    if g.load(Ordering::Relaxed) == id {
        Ok(())
    } else {
        Err("天气请求已取消".into())
    }
}
fn get(agent: &ureq::Agent, url: &str, g: &AtomicU64, id: u64) -> Result<String, String> {
    active(g, id)?;
    if !url.starts_with("https://") {
        return Err("天气地址无效".into());
    }
    let text = agent
        .get(url)
        .call()
        .map_err(|_| "无法连接天气服务，请检查网络或代理".to_string())?
        .into_string()
        .map_err(|_| "天气服务返回数据无效".to_string())?;
    active(g, id)?;
    Ok(text)
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
    let ip = get(agent, "https://ipv4.icanhazip.com", g, id)
        .ok()
        .and_then(|s| s.trim().parse::<std::net::Ipv4Addr>().ok());
    active(g, id)?;
    let endpoint = ip
        .map(|ip| format!("https://ipwho.is/{ip}"))
        .unwrap_or_else(|| "https://ipwho.is/".into());
    let first=json_get(agent,&endpoint,g,id).ok().and_then(|v|{
        if v.get("success").and_then(Value::as_bool)==Some(false){return None;}
        Place::parse(&json!({"name":v.get("city").and_then(Value::as_str).unwrap_or("当前位置"),"region":v.get("region").and_then(Value::as_str).unwrap_or(""),"lat":v.get("latitude")?,"lon":v.get("longitude")?}))
    });
    let mut place = if let Some(p) = first {
        p
    } else {
        let v = json_get(agent, "https://ipinfo.io/json", g, id)?;
        let mut parts = v
            .get("loc")
            .and_then(Value::as_str)
            .unwrap_or("")
            .split(',');
        let lat = parts.next().and_then(|s| s.parse::<f64>().ok());
        let lon = parts.next().and_then(|s| s.parse::<f64>().ok());
        Place::parse(&json!({"name":v.get("city").and_then(Value::as_str).unwrap_or("当前位置"),"region":v.get("region").and_then(Value::as_str).unwrap_or(""),"lat":lat,"lon":lon})).ok_or("网络定位失败，请手动选择城市")?
    };
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
) -> Result<ResultData, String> {
    let agent = crate::quota::http_agent();
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
        .append_pair(
            "current",
            "temperature_2m,weather_code,is_day,relative_humidity_2m,wind_speed_10m",
        )
        .append_pair(
            "daily",
            "weather_code,temperature_2m_max,temperature_2m_min",
        )
        .append_pair("forecast_days", "5")
        .append_pair("timezone", "auto");
    let v = json_get(&agent, url.as_str(), g, id)?;
    let c = v.get("current").ok_or("天气数据不完整")?;
    let temp = c
        .get("temperature_2m")
        .and_then(Value::as_f64)
        .filter(|t| t.is_finite() && (-100.0..=100.0).contains(t))
        .ok_or("天气温度无效")?;
    let code = c
        .get("weather_code")
        .and_then(Value::as_i64)
        .ok_or("天气代码缺失")?;
    let kind = Kind::from_code(code).ok_or("天气代码不受支持")?;
    let day = match c.get("is_day").and_then(Value::as_i64) {
        Some(1) => true,
        Some(0) => false,
        _ => return Err("天气昼夜数据缺失".into()),
    };
    let humidity = c
        .get("relative_humidity_2m")
        .and_then(Value::as_i64)
        .map(|x| x as i32);
    let wind_speed = c.get("wind_speed_10m").and_then(Value::as_f64);
    // 解析 daily 预报（跳过今天，取未来 4 天）
    let forecast = parse_daily_forecast(&v);
    Ok(ResultData {
        place,
        located_at,
        reading: Reading {
            temp: temp.round() as i32,
            kind,
            code,
            day,
            at: now(),
        },
        forecast,
        humidity,
        wind_speed,
    })
}

fn parse_daily_forecast(v: &Value) -> Vec<DailyForecast> {
    let daily = match v.get("daily") {
        Some(d) => d,
        None => return Vec::new(),
    };
    let codes = daily
        .get("weather_code")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let maxs = daily
        .get("temperature_2m_max")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let mins = daily
        .get("temperature_2m_min")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let times = daily
        .get("time")
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let weekday_names = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];
    let mut out = Vec::with_capacity(4);
    for i in 1..=4usize {
        let code = codes.get(i).and_then(Value::as_i64);
        let mx = maxs.get(i).and_then(Value::as_f64);
        let mn = mins.get(i).and_then(Value::as_f64);
        let time_str = times.get(i).and_then(Value::as_str);
        let (Some(c), Some(mx), Some(mn)) = (code, mx, mn) else {
            continue;
        };
        let Some(kind) = Kind::from_code(c) else {
            continue;
        };
        let label = compute_daily_label(time_str, i, &weekday_names);
        out.push(DailyForecast {
            label,
            kind,
            temp_max: mx.round() as i32,
            temp_min: mn.round() as i32,
        });
    }
    out
}

fn compute_daily_label(time_str: Option<&str>, offset: usize, weekday_names: &[&str; 7]) -> String {
    // 尝试用解析出的日期算星期；失败则按 offset 硬编码
    if let Some(s) = time_str {
        if let Some(naive) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok() {
            let wd = naive.weekday().num_days_from_sunday() as usize;
            if offset == 1 {
                return "明天".to_string();
            }
            return weekday_names[wd].to_string();
        }
    }
    match offset {
        1 => "明天".into(),
        2 => "后天".into(),
        _ => format!("+{offset}"),
    }
}

/// Compact weather glyphs use distinct accents and inherit the island's skin palette.
pub(crate) fn draw(c: &Card, x: f32, y: f32, reading: &Reading) {
    let color = theme::WHITE;
    let ix = x - WIDTH / 2.0 + 9.0;
    let gold = Color32::from_rgb(255, 190, 45);
    let moonlight = Color32::from_rgb(238, 207, 142);
    let cloud_color = Color32::from_rgb(161, 190, 229);
    let rain = Color32::from_rgb(85, 169, 238);
    let snow = Color32::from_rgb(145, 209, 249);
    let sun = |c: &Card, px: f32, py: f32| {
        c.circle(px, py, 2.7, gold);
        for n in 0..8 {
            let a = n as f32 * std::f32::consts::PI / 4.0;
            c.seg(
                (px + a.cos() * 4.3, py + a.sin() * 4.3),
                (px + a.cos() * 5.9, py + a.sin() * 5.9),
                1.0,
                gold,
            );
        }
    };
    let moon = |c: &Card, px: f32, py: f32| {
        c.path(
            &[
                (px + 2.5, py - 5.0),
                (px - 1.5, py - 4.5),
                (px - 3.5, py - 2.0),
                (px - 3.8, py + 1.0),
                (px - 2.0, py + 4.0),
                (px + 1.0, py + 5.0),
                (px + 3.8, py + 3.0),
                (px + 0.3, py + 2.7),
                (px - 0.8, py + 0.3),
                (px + 0.1, py - 2.8),
                (px + 2.5, py - 5.0),
            ],
            1.4,
            moonlight,
        );
    };
    let cloud = |c: &Card, px: f32, py: f32| {
        c.path(
            &[
                (px - 6.0, py + 2.5),
                (px - 7.0, py + 1.0),
                (px - 6.5, py - 1.0),
                (px - 4.5, py - 2.0),
                (px - 3.0, py - 1.8),
                (px - 2.0, py - 4.0),
                (px + 0.5, py - 5.0),
                (px + 3.0, py - 4.0),
                (px + 4.0, py - 1.8),
                (px + 5.5, py - 1.7),
                (px + 7.0, py),
                (px + 6.5, py + 2.0),
                (px + 5.0, py + 2.5),
                (px - 6.0, py + 2.5),
            ],
            1.3,
            cloud_color,
        );
    };
    match reading.kind {
        Kind::Clear => {
            if reading.day {
                sun(c, ix, y)
            } else {
                moon(c, ix, y)
            }
        }
        Kind::PartlyCloudy => {
            if reading.day {
                sun(c, ix + 3.5, y - 3.0)
            } else {
                moon(c, ix + 3.5, y - 3.0)
            }
            cloud(c, ix - 2.0, y + 2.5);
        }
        Kind::Cloudy => cloud(c, ix, y),
        Kind::Fog => {
            cloud(c, ix, y - 2.5);
            for dy in [3.0, 5.5] {
                c.seg((ix - 5.0, y + dy), (ix + 5.0, y + dy), 1.0, snow);
            }
        }
        Kind::Rain | Kind::Snow | Kind::Thunder => {
            cloud(c, ix, y - 2.5);
            match reading.kind {
                Kind::Rain => {
                    for dx in [-4.0, 0.0, 4.0] {
                        c.seg((ix + dx, y + 2.0), (ix + dx - 1.0, y + 5.0), 1.2, rain);
                    }
                }
                Kind::Snow => {
                    for dx in [-4.0, 0.0, 4.0] {
                        c.seg(
                            (ix + dx - 1.0, y + 3.0),
                            (ix + dx + 1.0, y + 3.0),
                            1.0,
                            snow,
                        );
                        c.seg((ix + dx, y + 2.0), (ix + dx, y + 4.0), 1.0, snow);
                    }
                }
                _ => c.path(
                    &[
                        (ix + 0.5, y + 1.0),
                        (ix - 1.5, y + 4.0),
                        (ix + 1.0, y + 4.0),
                        (ix - 0.5, y + 7.0),
                    ],
                    1.4,
                    gold,
                ),
            }
        }
    }
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

// ---------- 页面大图标绘制 ----------

fn big_sun(c: &Card, cx: f32, cy: f32, s: f32) {
    c.circle(cx, cy, s * 0.40, SUN_C);
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::TAU / 8.0;
        c.seg(
            (cx + angle.cos() * s * 0.58, cy + angle.sin() * s * 0.58),
            (cx + angle.cos() * s * 0.90, cy + angle.sin() * s * 0.90),
            1.7,
            paint::alpha(SUN_C, 0.9),
        );
    }
}

/// 弯月：外圆减去偏右的内圆，用闭合多边形绘制。
fn big_moon(c: &Card, cx: f32, cy: f32, s: f32) {
    let (outer, inner, offset) = (0.62 * s, 0.55 * s, 0.30 * s);
    let x = (offset * offset + outer * outer - inner * inner) / (2.0 * offset);
    let y = (outer * outer - x * x).max(0.0).sqrt();
    let mut points = Vec::with_capacity(40);
    let (top, bottom) = (y.atan2(x), (-y).atan2(x));
    let steps = 18;
    for i in 0..=steps {
        let angle = top + (bottom + std::f32::consts::TAU - top) * i as f32 / steps as f32;
        points.push((cx + angle.cos() * outer, cy + angle.sin() * outer));
    }
    let (inner_bottom, inner_top) = (
        (-y).atan2(x - offset),
        y.atan2(x - offset) - std::f32::consts::TAU,
    );
    for i in 0..=steps {
        let angle = inner_bottom + (inner_top - inner_bottom) * i as f32 / steps as f32;
        points.push((cx + offset + angle.cos() * inner, cy + angle.sin() * inner));
    }
    c.poly(&points, SNOW_C);
}

fn big_cloud(c: &Card, cx: f32, cy: f32, s: f32, color: Color32) {
    c.circle(cx - s * 0.36, cy + s * 0.04, s * 0.32, color);
    c.circle(cx, cy - s * 0.18, s * 0.42, color);
    c.circle(cx + s * 0.38, cy + s * 0.02, s * 0.31, color);
    c.fill(
        cx - s * 0.42,
        cy + s * 0.02,
        s * 0.84,
        s * 0.34,
        s * 0.17,
        color,
    );
}

fn big_icon(c: &Card, cx: f32, cy: f32, size: f32, kind: Kind, is_day: bool) {
    let s = size / 2.0;
    match kind {
        Kind::Clear => {
            if is_day {
                big_sun(c, cx, cy, s)
            } else {
                big_moon(c, cx, cy, s * 1.05)
            }
        }
        Kind::PartlyCloudy => {
            if is_day {
                big_sun(c, cx + s * 0.30, cy - s * 0.30, s * 0.60)
            } else {
                big_moon(c, cx + s * 0.30, cy - s * 0.30, s * 0.63)
            }
            big_cloud(c, cx - s * 0.10, cy + s * 0.22, s * 0.90, CLOUD_C);
        }
        Kind::Cloudy => big_cloud(c, cx, cy, s * 0.98, CLOUD_C),
        Kind::Fog => {
            big_cloud(c, cx, cy - s * 0.14, s * 0.90, CLOUD_C);
            for i in 0..3 {
                c.fill(
                    cx - s * 0.58,
                    cy + s * 0.30 + i as f32 * s * 0.16,
                    s * 1.16,
                    1.4,
                    0.7,
                    paint::alpha(CLOUD_C, 0.72),
                );
            }
        }
        Kind::Rain => {
            big_cloud(c, cx, cy - s * 0.16, s * 0.90, CLOUD_C);
            for i in 0..3 {
                let x = cx - s * 0.32 + i as f32 * s * 0.32;
                c.seg(
                    (x, cy + s * 0.30),
                    (x - s * 0.12, cy + s * 0.64),
                    1.8,
                    RAIN_C,
                );
            }
        }
        Kind::Snow => {
            big_cloud(c, cx, cy - s * 0.16, s * 0.90, CLOUD_C);
            for i in 0..3 {
                let x = cx - s * 0.30 + i as f32 * s * 0.30;
                c.circle(
                    x,
                    cy + s * 0.42 + (i % 2) as f32 * s * 0.14,
                    s * 0.10,
                    SNOW_C,
                );
            }
        }
        Kind::Thunder => {
            big_cloud(c, cx, cy - s * 0.18, s * 0.90, CLOUD_C);
            c.poly(
                &[
                    (cx - s * 0.04, cy + s * 0.26),
                    (cx + s * 0.18, cy + s * 0.26),
                    (cx + s * 0.02, cy + s * 0.56),
                    (cx + s * 0.18, cy + s * 0.56),
                    (cx - s * 0.12, cy + s * 0.88),
                    (cx - s * 0.01, cy + s * 0.54),
                    (cx - s * 0.16, cy + s * 0.54),
                ],
                BOLT_C,
            );
        }
    }
}

// ---------- 天气页面 UI ----------

impl State {
    /// 返回设置按钮是否被点击。
    pub(crate) fn page_ui(&self, c: &Card, _enabled: bool) -> bool {
        // 顶栏：地点 + 设置
        c.text(
            20.0,
            23.0,
            &self
                .place
                .as_ref()
                .map(Place::label)
                .unwrap_or_else(|| "天气".into()),
            11.0,
            W::Semibold,
            theme::WHITE,
            H::Left,
        );

        // 设置按钮（齿轮图标）
        let settings_hit = c.hit(380.0, 4.0, 28.0, 28.0, "weather-settings");
        let (hover, press) = c.hover_t(&settings_hit);
        c.circle(
            394.0,
            18.0,
            13.0,
            Color32::from_white_alpha((hover * 24.0) as u8),
        );
        crate::paint::gear(
            &c.scaled(394.0, 18.0, 0.8 * (1.0 - press * 0.08)),
            394.0,
            18.0,
            paint::mix(theme::GRAY, theme::WHITE, hover),
        );

        let Some(reading) = self.reading() else {
            // 无数据状态
            let title = if self.enabled {
                if self.failed {
                    "天气暂时取不到"
                } else if self.status.is_empty() {
                    "正在获取当地天气…"
                } else {
                    &self.status
                }
            } else {
                "天气已关闭"
            };
            c.text(
                209.5,
                72.0,
                title,
                13.0,
                W::Semibold,
                theme::WHITE,
                H::Center,
            );
            c.text(
                209.5,
                92.0,
                "在设置中开启天气并选择城市",
                10.5,
                W::Regular,
                theme::GRAY,
                H::Center,
            );
            return settings_hit.clicked();
        };

        // 左：当前实况（图标 + 大字温度 + 天气描述 + 地点）
        big_icon(c, 62.0, 61.0, 36.0, reading.kind, reading.day);
        c.text(
            94.0,
            74.0,
            &format!("{}°", reading.temp),
            30.0,
            W::Bold,
            theme::WHITE,
            H::Left,
        );
        c.text(
            96.0,
            92.0,
            reading.kind.label(),
            12.0,
            W::Semibold,
            theme::WHITE,
            H::Left,
        );
        if let Some(p) = &self.place {
            c.text(
                96.0,
                106.0,
                &c.fit(&p.label(), 10.5, W::Regular, 88.0),
                10.5,
                W::Regular,
                theme::GRAY,
                H::Left,
            );
        }

        // 底部附加信息
        let mut meta: Vec<String> = Vec::new();
        if let Some(h) = self.humidity {
            meta.push(format!("湿度 {h}%"));
        }
        if let Some(w) = self.wind_speed {
            meta.push(format!("风 {:.0} km/h", w));
        }
        if !meta.is_empty() {
            c.text(
                20.0,
                121.0,
                &c.fit(&meta.join(" · "), 9.5, W::Regular, 166.0),
                9.5,
                W::Regular,
                theme::DIM,
                H::Left,
            );
        }

        // 右：四日预报
        c.seg(
            (196.0, 34.0),
            (196.0, 108.0),
            0.8,
            Color32::from_white_alpha(16),
        );

        let count = self.forecast.len().clamp(1, 4);
        let span = (378.0 - 200.0) / count as f32;
        for (index, day) in self.forecast.iter().take(4).enumerate() {
            let x = 200.0 + span * (index as f32 + 0.5);
            c.text(
                x,
                45.0,
                &day.label,
                9.5,
                W::Regular,
                if index == 0 {
                    theme::WHITE
                } else {
                    theme::GRAY
                },
                H::Center,
            );
            big_icon(c, x, 66.0, 24.0, day.kind, true);
            c.text(
                x,
                90.0,
                &format!("{}°", day.temp_max),
                12.5,
                W::Semibold,
                theme::WHITE,
                H::Center,
            );
            c.text(
                x,
                103.0,
                &format!("{}°", day.temp_min),
                10.5,
                W::Regular,
                theme::GRAY,
                H::Center,
            );
        }

        settings_hit.clicked()
    }
}
