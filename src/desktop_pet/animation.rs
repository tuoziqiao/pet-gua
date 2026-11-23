//! Bounded, background decoder for the bundled animated WebP clips.
use image::{AnimationDecoder, codecs::webp::WebPDecoder};
use std::{borrow::Cow, collections::HashMap, io::{BufReader, Cursor}, path::PathBuf, sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}}, time::Duration};

macro_rules! clip {
    ($name:literal) => { include_bytes!(concat!("../../assets/desktop-pet/", $name, ".webp")) as &'static [u8] };
}
pub(super) static CLIPS: &[&[u8]] = &[clip!("00"), clip!("landing-dizzy")];
pub(super) const DIZZY_CLIP: usize = usize::MAX;
pub(super) const MANIFEST: &str = include_str!("../../assets/desktop-pet/manifest.json");

pub(super) struct ClipInfo { pub duration: f32, pub no_mirror: bool, pub name:String, file:Option<PathBuf>, embedded:Option<usize> }
pub(super) fn catalog() -> Result<&'static [ClipInfo], &'static str> {
    static CATALOG: OnceLock<Result<Vec<ClipInfo>,String>> = OnceLock::new();
    match CATALOG.get_or_init(|| {
        let root:serde_json::Value=serde_json::from_str(MANIFEST).map_err(|e|e.to_string())?;
        if root["upstream_commit"].as_str()!=Some("33938a355f422525886c3facd883d7179c58ebf4") { return Err("素材版本不匹配".into()); }
        let clips=root["clips"].as_array().ok_or("缺少素材列表")?;
        if clips.len()!=33 { return Err("素材数量不匹配".into()); }
        clips.iter().enumerate().map(|(i,clip)| {
            if clip["file"].as_str()!=Some(format!("{i:02}.webp").as_str()) || clip["size"]!=serde_json::json!([640,360]) { return Err(format!("素材 {i} 元数据无效")); }
            let durations=clip["durations_ms"].as_array().ok_or("缺少帧时长")?;
            if durations.is_empty() { return Err("没有动画帧".into()); }
            let mut total=0u64;
            for d in durations { let ms=d.as_u64().filter(|x|*x>0).ok_or("帧时长无效")?; total=total.checked_add(ms).ok_or("帧时长溢出")?; }
            Ok(ClipInfo { duration:total as f32/1000.0, no_mirror:clip["no_mirror"].as_bool().ok_or("缺少镜像标记")?, name:clip["source"].as_str().unwrap_or("动作").into(), file:if i==0 { None } else { Some(super::download::directory().join(format!("{i:02}.webp"))) }, embedded:if i==0 { Some(0) } else { None } })
        }).collect()
    }) { Ok(clips)=>Ok(clips), Err(error)=>Err(error.as_str()) }
}

#[derive(Clone, Copy)]
pub(super) struct MoveInfo { pub min_dist:u32, pub max_dist:u32, pub lead:f32, pub tail:f32 }
struct FullCatalog { clips:Vec<ClipInfo>, groups:HashMap<String,Vec<usize>>, weights:HashMap<String,usize>, moves:HashMap<usize,MoveInfo> }
fn full_catalog() -> Option<&'static FullCatalog> {
    static FULL:OnceLock<Option<FullCatalog>>=OnceLock::new();
    FULL.get_or_init(|| {
        let result=(|| -> Result<FullCatalog,String> {
            let dir=super::download::directory();
            let root:serde_json::Value=serde_json::from_str(super::download::MANIFEST).map_err(|e|e.to_string())?;
            if root["upstream_commit"].as_str()!=Some("572cf0ff68e7ffbe76afe4ba5fd8ae99ec7e0733") { return Err("动画素材版本不匹配".into()); }
            let entries=root["clips"].as_array().ok_or("缺少动画清单")?;
            let mut clips=Vec::with_capacity(entries.len());
            for (index,item) in entries.iter().enumerate() {
                let embedded=item["embedded"].as_u64().map(|v|v as usize);
                if embedded.is_some_and(|v|v>=CLIPS.len() || v!=index) { return Err("内嵌动画索引无效".into()); }
                let file=if let Some(name)=item["file"].as_str() {
                    let bytes=name.as_bytes();
                    let legacy=bytes.len()==7 && bytes[..2].iter().all(|b|b.is_ascii_digit()) && name.ends_with(".webp") && name!="00.webp";
                    let hashed=bytes.len()==69 && bytes[..64].iter().all(|b|b.is_ascii_hexdigit()) && name.ends_with(".webp");
                    if !legacy && !hashed { return Err("动画文件名无效".into()); }
                    Some(dir.join(name))
                } else { None };
                if embedded.is_none() && file.is_none() { return Err("动画引用缺失".into()); }
                let durations=item["durations_ms"].as_array().ok_or("帧时长缺失")?;
                let mut total=0u64;
                for v in durations { total=total.checked_add(v.as_u64().filter(|n|*n>0).ok_or("帧时长无效")?).ok_or("帧时长溢出")?; }
                if total==0 { return Err("动画没有帧".into()); }
                clips.push(ClipInfo { duration:total as f32/1000.0, no_mirror:item["no_mirror"].as_bool().unwrap_or(true), name:item["name"].as_str().unwrap_or("动作").into(), embedded, file });
            }
            if clips.len()<CLIPS.len() { return Err("动画清单不完整".into()); }
            let mut groups=HashMap::new();
            for (name,items) in root["groups"].as_object().ok_or("缺少动作分类")? {
                let mut indices=Vec::new();
                for item in items.as_array().ok_or("动作分类无效")? { let i=item.as_u64().ok_or("动作索引无效")? as usize; if i>=clips.len() { return Err("动作索引越界".into()); } indices.push(i); }
                groups.insert(name.clone(),indices);
            }
            let mut weights=HashMap::new();
            for (key,value) in root["category_weights"].as_object().ok_or("缺少分类权重")? { weights.insert(key.clone(),value.as_u64().unwrap_or(0) as usize); }
            let mut moves=HashMap::new();
            let defaults=&root["moves"]["default"];
            if let Some(actions)=root["moves"]["actions"].as_array() {
                for action in actions {
                    let Some(name)=action["name"].as_str() else { continue; };
                    let Some(index)=clips.iter().position(|c|c.name==name) else { continue; };
                    let params=&action["params"];
                    let number=|key:&str|params[key].as_f64().or_else(||defaults[key].as_f64()).unwrap_or(0.0);
                    let min_dist=number("minDist").max(0.0) as u32;
                    moves.insert(index,MoveInfo { min_dist,max_dist:(number("maxDist").max(min_dist as f64) as u32).min(10000),lead:number("leadSec").max(0.0) as f32,tail:number("tailSec").max(0.0) as f32 });
                }
            }
            Ok(FullCatalog { clips,groups,weights,moves })
        })();
        match result { Ok(v)=>Some(v), Err(e)=>{ eprintln!("扩展桌宠素材不可用：{e}；仅使用内嵌动画"); None } }
    }).as_ref()
}
pub(super) fn clip_info(index:usize) -> Option<&'static ClipInfo> {
    if index==DIZZY_CLIP {
        static DIZZY:OnceLock<ClipInfo>=OnceLock::new();
        return Some(DIZZY.get_or_init(||ClipInfo { duration:4.999, no_mirror:false, name:"甩出落地晕眩".into(), file:None, embedded:Some(1) }));
    }
    full_catalog().and_then(|c|c.clips.get(index)).or_else(||catalog().ok()?.get(index))
}
pub(crate) fn clip_available(index:usize) -> bool {
    if index==0 || index==DIZZY_CLIP { return true; }
    clip_info(index).and_then(|c|c.file.as_ref()).and_then(|p|p.file_name()).and_then(|n|n.to_str()).is_some_and(super::download::has)
}
pub(super) fn availability_revision()->u64 { super::download::revision() }
pub(super) fn invalidate_clip(index:usize) {
    if let Some(name)=clip_info(index).and_then(|c|c.file.as_ref()).and_then(|p|p.file_name()).and_then(|n|n.to_str()) { super::download::invalidate(name); }
}
pub(super) fn group(name:&str) -> Option<&'static [usize]> { full_catalog()?.groups.get(name).map(Vec::as_slice) }
pub(super) fn weight(name:&str) -> usize { full_catalog().and_then(|c|c.weights.get(name)).copied().unwrap_or(0) }
pub(super) fn movement(index:usize) -> Option<MoveInfo> { full_catalog()?.moves.get(&index).copied() }

pub(super) struct Frame {
    pub pixels: Vec<u8>,
    pub size: [usize; 2],
    pub duration: Duration,
}
pub(super) enum Message { Frame(Frame), End, Error(String) }

pub(super) struct Decoder {
    pub frames: Receiver<Message>,
    cancel: Arc<AtomicBool>,
}

fn send(tx: &SyncSender<Message>, cancel: &AtomicBool, mut message: Message) -> bool {
    loop {
        if cancel.load(Ordering::Relaxed) { return false; }
        match tx.try_send(message) {
            Ok(()) => return true,
            Err(TrySendError::Disconnected(_)) => return false,
            Err(TrySendError::Full(value)) => {
                message = value;
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

impl Decoder {
    pub fn start(index: usize) -> Result<Self, String> {
        if !clip_available(index) { return Err("动画尚未加载".into()); }
        let info=clip_info(index).ok_or_else(||format!("桌宠动画索引无效：{index}"))?;
        let embedded=info.embedded.and_then(|i|CLIPS.get(i).copied());
        let name=info.file.as_ref().and_then(|p|p.file_name()).and_then(|n|n.to_str()).map(str::to_owned);
        let (tx, frames) = mpsc::sync_channel(3);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::Builder::new().name("desktop-pet-decoder".into()).spawn(move || {
            let bytes:Cow<'static,[u8]>=if let Some(bytes)=embedded { Cow::Borrowed(bytes) } else {
                let Some(name)=name else { send(&tx,&worker_cancel,Message::Error("素材名称缺失".into())); return; };
                match super::download::read_clip(&name) { Ok(bytes)=>Cow::Owned(bytes), Err(err)=>{ send(&tx,&worker_cancel,Message::Error(err)); return; } }
            };
            let decoder = match WebPDecoder::new(BufReader::new(Cursor::new(bytes))) {
                Ok(decoder) => decoder,
                Err(err) => { send(&tx, &worker_cancel, Message::Error(err.to_string())); return; }
            };
            for decoded in decoder.into_frames() {
                match decoded {
                    Ok(frame) => {
                        let (n, d) = frame.delay().numer_denom_ms();
                        let image = frame.into_buffer();
                        let size = [image.width() as usize, image.height() as usize];
                        if size != [640,360] { send(&tx,&worker_cancel,Message::Error("动画尺寸无效".into())); return; }
                        let duration = Duration::from_secs_f64((n as f64 / d.max(1) as f64 / 1000.0).max(0.001));
                        if !send(&tx, &worker_cancel, Message::Frame(Frame { pixels: image.into_raw(), size, duration })) { return; }
                    }
                    Err(err) => { send(&tx, &worker_cancel, Message::Error(err.to_string())); return; }
                }
            }
            send(&tx, &worker_cancel, Message::End);
        }).map_err(|err| err.to_string())?;
        Ok(Self { frames, cancel })
    }
}

impl Drop for Decoder {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Relaxed); }
}

/// Keep just one displayed frame and a few pending decoded frames in memory.
#[derive(Default)]
pub(super) struct Player {
    decoder: Option<Decoder>,
    pub texture: Option<eframe::egui::TextureHandle>,
    pub mask: Option<(Vec<u8>, [usize; 2])>,
    pub index: usize,
    time_left: f32,
    pub ended: bool,
    pub error: Option<String>,
}

impl Player {
    pub fn is_started(&self) -> bool { self.decoder.is_some() || self.texture.is_some() || self.ended }
    pub fn play(&mut self, index: usize) {
        self.index = index;
        match Decoder::start(index) {
            Ok(decoder) => {
                self.decoder = Some(decoder);
                self.ended = false;
                self.error = None;
                self.time_left = 0.0;
            }
            Err(error) => { self.error = Some(error); self.ended = true; }
        }
    }

    pub fn step(&mut self, ctx: &eframe::egui::Context, dt: f32) -> bool {
        self.time_left -= dt.max(0.0);
        if self.time_left > 0.0 { return false; }
        let Some(decoder) = &self.decoder else { return false; };
        match decoder.frames.try_recv() {
            Ok(Message::Frame(frame)) => {
                self.time_left += frame.duration.as_secs_f32();
                let image = eframe::egui::ColorImage::from_rgba_unmultiplied(frame.size, &frame.pixels);
                if let Some(texture) = &mut self.texture {
                    texture.set(image, eframe::egui::TextureOptions::LINEAR);
                } else {
                    self.texture = Some(ctx.load_texture("desktop-pet-frame", image, eframe::egui::TextureOptions::LINEAR));
                }
                self.mask = Some((frame.pixels, frame.size));
                true
            }
            Ok(Message::End) => { self.ended = true; self.decoder = None; false }
            Ok(Message::Error(error)) => { self.error = Some(error); self.ended = true; self.decoder = None; false }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.error = Some("动画解码线程已退出".into()); self.ended = true; self.decoder = None; false
            }
            Err(mpsc::TryRecvError::Empty) => { self.time_left = 0.0; false }
        }
    }
}
