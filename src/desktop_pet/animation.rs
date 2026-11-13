//! Bounded, background decoder for the bundled animated WebP clips.
use image::{AnimationDecoder, codecs::webp::WebPDecoder};
use std::{io::{BufReader, Cursor}, sync::{Arc, OnceLock, atomic::{AtomicBool, Ordering}, mpsc::{self, Receiver, SyncSender, TrySendError}}, time::Duration};

macro_rules! clip {
    ($name:literal) => { include_bytes!(concat!("../../assets/desktop-pet/", $name, ".webp")) as &'static [u8] };
}
pub(super) static CLIPS: &[&[u8]] = &[
    clip!("00"), clip!("01"), clip!("02"), clip!("03"), clip!("04"), clip!("05"),
    clip!("06"), clip!("07"), clip!("08"), clip!("09"), clip!("10"), clip!("11"),
    clip!("12"), clip!("13"), clip!("14"), clip!("15"), clip!("16"), clip!("17"),
    clip!("18"), clip!("19"), clip!("20"), clip!("21"), clip!("22"), clip!("23"),
    clip!("24"), clip!("25"), clip!("26"), clip!("27"), clip!("28"), clip!("29"),
    clip!("30"), clip!("31"), clip!("32"),
];
pub(super) const MANIFEST: &str = include_str!("../../assets/desktop-pet/manifest.json");

pub(super) struct ClipInfo { pub duration: f32, pub no_mirror: bool }
pub(super) fn catalog() -> Result<&'static [ClipInfo], &'static str> {
    static CATALOG: OnceLock<Result<Vec<ClipInfo>,String>> = OnceLock::new();
    match CATALOG.get_or_init(|| {
        let root:serde_json::Value=serde_json::from_str(MANIFEST).map_err(|e|e.to_string())?;
        if root["upstream_commit"].as_str()!=Some("33938a355f422525886c3facd883d7179c58ebf4") { return Err("素材版本不匹配".into()); }
        let clips=root["clips"].as_array().ok_or("缺少素材列表")?;
        if clips.len()!=CLIPS.len() { return Err("素材数量不匹配".into()); }
        clips.iter().enumerate().map(|(i,clip)| {
            if clip["file"].as_str()!=Some(format!("{i:02}.webp").as_str()) || clip["size"]!=serde_json::json!([640,360]) { return Err(format!("素材 {i} 元数据无效")); }
            let durations=clip["durations_ms"].as_array().ok_or("缺少帧时长")?;
            if durations.is_empty() { return Err("没有动画帧".into()); }
            let mut total=0u64;
            for d in durations { let ms=d.as_u64().filter(|x|*x>0).ok_or("帧时长无效")?; total=total.checked_add(ms).ok_or("帧时长溢出")?; }
            Ok(ClipInfo { duration:total as f32/1000.0, no_mirror:clip["no_mirror"].as_bool().ok_or("缺少镜像标记")? })
        }).collect()
    }) { Ok(clips)=>Ok(clips), Err(error)=>Err(error.as_str()) }
}

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
        let bytes = *CLIPS.get(index).ok_or_else(|| format!("桌宠动画索引无效：{index}"))?;
        let (tx, frames) = mpsc::sync_channel(3);
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        std::thread::Builder::new().name("desktop-pet-decoder".into()).spawn(move || {
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
        match Decoder::start(index) {
            Ok(decoder) => {
                self.decoder = Some(decoder);
                self.index = index;
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
