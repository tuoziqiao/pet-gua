//! 和风生活指数：仅在查看时请求，按城市及当地日期缓存。
use super::{now, Place, Snapshot};
use crate::qweather::{self, Client, Error};
use chrono::{Datelike, TimeZone};
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::Receiver, Arc};

const TTL: i64 = 6 * 3600;
const TYPES: [&str; 4] = ["3", "9", "16", "7"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Index {
    pub kind: String,
    pub name: String,
    pub category: String,
    pub text: String,
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    host: String,
    lat: i32,
    lon: i32,
    date: String,
    fetched: i64,
    indices: Vec<Index>,
    credits: Vec<String>,
}

fn coords(place: &Place) -> (i32, i32) {
    ((place.lat * 100.0).round() as i32, (place.lon * 100.0).round() as i32)
}

fn local_date(snapshot: &Snapshot, timestamp: i64) -> String {
    let date = snapshot.timezone.as_deref().and_then(|t| t.parse::<chrono_tz::Tz>().ok())
        .and_then(|tz| tz.timestamp_opt(timestamp, 0).single())
        .map(|d| d.date_naive())
        .unwrap_or_else(|| chrono::Local.timestamp_opt(timestamp, 0).single().unwrap_or_else(chrono::Local::now).date_naive());
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

fn parse(value: &Value, date: &str) -> Result<(Vec<Index>, Vec<String>), Error> {
    match value["code"].as_str() {
        Some("404") => return Ok((Vec::new(), Vec::new())),
        Some("200") => {},
        _ => return Err(Error::Response),
    }
    let daily = value["daily"].as_array().ok_or(Error::Response)?;
    let find = |kind: &str| daily.iter().find(|v| v["date"].as_str() == Some(date) && v["type"].as_str() == Some(kind))
        .and_then(|v| Some(Index {
            kind: kind.to_owned(), name: v["name"].as_str()?.to_owned(),
            category: v["category"].as_str()?.to_owned(), text: v["text"].as_str().unwrap_or("").to_owned(),
        }));
    let indices = TYPES.iter().filter_map(|kind| find(kind).or_else(|| (*kind == "16").then(|| find("5")).flatten())).collect();
    let credits = ["sources", "license"].iter().flat_map(|k| value["refer"][*k].as_array().into_iter().flatten())
        .filter_map(Value::as_str).map(str::to_owned).collect();
    Ok((indices, credits))
}

#[derive(Default)]
pub(super) struct State {
    pub open: bool,
    pub selected: Option<usize>,
    pub detail_page: usize,
    cache: Vec<Entry>,
    identity: Option<(String, String, bool, i32, i32, String)>,
    gate: Option<Arc<AtomicBool>>,
    request: Option<Receiver<Result<Value, Error>>>,
    last_attempt: i64,
    error: Option<Error>,
    offline: bool,
}

impl State {
    pub fn load() -> Self {
        let mut state = Self::default();
        if !cfg!(test) {
            state.cache = std::fs::read(crate::model::config_dir().join("weather-indices.json")).ok()
                .and_then(|b| serde_json::from_slice::<Vec<Entry>>(&b).ok()).unwrap_or_default();
            state.cache.truncate(6);
        }
        state
    }
    fn cancel(&mut self) {
        if let Some(gate) = self.gate.take() { gate.store(false, Ordering::SeqCst); }
        self.request = None;
    }
    pub fn sync(&mut self, enabled: bool, snapshot: Option<&Snapshot>, host: &str, credential: &str, qweather: bool, network: Option<bool>) {
        self.offline = network == Some(false);
        let identity = snapshot.filter(|_| enabled).map(|s| {
            let (lat, lon) = coords(&s.place);
            (host.to_owned(), credential.to_owned(), qweather, lat, lon, local_date(s, now() as i64))
        });
        if identity != self.identity {
            self.cancel();
            self.identity = identity;
            self.last_attempt = 0;
            self.error = None;
            self.selected = None;
            self.detail_page = 0;
            if !enabled || self.identity.is_none() { self.open = false; }
        }
    }
    fn entry(&self) -> Option<&Entry> {
        let (host, _, _, lat, lon, date) = self.identity.as_ref()?;
        self.cache.iter().find(|e| &e.host == host && e.lat == *lat && e.lon == *lon && &e.date == date)
    }
    pub fn items(&self) -> Vec<Index> {
        let Some(entry) = self.entry() else {return Vec::new();};
        TYPES.iter().filter_map(|kind| entry.indices.iter().find(|i| i.kind == *kind)
            .or_else(|| (*kind == "16").then(|| entry.indices.iter().find(|i| i.kind == "5")).flatten()).cloned()).collect()
    }
    pub fn stale(&self) -> bool { self.entry().is_some_and(|e| self.offline || self.error.is_some() || now() as i64 - e.fetched >= TTL) }
    pub fn credits(&self) -> &[String] { self.entry().map_or(&[], |e| e.credits.as_slice()) }
    pub fn loading(&self) -> bool {self.request.is_some()}
    pub fn can_refresh(&self) -> bool {self.identity.is_some() && !self.offline && self.request.is_none() && (self.last_attempt == 0 || now() as i64 - self.last_attempt >= 30)}
    pub fn status(&self) -> &str {
        if self.request.is_some() { "正在获取生活指数…" }
        else if self.offline { "网络未连接，暂无今日指数" }
        else if let Some(error) = self.error { error.text() }
        else if self.entry().is_some() { "该地区暂无生活指数" }
        else { "点击刷新获取生活指数" }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        let result = self.request.as_ref().and_then(|r| match r.try_recv() {
            Ok(v) => Some(v),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err(Error::Network)),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
        });
        if let Some(result) = result {
            self.request = None;
            let Some((host, _, _, lat, lon, date)) = self.identity.clone() else { return; };
            match result.and_then(|v| parse(&v, &date)) {
                Ok((indices, credits)) => {
                    self.error = None;
                    self.cache.retain(|e| !(e.host == host && e.lat == lat && e.lon == lon && e.date == date));
                    self.cache.insert(0, Entry {host, lat, lon, date, fetched: now() as i64, indices, credits});
                    self.cache.truncate(6);
                    if !cfg!(test) { if let Ok(bytes) = serde_json::to_vec(&self.cache) {
                        let _ = crate::model::atomic_write(&crate::model::config_dir().join("weather-indices.json"), &bytes);
                    }}
                },
                Err(error) => self.error = Some(error),
            }
            ctx.request_repaint();
        }
        if self.request.is_some() {ctx.request_repaint_after(std::time::Duration::from_millis(100));}
    }
    pub fn ensure(&mut self, snapshot: &Snapshot, client: Option<Client>, gate: Arc<AtomicBool>, ctx: &egui::Context, manual: bool) {
        if self.identity.is_none() || self.request.is_some() || self.offline {return;}
        if self.error.is_some() && !manual {return;}
        let timestamp = now() as i64;
        if self.last_attempt != 0 && timestamp - self.last_attempt < if manual {30} else {120} {return;}
        if !manual && self.entry().is_some_and(|e| timestamp - e.fetched < TTL) {return;}
        let Some(client) = client else {self.error = Some(Error::Invalid);return;};
        self.last_attempt = timestamp;
        self.error = None;
        self.gate = Some(gate);
        let lat = snapshot.place.lat; let lon = snapshot.place.lon;
        let repaint = ctx.clone();
        self.request = Some(qweather::spawn(move || {let result = client.indices(lat, lon); repaint.request_repaint(); result}));
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    pub fn new_gate() -> Arc<AtomicBool> { Arc::new(AtomicBool::new(true)) }
    pub fn leave(&mut self) {self.open = false; self.selected = None; self.detail_page = 0; self.cancel();}
    #[cfg(test)]
    pub fn test_set(&mut self, snapshot: &Snapshot, values: Vec<Index>) {
        let (lat,lon)=coords(&snapshot.place); let date=local_date(snapshot,now() as i64);
        self.identity=Some((String::new(),String::new(),false,lat,lon,date.clone()));
        self.cache=vec![Entry{host:String::new(),lat,lon,date,fetched:now() as i64,indices:values,credits:vec![]}];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn snapshot(lat:f64,lon:f64) -> Snapshot {
        Snapshot::parse(&json!({"place":{"name":"城市","region":"","lat":lat,"lon":lon},
            "reading":{"temp":20,"code":0,"day":true,"at":100},"timezone":"Asia/Shanghai"})).unwrap()
    }
    #[test]
    fn selects_today_orders_indices_and_uses_uv_only_as_sunscreen_fallback() {
        let v = json!({"code":"200","daily":[
            {"date":"2026-11-19","type":"5","name":"紫外线指数","category":"弱","text":"注意防晒"},
            {"date":"2026-11-19","type":"1","name":"运动指数","category":"适宜","text":"适宜户外"},
            {"date":"2026-11-19","type":"3","name":"穿衣指数","category":"冷","text":"穿外套"},
            {"date":"2026-11-20","type":"9","name":"感冒指数","category":"易发"}],"refer":{"sources":["source"],"license":["license"]}});
        let (items, credits) = parse(&v, "2026-11-19").unwrap();
        assert_eq!(items.iter().map(|i| i.kind.as_str()).collect::<Vec<_>>(), ["3","5"]);
        assert_eq!(items[0].text, "穿外套"); assert_eq!(credits, ["source","license"]);
        assert!(parse(&json!({"code":"200"}), "2026-11-19").is_err());
        assert!(parse(&json!({"code":"404"}), "2026-11-19").unwrap().0.is_empty());
    }
    #[test]
    fn old_cache_only_exposes_four_indices_with_sunscreen_preferred() {
        let a=snapshot(22.54,114.06);let mut state=State::default();
        let index=|kind:&str| Index{kind:kind.into(),name:kind.into(),category:"正常".into(),text:String::new()};
        state.test_set(&a,vec![index("1"),index("5"),index("7"),index("3"),index("16"),index("9"),index("2")]);
        assert_eq!(state.items().iter().map(|i|i.kind.as_str()).collect::<Vec<_>>(),["3","9","16","7"]);
        state.cache[0].indices.retain(|i|i.kind!="16");
        assert_eq!(state.items().iter().map(|i|i.kind.as_str()).collect::<Vec<_>>(),["3","9","5","7"]);
    }
    #[test]
    fn switching_city_provider_credentials_or_disabling_discards_inflight_data() {
        let ctx=egui::Context::default(); let a=snapshot(22.54,114.06); let b=snapshot(39.90,116.40);
        let mut state=State::default();
        state.sync(true,Some(&a),"host","slot",false,Some(true));
        for (place,host,slot,provider,enabled) in [(&b,"host","slot",false,true),(&b,"host","slot",true,true),(&b,"host","other",true,true),(&b,"host","other",true,false)] {
            let gate=State::new_gate();let(tx,rx)=std::sync::mpsc::channel();state.gate=Some(gate.clone());state.request=Some(rx);
            tx.send(Ok(json!({"code":"200","daily":[]}))).unwrap();
            state.sync(enabled,Some(place),host,slot,provider,Some(true));state.poll(&ctx);
            assert!(!gate.load(Ordering::SeqCst));assert!(state.request.is_none());assert!(state.items().is_empty());
        }
    }
    #[test]
    fn cache_is_scoped_to_city_date_and_host_and_offline_is_marked() {
        let a=snapshot(22.54,114.06);let b=snapshot(39.90,116.40);let mut state=State::default();
        state.test_set(&a,vec![Index{kind:"3".into(),name:"穿衣指数".into(),category:"冷".into(),text:"外套".into()}]);
        state.sync(true,Some(&a),"","",false,Some(false));assert_eq!(state.items().len(),1);assert!(state.stale());
        state.sync(true,Some(&b),"","",false,Some(true));assert!(state.items().is_empty());
        state.sync(true,Some(&a),"other","",false,Some(true));assert!(state.items().is_empty());
        state.cache[0].date="2000-01-01".into();state.sync(true,Some(&a),"","",false,Some(true));assert!(state.items().is_empty());
    }
}
