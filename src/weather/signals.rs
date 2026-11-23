//! On-demand QWeather warnings and precipitation; no work runs while hidden.
use super::{now, Snapshot};
use crate::qweather::{self, Client, Error};
use eframe::egui;
use serde_json::Value;
use std::sync::{atomic::{AtomicBool, Ordering}, mpsc::Receiver, Arc};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind { Alert, Rain }
impl Kind { pub(super) fn index(self)->usize {if self==Self::Alert {0}else{1}} fn ttl(self)->i64 {if self==Self::Alert {1200}else{600}} }

#[derive(Clone, Debug)]
pub(super) struct Alert {pub headline:String,pub sender:String,pub issued:String,pub expires:Option<i64>,pub expire_text:String,pub description:String,pub instruction:String,pub severity:String}
#[derive(Clone, Debug)]
pub(super) struct Point {pub time:i64,pub amount:f32,pub kind:String}
#[derive(Clone, Debug)]
pub(super) enum Data {Alerts(Vec<Alert>), Rain{summary:String,updated:String,points:Vec<Point>},Unavailable}
#[derive(Clone)]
struct Cache {host:String,lat:i32,lon:i32,kind:Kind,fetched:i64,data:Data,credits:Vec<String>}
#[derive(Default)]
struct Slot {request:Option<Receiver<Result<Value,Error>>>,gate:Option<Arc<AtomicBool>>,last:i64,failures:u32,error:Option<Error>,paused:bool}

fn timestamp(s:&str)->Option<i64> {chrono::DateTime::parse_from_rfc3339(s).ok().map(|v|v.timestamp())}
fn strval(v:&Value,key:&str)->String {v[key].as_str().unwrap_or("").to_owned()}
fn parse(kind:Kind,v:&Value,now:i64)->Result<(Data,Vec<String>),Error> {
    if kind==Kind::Rain && v["code"].as_str()==Some("404") {return Ok((Data::Unavailable,vec![]));}
    let credits=if kind==Kind::Alert {v["metadata"]["attributions"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()}
        else {["sources","license"].iter().flat_map(|k|v["refer"][*k].as_array().into_iter().flatten()).filter_map(Value::as_str).map(str::to_owned).collect()};
    if kind==Kind::Alert {
        if v["metadata"]["zeroResult"].as_bool()==Some(true) {return Ok((Data::Alerts(vec![]),credits));}
        let raw=v["alerts"].as_array().ok_or(Error::Response)?;
        if raw.iter().any(|a|a["messageType"]["code"].as_str()!=Some("cancel") && (a["headline"].as_str().is_none() || a["expireTime"].as_str().is_some_and(|t|timestamp(t).is_none()))) {return Err(Error::Response);}
        let superseded:Vec<&str>=raw.iter().flat_map(|a|a["messageType"]["supersedes"].as_array().into_iter().flatten()).filter_map(Value::as_str).collect();
        let alerts=raw.iter().filter(|a| {
            let typ=a["messageType"]["code"].as_str().unwrap_or("");
            typ!="cancel" && !a["id"].as_str().is_some_and(|id|superseded.contains(&id))
                && a["expireTime"].as_str().and_then(timestamp).is_none_or(|end|end>now)
        }).filter_map(|a| {
            let headline=a["headline"].as_str()?.to_owned();
            Some(Alert{headline,sender:strval(a,"senderName"),issued:strval(a,"issuedTime"),expires:a["expireTime"].as_str().and_then(timestamp),expire_text:strval(a,"expireTime"),description:strval(a,"description"),instruction:strval(a,"instruction"),severity:strval(a,"severity")})
        }).collect();
        Ok((Data::Alerts(alerts),credits))
    } else {
        if v["code"].as_str()!=Some("200") {return Err(Error::Response);}
        let values=v["minutely"].as_array().ok_or(Error::Response)?;
        if values.is_empty() {return Ok((Data::Unavailable,credits));}
        let mut points:Vec<Point>=values.iter().filter_map(|p| {
            let time=timestamp(p["fxTime"].as_str()?)?;
            let amount=p["precip"].as_str()?.parse::<f32>().ok()?;
            (amount.is_finite() && amount>=0.0).then(||Point{time,amount,kind:strval(p,"type")})
        }).collect();
        if points.len()!=values.len() {return Err(Error::Response);}
        points.sort_by_key(|p|p.time);
        if points.windows(2).any(|p|p[0].time==p[1].time) {return Err(Error::Response);}
        Ok((Data::Rain{summary:strval(v,"summary"),updated:strval(v,"updateTime"),points},credits))
    }
}

#[derive(Default)]
pub(super) struct State {
    visible:bool, identity:Option<(String,String,bool,i32,i32)>, slots:[Slot;2],cache:Vec<Cache>,offline:bool,
    pub view:Option<Kind>,pub selected:Option<usize>,pub credits_open:bool,pub text_page:usize,pub list_page:usize,
}
impl State {
    fn cancel(&mut self,kind:Kind) {let slot=&mut self.slots[kind.index()];if let Some(g)=slot.gate.take(){g.store(false,Ordering::SeqCst);}slot.request=None;}
    pub fn set_visible(&mut self,visible:bool) {if self.visible&&!visible {self.cancel(Kind::Alert);self.cancel(Kind::Rain);self.view=None;self.selected=None;self.credits_open=false;}self.visible=visible;}
    pub fn sync(&mut self,enabled:bool,snapshot:Option<&Snapshot>,host:&str,credential:&str,provider:bool,network:Option<bool>) {
        self.offline=network==Some(false);
        let identity=snapshot.filter(|_|enabled).map(|s|(host.to_owned(),credential.to_owned(),provider,(s.place.lat*100.0).round() as i32,(s.place.lon*100.0).round() as i32));
        if identity!=self.identity {self.cancel(Kind::Alert);self.cancel(Kind::Rain);let last=[self.slots[0].last,self.slots[1].last];self.slots=[Slot::default(),Slot::default()];self.slots[0].last=last[0];self.slots[1].last=last[1];self.identity=identity;self.view=None;self.selected=None;self.credits_open=false;self.text_page=0;self.list_page=0;}
    }
    fn entry(&self,kind:Kind)->Option<&Cache> {let(host,_,_,lat,lon)=self.identity.as_ref()?;self.cache.iter().find(|c|&c.host==host&&c.lat==*lat&&c.lon==*lon&&c.kind==kind)}
    pub fn data(&self,kind:Kind)->Option<&Data> {self.entry(kind).map(|e|&e.data)}
    pub fn credits(&self,kind:Kind)->&[String] {self.entry(kind).map_or(&[],|e|e.credits.as_slice())}
    pub fn fetched(&self,kind:Kind)->Option<i64> {self.entry(kind).map(|e|e.fetched)}
    pub fn stale(&self,kind:Kind)->bool {self.offline || self.slots[kind.index()].error.is_some() || self.entry(kind).is_none_or(|e|{
        now() as i64-e.fetched>=kind.ttl() || matches!(&e.data,Data::Rain{updated,..} if timestamp(updated).is_none_or(|t|now() as i64-t>=kind.ttl()))
    })}
    pub fn loading(&self,kind:Kind)->bool {self.slots[kind.index()].request.is_some()}
    pub fn can_refresh(&self,kind:Kind)->bool {let s=&self.slots[kind.index()];self.visible&&!self.offline&&self.identity.is_some()&&s.request.is_none()&&(s.last==0||now() as i64-s.last>=60)}
    pub fn status(&self,kind:Kind)->&str {
        let s=&self.slots[kind.index()];
        if s.request.is_some(){"正在获取…"}else if self.offline {"网络未连接"}else if let Some(e)=s.error {e.text()}else if self.entry(kind).is_some()&&self.stale(kind){"数据已过期，请刷新"}else{"暂无数据，请刷新"}
    }
    pub fn alerts(&self)->Vec<Alert> {match self.data(Kind::Alert) {Some(Data::Alerts(a))=>a.iter().filter(|a|a.expires.is_none_or(|end|end>now() as i64)).cloned().collect(),_=>vec![]}}
    pub fn ensure(&mut self,snapshot:&Snapshot,client:Option<Client>,gate:Arc<AtomicBool>,ctx:&egui::Context,kind:Kind,manual:bool) {
        if !self.visible || self.identity.is_none() || self.offline || self.loading(kind) {return;}
        let n=now() as i64;let s=&self.slots[kind.index()];
        if s.last!=0 && (manual || s.failures>0) && n-s.last<if manual {60}else{(120i64 << s.failures.saturating_sub(1).min(4)).min(1800)} {return;}
        if !manual && matches!(self.slots[kind.index()].error,Some(Error::AutoLimit|Error::Limit)) && qweather::usage()<4500 {self.slots[kind.index()].paused=false;self.slots[kind.index()].error=None;}
        let s=&self.slots[kind.index()];
        if !manual && (s.paused || self.entry(kind).is_some_and(|c|n-c.fetched<kind.ttl())) {return;}
        let Some(client)=client else {self.slots[kind.index()].error=Some(Error::Invalid);self.slots[kind.index()].paused=true;return;};
        let s=&mut self.slots[kind.index()];s.last=n;s.error=None;s.gate=Some(gate);if manual{s.paused=false;}
        let lat=snapshot.place.lat;let lon=snapshot.place.lon;let repaint=ctx.clone();
        s.request=Some(qweather::spawn(move||{let r=if kind==Kind::Alert{client.alerts(lat,lon,!manual)}else{client.minutely(lat,lon,!manual)};repaint.request_repaint();r}));
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    pub fn poll(&mut self,ctx:&egui::Context) {
        for kind in [Kind::Alert,Kind::Rain] {
            let slot=&mut self.slots[kind.index()];
            let result=slot.request.as_ref().and_then(|r|match r.try_recv(){Ok(v)=>Some(v),Err(std::sync::mpsc::TryRecvError::Disconnected)=>Some(Err(Error::Network)),_=>None});
            if let Some(result)=result {
                slot.request=None;
                let Some((host,_,_,lat,lon))=self.identity.clone() else {continue;};
                let checked=match result {Err(Error::NoData) if kind==Kind::Rain=>Ok((Data::Unavailable,vec![])),r=>r.and_then(|v|parse(kind,&v,now() as i64))};
                match checked {
                    Ok((data,credits))=>{slot.error=None;slot.failures=0;slot.paused=false;self.cache.retain(|e|!(e.host==host&&e.lat==lat&&e.lon==lon&&e.kind==kind));self.cache.insert(0,Cache{host,lat,lon,kind,fetched:now() as i64,data,credits});self.cache.truncate(12);},
                    Err(e)=>{slot.error=Some(e);slot.failures=slot.failures.saturating_add(1);slot.paused=matches!(e,Error::Invalid|Error::Auth|Error::AutoLimit|Error::Limit|Error::Storage);},
                }
                ctx.request_repaint();
            }
            if slot.request.is_some(){ctx.request_repaint_after(std::time::Duration::from_millis(100));}
        }
    }
    pub fn back(&mut self) {if self.credits_open {self.credits_open=false;self.text_page=0;}else if self.selected.take().is_some(){self.text_page=0;}else{if self.view==Some(Kind::Rain){self.cancel(Kind::Rain);}self.view=None;}}
    #[cfg(test)]
    pub fn seed(&mut self,snapshot:&Snapshot,kind:Kind,value:Value) {
        let lat=(snapshot.place.lat*100.0).round() as i32;let lon=(snapshot.place.lon*100.0).round() as i32;
        self.identity=Some((String::new(),String::new(),false,lat,lon));
        let (data,credits)=parse(kind,&value,now() as i64).unwrap();
        self.cache.retain(|c|c.kind!=kind);self.cache.push(Cache{host:String::new(),lat,lon,kind,fetched:now() as i64,data,credits});
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn snapshot(lat:f64,lon:f64)->Snapshot {Snapshot::parse(&json!({"place":{"name":"城市","region":"","lat":lat,"lon":lon},"reading":{"temp":20,"code":0,"day":true,"at":100},"timezone":"Asia/Shanghai"})).unwrap()}
    #[test]
    fn warnings_keep_current_updates_remove_superseded_cancelled_and_expired() {
        let v=json!({"metadata":{"attributions":["source"]},"alerts":[
            {"id":"old","headline":"old","expireTime":"2030-01-01T00:00:00Z"},
            {"id":"new","headline":"new","expireTime":"2030-01-01T00:00:00Z","messageType":{"code":"update","supersedes":["old"]}},
            {"id":"cancel","messageType":{"code":"cancel"}},
            {"id":"expired","headline":"expired","expireTime":"2020-01-01T00:00:00Z"}]});
        let (Data::Alerts(a),credits)=parse(Kind::Alert,&v,1_750_000_000).unwrap() else {panic!()};
        assert_eq!(a.len(),1);assert_eq!(a[0].headline,"new");assert_eq!(credits,["source"]);
        assert!(matches!(parse(Kind::Alert,&json!({"metadata":{"zeroResult":true}}),0).unwrap().0,Data::Alerts(a) if a.is_empty()));
        assert!(parse(Kind::Alert,&json!({"alerts":[{}]}),0).is_err());
    }
    #[test]
    fn minutely_parses_precipitation_and_distinguishes_no_data() {
        let v=json!({"code":"200","summary":"降雨","updateTime":"2026-11-19T09:00:00+08:00","minutely":[{"fxTime":"2026-11-19T09:00:00+08:00","precip":"0.15","type":"rain"},{"fxTime":"2026-11-19T09:05:00+08:00","precip":"0","type":"snow"}]});
        let (Data::Rain{points,summary,..},_)=parse(Kind::Rain,&v,0).unwrap() else {panic!()};
        assert_eq!(summary,"降雨");assert_eq!(points.len(),2);assert_eq!(points[0].amount,0.15);assert_eq!(points[1].kind,"snow");
        assert!(matches!(parse(Kind::Rain,&json!({"code":"404"}),0).unwrap().0,Data::Unavailable));
        assert!(matches!(parse(Kind::Rain,&json!({"code":"200","minutely":[]}),0).unwrap().0,Data::Unavailable));
        assert!(parse(Kind::Rain,&json!({"code":"200"}),0).is_err());
    }
    #[test]
    fn closing_or_changing_city_discards_inflight_and_retains_isolated_cache() {
        let a=snapshot(22.54,114.06);let b=snapshot(39.90,116.40);let mut state=State::default();let ctx=egui::Context::default();
        state.set_visible(true);state.sync(true,Some(&a),"host","keyslot",false,Some(true));
        state.cache.push(Cache{host:"host".into(),lat:2254,lon:11406,kind:Kind::Alert,fetched:now() as i64,data:Data::Alerts(vec![]),credits:vec![]});
        let gate=Arc::new(AtomicBool::new(true));let(tx,rx)=std::sync::mpsc::channel();state.slots[0].gate=Some(gate.clone());state.slots[0].request=Some(rx);
        tx.send(Ok(json!({"metadata":{"zeroResult":true}}))).unwrap();state.sync(true,Some(&b),"host","keyslot",false,Some(true));state.poll(&ctx);
        assert!(!gate.load(Ordering::SeqCst));assert!(state.data(Kind::Alert).is_none());
        state.sync(true,Some(&a),"host","keyslot",true,Some(true));assert!(state.data(Kind::Alert).is_some());
        let gate=Arc::new(AtomicBool::new(true));let(_tx,rx)=std::sync::mpsc::channel();state.slots[1].gate=Some(gate.clone());state.slots[1].request=Some(rx);state.view=Some(Kind::Rain);state.set_visible(false);
        assert!(!gate.load(Ordering::SeqCst));assert!(state.view.is_none());assert!(!state.loading(Kind::Rain));
    }
    #[test]
    fn old_or_offline_data_is_never_treated_as_fresh() {
        let a=snapshot(22.54,114.06);let mut state=State::default();state.set_visible(true);state.sync(true,Some(&a),"host","keyslot",false,Some(true));
        state.cache.push(Cache{host:"host".into(),lat:2254,lon:11406,kind:Kind::Rain,fetched:now() as i64,data:Data::Rain{summary:"old".into(),updated:"2020-01-01T00:00:00Z".into(),points:vec![]},credits:vec![]});
        assert!(state.stale(Kind::Rain));
        state.cache.push(Cache{host:"host".into(),lat:2254,lon:11406,kind:Kind::Alert,fetched:now() as i64,data:Data::Alerts(vec![]),credits:vec![]});
        assert!(!state.stale(Kind::Alert));state.sync(true,Some(&a),"host","keyslot",false,Some(false));assert!(state.stale(Kind::Alert));
    }
    #[test]
    fn fresh_cache_and_manual_cooldown_prevent_new_requests_and_hidden_page_cancels() {
        let a=snapshot(22.54,114.06);let mut state=State::default();let ctx=egui::Context::default();
        state.set_visible(true);state.sync(true,Some(&a),"host","slot",false,Some(true));
        state.cache.push(Cache{host:"host".into(),lat:2254,lon:11406,kind:Kind::Alert,fetched:now() as i64,data:Data::Alerts(vec![]),credits:vec![]});
        let gate=||Arc::new(AtomicBool::new(true));
        state.ensure(&a,None,gate(),&ctx,Kind::Alert,false);assert_eq!(state.slots[0].error,None);
        state.slots[0].last=now() as i64;state.ensure(&a,None,gate(),&ctx,Kind::Alert,true);assert_eq!(state.slots[0].error,None);
        state.slots[0].last=now() as i64-61;state.ensure(&a,None,gate(),&ctx,Kind::Alert,true);assert_eq!(state.slots[0].error,Some(Error::Invalid));
        state.slots[0].error=None;state.set_visible(false);state.ensure(&a,None,gate(),&ctx,Kind::Alert,true);assert_eq!(state.slots[0].error,None);
    }
}
