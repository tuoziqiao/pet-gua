//! On-demand hourly forecast, scoped to the current city and credentials.
use super::{now, Snapshot};
use crate::qweather::{self, Client, Error};
use chrono::Timelike;
use eframe::egui;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::Receiver, Arc};

const TTL:i64=3600;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Hour {pub at:i64,pub offset:i32,pub temp:Option<f32>,pub code:Option<String>}
#[derive(Clone, Serialize, Deserialize)]
struct Entry {host:String,lat:i32,lon:i32,fetched:i64,hours:Vec<Hour>,credits:Vec<String>}
fn parse(value:&Value)->Result<(Vec<Hour>,Vec<String>),Error> {
    if value["code"].as_str()==Some("404") || value["metadata"]["zeroResult"].as_bool()==Some(true){return Err(Error::NoData);}
    let rows=value["hours"].as_array().ok_or(Error::Response)?;
    let mut hours:Vec<_>=rows.iter().filter_map(|row|{
        let text=row["forecastTime"].as_str()?;
        let local=chrono::DateTime::parse_from_rfc3339(text).or_else(|_|chrono::DateTime::parse_from_str(text,"%Y-%m-%dT%H:%M%#z")).ok()?;
        let temp=qweather::temperature(&row["temperature"]).filter(|t|t.is_finite() && (-100.0..=100.0).contains(t));
        let code=row["condition"]["code"].as_str().filter(|s|matches!(s.parse::<u16>().ok(),Some(100..=104|150..=153|300..=318|350..=351|399|400..=410|499|500..=515))).map(str::to_owned);
        Some(Hour{at:local.timestamp(),offset:local.offset().local_minus_utc(),temp,code})
    }).collect();
    hours.sort_by_key(|h|h.at);hours.dedup_by_key(|h|h.at);
    if hours.is_empty(){return Err(Error::NoData);}
    let credits=value["metadata"]["attributions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
    Ok((hours,credits))
}
impl Hour {
    pub fn local(&self,timezone:Option<&str>)->Option<chrono::DateTime<chrono::FixedOffset>> {
        let time=chrono::DateTime::from_timestamp(self.at,0)?;
        if let Some(tz)=timezone.and_then(|s|s.parse::<chrono_tz::Tz>().ok()) {let t=time.with_timezone(&tz);return Some(t.fixed_offset());}
        Some(time.with_timezone(&chrono::FixedOffset::east_opt(self.offset)?))
    }
    pub fn is_day(&self,timezone:Option<&str>)->bool {
        match self.code.as_deref().and_then(|c|c.parse::<u16>().ok()) {Some(150..=153|350..=351)=>false,Some(100..=103)=>true,_=>self.local(timezone).is_some_and(|d|(6..18).contains(&d.hour()))}
    }
}

#[derive(Default)]
pub(super) struct State {
    pub open:bool,pub entered:f64,pub offset:f32,pub animate_on_data:bool,
    identity:Option<(String,String,bool,i32,i32)>,cache:Vec<Entry>,request:Option<Receiver<Result<Value,Error>>>,gate:Option<Arc<AtomicBool>>,
    offline:bool,last:i64,error:Option<Error>,
}
impl State {
    pub fn load()->Self {
        let mut s=Self::default();
        s.cache=std::fs::read(crate::model::config_dir().join("weather-hourly.json")).ok().and_then(|b|serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.cache.truncate(6);s
    }
    fn cancel(&mut self){if let Some(g)=self.gate.take(){g.store(false,Ordering::SeqCst);}self.request=None;}
    pub fn leave(&mut self){self.cancel();self.open=false;self.offset=0.0;self.animate_on_data=false;}
    pub fn sync(&mut self,enabled:bool,snapshot:Option<&Snapshot>,host:&str,credential:&str,provider:bool,network:Option<bool>){
        self.offline=network==Some(false);
        let identity=snapshot.filter(|_|enabled).map(|s|(host.to_owned(),credential.to_owned(),provider,(s.place.lat*100.0).round() as i32,(s.place.lon*100.0).round() as i32));
        if identity!=self.identity {self.cancel();self.identity=identity;self.open=false;self.offset=0.0;self.error=None;}
    }
    fn entry(&self)->Option<&Entry>{let(host,_,_,lat,lon)=self.identity.as_ref()?;self.cache.iter().find(|e|&e.host==host&&e.lat==*lat&&e.lon==*lon)}
    pub fn hours(&self)->Vec<Hour>{let n=now() as i64;self.entry().map(|e|e.hours.iter().filter(|h|h.at>n).take(12).cloned().collect()).unwrap_or_default()}
    pub fn stale(&self)->bool {self.offline||self.error.is_some()||self.entry().is_none_or(|e|now() as i64-e.fetched>=TTL)}
    pub fn fetched(&self)->Option<i64>{self.entry().map(|e|e.fetched)}
    pub fn error(&self)->Option<Error>{self.error}
    pub fn credits(&self)->&[String]{self.entry().map_or(&[],|e|e.credits.as_slice())}
    pub fn loading(&self)->bool{self.request.is_some()}
    pub fn can_refresh(&self)->bool{self.identity.is_some()&&!self.offline&&!self.loading()&&(self.last==0||now() as i64-self.last>=60)}
    pub fn status(&self)->&str{if self.loading(){"正在获取小时预报…"}else if self.offline{"网络未连接"}else if let Some(e)=self.error{e.text()}else{"暂无未来小时预报"}}
    pub fn ensure(&mut self,snapshot:&Snapshot,client:Option<Client>,gate:Arc<AtomicBool>,ctx:&egui::Context,manual:bool){
        if !self.open||!self.can_refresh(){return;}
        let n=now() as i64;
        if !manual && self.entry().is_some_and(|e|n-e.fetched<TTL&&!self.hours().is_empty()){return;}
        let Some(client)=client else{self.error=Some(Error::Invalid);return;};
        self.last=n;self.error=None;self.gate=Some(gate);let lat=snapshot.place.lat;let lon=snapshot.place.lon;let repaint=ctx.clone();
        self.request=Some(qweather::spawn(move||{let r=client.hourly(lat,lon);repaint.request_repaint();r}));
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    pub fn poll(&mut self,ctx:&egui::Context){
        let received=self.request.as_ref().and_then(|r|match r.try_recv(){Ok(v)=>Some(v),Err(std::sync::mpsc::TryRecvError::Disconnected)=>Some(Err(Error::Network)),_=>None});
        if let Some(result)=received {
            self.request=None;let Some((host,_,_,lat,lon))=self.identity.clone() else{return;};
            match result.and_then(|v|parse(&v)) {
                Ok((hours,credits))=>{self.error=None;self.animate_on_data=true;self.cache.retain(|e|!(e.host==host&&e.lat==lat&&e.lon==lon));self.cache.insert(0,Entry{host,lat,lon,fetched:now() as i64,hours,credits});self.cache.truncate(6);
                    if let Ok(bytes)=serde_json::to_vec(&self.cache){let dir=crate::model::config_dir();if std::fs::create_dir_all(&dir).is_ok(){let _=crate::model::atomic_write(&dir.join("weather-hourly.json"),&bytes);}}},
                Err(e)=>self.error=Some(e),
            }
            ctx.request_repaint();
        }
        if self.loading(){ctx.request_repaint_after(std::time::Duration::from_millis(100));}
    }
    pub fn scroll(&mut self,delta:f32)->bool {if !self.open{return false;}let max=self.hours().len().saturating_sub(6) as f32;self.offset=(self.offset+delta).clamp(0.0,max);true}
}
