//! Human desktop pet: playback and interaction are independent from the island mascot.
use super::{animation::{self, Player}, Event, TITLE};
use crate::model::{Settings, DESKTOP_PET_MIN_SIZE, DESKTOP_PET_MAX_SIZE};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use std::time::{Duration, Instant};

pub(crate) const CATEGORIES: &[(&str, &[usize])] = &[
    ("待机", &[0, 1]), ("移动", &[8, 9, 10]), ("小动作", &[11, 12, 13, 14]),
    ("玩耍", &[15, 16, 17, 18]), ("吃东西", &[19, 20, 21]),
    ("时节", &[22, 23, 24]), ("文字", &[25, 26]),
    ("点击回应", &[3, 4, 5, 6, 7]), ("拖拽", &[2]), ("说话", &[27, 28, 29]),
    ("睡觉", &[30, 31, 32]),
];
const WHISPERS: &[&str] = &["今天也要好好休息哦。", "喝点水吧。", "我在这里陪着你。", "忙完记得伸个懒腰。", "慢慢来，一切都会好的。"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode { Normal, Drag, SleepStart, Sleep, Wake }
impl Default for Mode { fn default() -> Self { Self::Normal } }

#[derive(Clone, Copy)]
pub(crate) enum Command { Sleep, Wake, Recenter }

#[derive(Default)]
pub(super) struct Human {
    active: Player,
    incoming: Option<Player>,
    prepared: Option<Player>,
    mode: Mode,
    visible: bool,
    initialized: bool,
    dragging: bool,
    drag_offset: (i32, i32),
    last_frame: Option<Instant>,
    seed: u64,
    facing_right: bool,
    whisper_clock: f32,
    bubble: Option<(&'static str, f32)>,
    fade: f32,
    target: usize,
    start_x: Option<i32>,
    movement: f32,
    elapsed: f32,
    failed: Vec<usize>,
    error: Option<String>,
    animations_enabled: bool,
    region_state: Option<((i32,i32),usize,bool)>,
    region_elapsed: f32,
}

impl Human {
    pub(super) fn error_message(&self) -> Option<String> { self.error.clone() }
    fn whisper_due(&mut self, dt:f32, motion:bool) -> bool {
        if !motion || self.mode!=Mode::Normal { return false; }
        self.whisper_clock+=dt.max(0.0);
        if self.whisper_clock>=300.0 { self.whisper_clock=0.0; true } else { false }
    }
    fn random(&mut self, n: usize) -> usize {
        if self.seed == 0 { self.seed = 0x94d049bb133111eb ^ std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64; }
        self.seed ^= self.seed << 13; self.seed ^= self.seed >> 7; self.seed ^= self.seed << 17;
        (self.seed as usize) % n.max(1)
    }
    fn pick(&mut self, indices: &[usize]) -> usize {
        let available: Vec<_> = indices.iter().copied().filter(|i| !self.failed.contains(i)).collect();
        if available.is_empty() { return 0; }
        available[self.random(available.len())]
    }
    fn play(&mut self, index: usize) {
        if self.failed.contains(&index) { return; }
        let mut player=Player::default(); player.play(index);
        self.incoming=Some(player); self.target=index; self.fade=0.0; self.elapsed=0.0;
        self.prepared=None;
        self.start_x=None; self.movement=0.0;
    }
    fn next(&mut self) {
        if self.mode==Mode::Normal {
            if let Some(prepared)=self.prepared.take() {
                if self.active.index==1 { self.facing_right=!self.facing_right; }
                self.target=prepared.index; self.incoming=Some(prepared); self.fade=0.0; return;
            }
        }
        let next=match self.mode {
            Mode::Drag => 2,
            Mode::SleepStart => { self.mode=Mode::Sleep; 31 },
            Mode::Sleep => 31,
            Mode::Wake => { self.mode=Mode::Normal; 0 },
            Mode::Normal => {
                let roll=self.random(100);
                if roll<25 { 0 } else if roll<35 { 1 } else if roll<50 { self.pick(&[8,9,10]) }
                else { self.pick(&(11..=26).collect::<Vec<_>>()) }
            }
        };
        if self.active.index==1 && self.mode==Mode::Normal { self.facing_right=!self.facing_right; }
        self.play(next);
    }
    fn animate(&mut self, ctx: &egui::Context, dt: f32) -> bool {
        let mut changed=false;
        if let Some(next)=&mut self.incoming {
            changed|=next.step(ctx,dt);
            if let Some(error)=next.error.take() {
                let index=next.index; self.failed.push(index); eprintln!("桌宠动画 {index}: {error}");
                self.incoming=None;
                if index==0 { self.error=Some(format!("待机动画不可用：{error}")); }
            } else if next.texture.is_some() {
                self.fade+=dt;
                if self.active.texture.is_none() || self.fade>=0.16 {
                    self.active=self.incoming.take().unwrap(); self.fade=0.0; self.elapsed=0.0; self.start_x=None;
                }
            }
        }
        if self.incoming.is_none() {
            self.elapsed+=dt;
            changed|=self.active.step(ctx,dt);
            if self.active.ended {
                if let Some(error)=self.active.error.take() {
                    self.failed.push(self.active.index); eprintln!("桌宠动画 {}: {error}",self.active.index);
                    if self.active.index==0 { self.error=Some(format!("待机动画不可用：{error}")); }
                }
                if self.error.is_none() { self.next(); }
            }
        }
        changed
    }
    fn reset(&mut self) { *self=Self::default(); }
    pub(super) fn show(&mut self, ctx:&egui::Context, settings:&mut Settings, fullscreen:bool, system_animations:bool, event:Option<Event>, preview:Option<usize>, command:Option<Command>) {
        if !settings.desktop_pet {
            #[cfg(windows)]
            if self.dragging { if let Some((pos,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(pos); settings.save(); } }
            if self.initialized { self.reset(); } return;
        }
        let hidden=fullscreen && settings.hide_fullscreen;
        let motion=settings.desktop_pet_motion && system_animations;
        if let Err(error)=animation::catalog() { self.error=Some(format!("桌宠素材清单错误：{error}")); }
        if self.animations_enabled && !motion && self.initialized { self.mode=Mode::Normal; self.bubble=None; self.play(0); }
        self.animations_enabled=motion;
        let requested=Vec2::new(462.0,260.0)*(settings.desktop_pet_size_pct.clamp(DESKTOP_PET_MIN_SIZE,DESKTOP_PET_MAX_SIZE) as f32/100.0);
        let id=egui::ViewportId::from_hash_of("gua-desktop-pet");
        let mut bubble_pos=None;
        ctx.show_viewport_immediate(id, egui::ViewportBuilder::default().with_title(TITLE).with_inner_size(requested).with_decorations(false).with_transparent(true).with_resizable(false).with_taskbar(false).with_always_on_top().with_active(false).with_visible(false), |ui,_| {
            let now=Instant::now();
            if hidden {
                if self.visible { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false)); self.visible=false; }
                self.last_frame=None;
                #[cfg(windows)]
                if self.dragging { if let Some((pos,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(pos); settings.save(); } }
                self.dragging=false; return;
            }
            #[cfg(windows)]
            if !self.initialized {
                if super::native::initialize(settings.desktop_pet_position).is_some() { self.initialized=true; }
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            }
            #[cfg(not(windows))]
            { self.initialized=true; }
            if !self.initialized { return; }
            let dt=self.last_frame.replace(now).map_or(0.0,|t| now.duration_since(t).as_secs_f32());
            let frame_dt=dt.min(0.1);
            self.region_elapsed+=frame_dt;
            #[cfg(windows)]
            super::native::ensure_frameless();
            if !self.active.is_started() && self.incoming.is_none() && self.error.is_none() { self.play(0); }
            match command {
                Some(Command::Sleep) if motion && !matches!(self.mode,Mode::Sleep|Mode::SleepStart) => { self.mode=Mode::SleepStart; self.bubble=None; self.play(30); }
                Some(Command::Wake) if motion && matches!(self.mode,Mode::Sleep|Mode::SleepStart) => { self.mode=Mode::Wake; self.play(32); }
                Some(Command::Recenter) => {
                    #[cfg(windows)]
                    if let Some((pos,size))=super::native::window_geometry() { super::native::force_visible(pos,size); if let Some((p,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(p); settings.save(); } }
                }
                _ => {}
            }
            if let Some(index)=preview { if index<33 && motion { self.mode=Mode::Normal; self.play(index); } }
            if event.is_some() && self.mode==Mode::Normal && motion { let index=self.pick(&[3,4,5,6,7]); self.play(index); }
            let rect=ui.max_rect();
            let response=ui.interact(rect,ui.scope_id().with("human"),Sense::click_and_drag());
            #[cfg(windows)]
            let mut geometry=super::native::window_geometry();
            #[cfg(windows)]
            let cursor=super::native::cursor_screen();
            #[cfg(windows)]
            if response.drag_started() {
                if let (Some((pos,_)),Some(cursor))=(geometry,cursor) {
                    self.dragging=true; self.drag_offset=(cursor.0-pos.0,cursor.1-pos.1);
                    if motion { self.mode=Mode::Drag; self.play(2); }
                }
            }
            #[cfg(not(windows))]
            if response.drag_started() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag); }
            #[cfg(windows)]
            if self.dragging {
                if super::native::left_button_down() {
                    if let Some(cursor)=cursor { super::native::move_to((cursor.0-self.drag_offset.0,cursor.1-self.drag_offset.1)); geometry=super::native::window_geometry(); }
                } else {
                    self.dragging=false; self.mode=Mode::Normal;
                    if let Some((pos,_))=geometry { settings.desktop_pet_position=Some(pos); settings.save(); }
                    if motion { self.play(0); }
                }
            }
            if response.clicked() && !self.dragging {
                if matches!(self.mode,Mode::Sleep|Mode::SleepStart) { self.mode=Mode::Wake; self.play(32); }
                else if motion { let index=self.pick(&[3,4,5,6,7]); self.play(index); }
            }
            if self.whisper_due(dt,motion) {
                let text=WHISPERS[self.random(WHISPERS.len())]; self.bubble=Some((text,5.0));
                let index=self.pick(&[27,28,29]); self.play(index);
            }
            if let Some((_,remaining))=&mut self.bubble { *remaining-=dt; if *remaining<=0.0 { self.bubble=None; } }
            let changed=if motion || self.incoming.is_some() || self.active.texture.is_none() { self.animate(ui.ctx(),frame_dt) } else { false };
            if motion && self.mode==Mode::Normal && self.incoming.is_none() && self.prepared.is_none() && self.active.texture.is_some() {
                if let Ok(clips)=animation::catalog() { if self.elapsed >= (clips[self.active.index].duration-0.7).max(0.0) {
                    let roll=self.random(100);
                    let index=if roll<25 {0} else if roll<35 {1} else if roll<50 {self.pick(&[8,9,10])} else {self.pick(&(11..=26).collect::<Vec<_>>())};
                    let mut player=Player::default(); player.play(index); self.prepared=Some(player);
                } }
            }
            if let Some(prepared)=&mut self.prepared { if prepared.texture.is_none() { prepared.step(ui.ctx(),0.0); } }
            #[cfg(windows)]
            if motion && self.mode==Mode::Normal && self.incoming.is_none() && matches!(self.active.index,8|9|10) {
                if let (Some((pos,size)),Ok(clips))=(geometry,animation::catalog()) {
                    let start=*self.start_x.get_or_insert(pos.0);
                    let duration=clips[self.active.index].duration;
                    let lead=if self.active.index==10 {1.75} else {2.0};
                    let tail=if self.active.index==10 {4.8} else {2.0};
                    let progress=((self.elapsed-lead)/(duration-lead-tail).max(0.5)).clamp(0.0,1.0);
                    let distance=size.0 as f32*0.45*if self.facing_right {1.0} else {-1.0};
                    super::native::move_clamped((start+(distance*progress) as i32,pos.1),size);
                    geometry=super::native::window_geometry();
                    if progress>=1.0 && self.movement<1.0 { if let Some((p,_))=geometry { settings.desktop_pet_position=Some(p); settings.save(); } }
                    self.movement=progress;
                }
            }
            let visible_player=self.incoming.as_ref().filter(|p|p.texture.is_some()).unwrap_or(&self.active);
            #[cfg(windows)]
            if let Some((pos,size))=geometry {
                let no_mirror=animation::catalog().ok().and_then(|c|c.get(visible_player.index)).is_some_and(|c|c.no_mirror);
                let region_state=(size,visible_player.index,self.facing_right && !no_mirror);
                if !self.dragging && ((changed && self.region_elapsed>=0.1) || !self.visible || self.region_state!=Some(region_state)) { if let Some((pixels,dims))=&visible_player.mask { super::native::set_alpha_region(size,pixels,*dims,region_state.2); self.region_state=Some(region_state); self.region_elapsed=0.0; } }
                super::native::ensure_visible(pos,size);
                bubble_pos=Some((pos,size));
            }
            if !self.visible && visible_player.texture.is_some() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true)); self.visible=true; }
            let uv=|index:usize| if self.facing_right && !animation::catalog().ok().and_then(|c|c.get(index)).is_some_and(|c|c.no_mirror) { Rect::from_min_max(Pos2::new(1.0,0.0),Pos2::new(0.0,1.0)) } else { Rect::from_min_max(Pos2::ZERO,Pos2::new(1.0,1.0)) };
            if let Some(texture)=&self.active.texture { ui.painter().image(texture.id(),rect,uv(self.active.index),Color32::WHITE); }
            if let Some(next)=&self.incoming { if let Some(texture)=&next.texture { let a=if self.active.texture.is_some() {(self.fade/0.16).clamp(0.0,1.0)} else {1.0}; ui.painter().image(texture.id(),rect,uv(next.index),Color32::from_white_alpha((a*255.0) as u8)); } }
            ui.ctx().request_repaint_after(Duration::from_millis(20));
        });
        if !hidden { if let Some((text,_))=self.bubble { self.show_bubble(ctx,bubble_pos,text); } }
    }
    fn show_bubble(&self,ctx:&egui::Context,geometry:Option<((i32,i32),(i32,i32))>,text:&str) {
        let scale=ctx.pixels_per_point().max(0.1);
        let pos=geometry.map(|(p,_)|Pos2::new(p.0 as f32/scale,p.1 as f32/scale-58.0)).unwrap_or(Pos2::ZERO);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("human-pet-bubble"),egui::ViewportBuilder::default().with_title("桌宠说话").with_position(pos).with_inner_size([240.0,50.0]).with_decorations(false).with_always_on_top().with_taskbar(false).with_active(false).with_mouse_passthrough(true),|ui,_| { ui.painter().rect_filled(ui.max_rect(),6.0,Color32::from_rgb(32,32,36)); ui.centered_and_justified(|ui| { ui.label(text); }); });
    }
}

pub(crate) fn clip_name(index:usize)-> &'static str {
    const NAMES:&[&str]=&["待机呼吸","东张西望","拖拽","开心跃动","害羞惊讶","傲娇生气","挠痒笑","挥手","螃蟹走路","漂浮踏步","转身奔跑","哼歌","伸懒腰","打哈欠","刷牙","玩魔方","玩汽车","吐泡泡","玩水枪","吃白饭","吃冰淇淋","涮火锅","落叶","堆雪人","放烟花","吃什么","深度思考","擦桌说话","发呆说话","对屏说话","入睡","睡眠","醒来"];
    NAMES.get(index).copied().unwrap_or("未知动作")
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::animation::{self, Decoder, Message};
    #[test]
    fn bundled_manifest_has_timed_clips_and_mirror_restrictions() {
        let clips=animation::catalog().expect("valid bundled manifest");
        assert_eq!(clips.len(),33);
        assert!(clips.iter().all(|c|c.duration>0.0));
        assert!(clips[25].no_mirror && clips[26].no_mirror);
        assert!(!clips[0].no_mirror);
    }
    #[test]
    fn idle_decoder_delivers_a_transparent_timed_frame() {
        let decoder=Decoder::start(0).expect("start decoder");
        match decoder.frames.recv_timeout(Duration::from_secs(15)).expect("decoded frame") {
            Message::Frame(frame) => {
                assert_eq!(frame.size,[640,360]);
                assert!(frame.duration>Duration::ZERO);
                assert!(frame.pixels.chunks_exact(4).any(|p|p[3]==0));
                assert!(frame.pixels.chunks_exact(4).any(|p|p[3]>0));
            }
            _ => panic!("expected a decoded frame"),
        }
    }
    #[test]
    fn every_bundled_animation_has_transparency_and_a_positive_frame_delay() {
        use image::{AnimationDecoder,codecs::webp::WebPDecoder};
        for (index,bytes) in animation::CLIPS.iter().enumerate() {
            let decoder=WebPDecoder::new(std::io::Cursor::new(*bytes)).expect("open WebP");
            let frame=decoder.into_frames().next().expect("first frame").expect("decode frame");
            assert_eq!(frame.buffer().dimensions(),(640,360),"clip {index}");
            assert!(frame.delay().numer_denom_ms().0>0,"clip {index}");
            assert!(frame.buffer().pixels().any(|p|p.0[3]<255),"clip {index}");
        }
    }
    #[test]
    fn sleep_transitions_to_loop_and_wake_returns_to_idle() {
        let mut pet=Human::default(); pet.mode=Mode::SleepStart; pet.next();
        assert!(pet.mode==Mode::Sleep); assert_eq!(pet.target,31);
        pet.mode=Mode::Wake; pet.next();
        assert!(pet.mode==Mode::Normal); assert_eq!(pet.target,0);
    }
    #[test]
    fn failed_actions_are_not_selected() {
        let mut pet=Human::default(); pet.failed=vec![3,4,5,6];
        for _ in 0..20 { assert_eq!(pet.pick(&[3,4,5,6,7]),7); }
    }
    #[test]
    fn whisper_timer_pauses_during_sleep_and_without_motion() {
        let mut pet=Human::default(); pet.whisper_clock=299.0;
        pet.mode=Mode::Sleep;
        assert!(!pet.whisper_due(10.0,true)); assert_eq!(pet.whisper_clock,299.0);
        pet.mode=Mode::Normal;
        assert!(!pet.whisper_due(10.0,false)); assert_eq!(pet.whisper_clock,299.0);
        assert!(!pet.whisper_due(0.9,true));
        assert!(pet.whisper_due(0.11,true)); assert_eq!(pet.whisper_clock,0.0);
    }
}
