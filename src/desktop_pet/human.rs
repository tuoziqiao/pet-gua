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
enum Mode { Normal, Drag, Sling, Dizzy, SleepStart, Sleep, Wake, Perch }
impl Default for Mode { fn default() -> Self { Self::Normal } }

#[cfg(any(windows, test))]
fn primary_press_on_pet(ui: &egui::Ui, response: &egui::Response) -> bool {
    ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary)) && response.contains_pointer()
}

#[derive(Clone, Copy)]
pub(crate) enum Command { Sleep, Wake, Recenter }

#[cfg(windows)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum FlightSource { Throw, Slingshot, WindowDrop }
#[cfg(windows)]
impl FlightSource {
    fn enabled(self, settings: &Settings) -> bool {
        match self { Self::Throw => settings.desktop_pet_throw, Self::Slingshot => settings.desktop_pet_slingshot, Self::WindowDrop => true }
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
struct TravelProgress {
    distance: Option<i32>,
    pixels: i32,
    progress: f32,
}

#[cfg(any(windows, test))]
impl TravelProgress {
    fn advance(&mut self, progress: f32, width: i32, right: bool) -> (i32, bool) {
        let distance = *self.distance.get_or_insert_with(|| (width.max(0) as f32 * 0.45) as i32);
        let progress = progress.clamp(self.progress, 1.0);
        let pixels = (distance as f32 * progress) as i32;
        let delta = pixels.saturating_sub(self.pixels);
        let completed = progress >= 1.0 && self.progress < 1.0;
        self.pixels = pixels;
        self.progress = progress;
        (if right { delta } else { -delta }, completed)
    }
}

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
    #[cfg(windows)] trail: super::throw::Trail,
    #[cfg(windows)] slingshot: Option<super::slingshot::Session>,
    #[cfg(windows)] sling_press: Option<(i32,i32)>,
    #[cfg(windows)] last_click: Option<(Instant,(i32,i32))>,
    #[cfg(windows)] flight: Option<super::throw::Flight>,
    #[cfg(windows)] flight_source: Option<FlightSource>,
    #[cfg(windows)] icon_targets: Vec<super::placement::Rect>,
    #[cfg(windows)] icon_hit: Option<((i32,i32), Instant)>,
    #[cfg(windows)] icon_hit_window: Option<super::guide::Window>,
    #[cfg(windows)] perch: Option<super::window_perch::Perch>,
    #[cfg(windows)] drag_source: Option<FlightSource>,
    #[cfg(windows)] drag_effect_enabled: bool,
    #[cfg(windows)] drag_started_sleeping: bool,
    #[cfg(windows)] guide: Option<super::guide::Window>,
    #[cfg(windows)] guide_front: Option<super::guide::Window>,
    last_frame: Option<Instant>,
    seed: u64,
    facing_right: bool,
    whisper_clock: f32,
    bubble: Option<(&'static str, f32)>,
    fade: f32,
    target: usize,
    #[cfg(windows)] travel: TravelProgress,
    #[cfg(windows)] navigator: super::placement::Navigator,
    #[cfg(windows)] screens: Option<Vec<super::placement::Screen>>,
    #[cfg(windows)] screen_revision: u64,
    #[cfg(windows)] screen_refresh: Option<Instant>,
    #[cfg(windows)] last_move_size: Option<(i32, i32)>,
    #[cfg(windows)] teleported_at: Option<Instant>,
    #[cfg(windows)] position_dirty: bool,
    elapsed: f32,
    failed: Vec<usize>,
    error: Option<String>,
    animations_enabled: bool,
    work_active: bool,
    availability_revision: u64,
    region_state: Option<((i32,i32),usize,bool)>,
    region_elapsed: f32,
}

impl Human {
    pub(super) fn error_message(&self) -> Option<String> { self.error.clone() }
    #[cfg(windows)]
    fn completed_double_click(&mut self, now:Instant, cursor:(i32,i32)) -> bool {
        let (interval,width,height)=super::native::double_click_limits();
        let double=self.last_click.is_some_and(|(last,point)| now.saturating_duration_since(last)<=interval
            && (i64::from(cursor.0)-i64::from(point.0)).abs()<=i64::from(width)/2
            && (i64::from(cursor.1)-i64::from(point.1)).abs()<=i64::from(height)/2);
        self.last_click=if double {None} else {Some((now,cursor))};
        double
    }
    #[cfg(windows)]
    fn cancel_slingshot(&mut self) {
        self.guide=None; self.guide_front=None; self.sling_press=None; self.last_click=None;
        self.icon_targets.clear();
        if let Some(session)=self.slingshot.take() {
            super::native::move_to(session.original);
            if session.revision!=super::native::display_revision() {
                if let Some((pos,size))=super::native::window_geometry() { super::native::ensure_visible(pos,size); }
            }
            if self.drag_source==Some(FlightSource::Slingshot) { self.dragging=false; self.drag_source=None; self.drag_effect_enabled=false; }
            self.mode=Mode::Normal;
        }
    }
    #[cfg(windows)]
    fn reset_travel(&mut self) {
        self.travel = TravelProgress::default();
        self.navigator.reset();
        self.last_move_size = None;
        self.teleported_at = None;
    }
    #[cfg(windows)]
    fn save_position(&mut self, settings: &Settings) {
        if self.position_dirty { settings.save(); self.position_dirty = false; }
    }
    #[cfg(windows)]
    fn stop_flight(&mut self, settings: &mut Settings, make_visible: bool) {
        self.flight_source=None;
        self.icon_targets.clear();
        if make_visible { self.icon_hit=None; self.icon_hit_window=None; }
        if self.flight.take().is_some() {
            if let Some((pos,size))=super::native::window_geometry() {
                if make_visible { super::native::force_visible(pos,size); }
                if let Some((pos,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(pos); self.position_dirty=true; }
            }
            self.save_position(settings);
        }
    }
    #[cfg(windows)]
    fn leave_perch(&mut self, settings: &mut Settings) {
        if self.perch.take().is_some() {
            if self.mode==Mode::Perch { self.mode=Mode::Normal; }
            if let Some((pos,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(pos); self.position_dirty=true; }
            self.save_position(settings);
        }
    }
    #[cfg(windows)]
    fn refresh_screens(&mut self, now: Instant, force: bool) {
        let revision = super::native::display_revision();
        if force || self.screen_revision != revision || self.screen_refresh.is_none_or(|at| now.duration_since(at) >= Duration::from_secs(1)) {
            let screens = super::native::screens();
            if self.screens != screens { self.navigator.reset(); }
            self.screens = screens;
            self.screen_revision = revision;
            self.screen_refresh = Some(now);
        }
    }
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
        let available: Vec<_> = indices.iter().copied().filter(|i| animation::clip_available(*i) && !self.failed.contains(i)).collect();
        if available.is_empty() { return 0; }
        available[self.random(available.len())]
    }
    fn pick_group(&mut self, label:&str, fallback:&[usize]) -> usize {
        self.pick(animation::group(label).unwrap_or(fallback))
    }
    fn choose_normal(&mut self) -> usize {
        if self.work_active { if let Some(i)=animation::group("工作状态").and_then(|g|g.get(1)).copied().filter(|i|animation::clip_available(*i) && !self.failed.contains(i)) { return i; } }
        let groups=[("待机",10usize,&[0][..]),("转向",5,&[1][..]),("移动",5,&[8,9,10][..]),("小动作",20,&[11,12,13,14][..]),("玩耍",20,&[15,16,17,18][..]),("吃什么",16,&[19,20,21][..]),("时节",14,&[22,23,24][..]),("文字",10,&[25,26][..])];
        let choices:Vec<_>=groups.into_iter().filter_map(|(name,default_weight,fallback)| {
            let indices=animation::group(name).unwrap_or(fallback);
            if !indices.iter().any(|i|animation::clip_available(*i) && !self.failed.contains(i)) { return None; }
            let weight=if matches!(name,"待机"|"转向"|"移动") {default_weight} else { let n=animation::weight(name); if n==0 {default_weight} else {n} };
            Some((indices,weight))
        }).collect();
        let total:usize=choices.iter().map(|(_,weight)|*weight).sum();
        if total==0 { return 0; }
        let mut roll=self.random(total);
        for (indices,weight) in choices { if roll<weight { return self.pick(indices); } roll-=weight; }
        0
    }
    fn play(&mut self, index: usize) {
        if !animation::clip_available(index) { return; }
        if self.failed.contains(&index) { return; }
        let mut player=Player::default(); player.play(index);
        self.incoming=Some(player); self.target=index; self.fade=0.0; self.elapsed=0.0;
        self.prepared=None;
        #[cfg(windows)] self.reset_travel();
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
            Mode::Sling => 0,
            Mode::Dizzy => { self.mode=Mode::Normal; 0 },
            Mode::SleepStart => { if animation::clip_available(31) { self.mode=Mode::Sleep; 31 } else { self.mode=Mode::Normal; 0 } },
            Mode::Sleep => if animation::clip_available(31) {31} else {self.mode=Mode::Normal; 0},
            Mode::Wake => { self.mode=Mode::Normal; if self.work_active { self.choose_normal() } else { 0 } },
            Mode::Normal => self.choose_normal(),
            Mode::Perch => 0,
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
                else { animation::invalidate_clip(index); self.mode=Mode::Normal; if index==animation::DIZZY_CLIP { self.play(0); } }
            } else if next.texture.is_some() {
                self.fade+=dt;
                if self.active.texture.is_none() || self.fade>=0.16 {
                    self.active=self.incoming.take().unwrap(); self.fade=0.0; self.elapsed=0.0;
                    #[cfg(windows)] self.reset_travel();
                }
            }
        }
        if self.incoming.is_none() {
            self.elapsed+=dt;
            changed|=self.active.step(ctx,dt);
            if self.active.ended {
                let mut dizzy_failed=false;
                if let Some(error)=self.active.error.take() {
                    self.failed.push(self.active.index); eprintln!("桌宠动画 {}: {error}",self.active.index);
                    if self.active.index==0 { self.error=Some(format!("待机动画不可用：{error}")); }
                    else { dizzy_failed=self.active.index==animation::DIZZY_CLIP; animation::invalidate_clip(self.active.index); self.mode=Mode::Normal; }
                }
                if self.error.is_none() { if dizzy_failed { self.play(0); } else { self.next(); } }
            }
        }
        changed
    }
    fn reset(&mut self) { *self=Self::default(); }
    pub(super) fn show(&mut self, ctx:&egui::Context, settings:&mut Settings, fullscreen:bool, system_animations:bool, focus:bool, focus_complete:bool, event:Option<Event>, preview:Option<usize>, command:Option<Command>) {
        if !settings.desktop_pet {
            #[cfg(windows)]
            {
                self.cancel_slingshot();
                self.leave_perch(settings);
                self.stop_flight(settings, true);
                if self.dragging { if let Some((pos,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(pos); self.position_dirty=true; } }
                self.save_position(settings);
            }
            if self.initialized { self.reset(); } return;
        }
        let hidden=fullscreen && settings.hide_fullscreen;
        let motion=settings.desktop_pet_motion && system_animations;
        let revision=animation::availability_revision();
        if revision!=self.availability_revision { self.failed.clear(); self.prepared=None; self.availability_revision=revision; }
        if let Err(error)=animation::catalog() { self.error=Some(format!("桌宠素材清单错误：{error}")); }
        if self.animations_enabled && !motion && self.initialized { self.mode=Mode::Normal; self.bubble=None; self.play(0); }
        self.animations_enabled=motion;
        let requested=Vec2::new(462.0,260.0)*(settings.desktop_pet_size_pct.clamp(DESKTOP_PET_MIN_SIZE,DESKTOP_PET_MAX_SIZE) as f32/100.0);
        let id=egui::ViewportId::from_hash_of("gua-desktop-pet");
        let mut bubble_pos=None;
        #[cfg(windows)]
        let mut guide=None;
        ctx.show_viewport_immediate(id, egui::ViewportBuilder::default().with_title(TITLE).with_inner_size(requested).with_decorations(false).with_transparent(true).with_resizable(false).with_taskbar(false).with_always_on_top().with_active(false).with_visible(false), |ui,_| {
            let now=Instant::now();
            if hidden {
                if self.visible { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false)); self.visible=false; }
                self.last_frame=None;
                #[cfg(windows)]
                {
                    self.leave_perch(settings);
                    self.stop_flight(settings, true);
                    self.cancel_slingshot();
                    if self.dragging { if let Some((pos,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(pos); self.position_dirty=true; } }
                    self.save_position(settings);
                }
                self.dragging=false; self.work_active=false; return;
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
            #[cfg(windows)]
            if !motion || command.is_some() || preview.is_some() || !settings.desktop_pet_slingshot {
                if self.slingshot.is_some() { self.cancel_slingshot(); if motion { self.play(0); } }
                self.last_click=None;
                if !motion || command.is_some() { self.stop_flight(settings, true); }
            }
            #[cfg(windows)]
            if !motion || !settings.desktop_pet_window_perch || command.is_some() || preview.is_some() { self.leave_perch(settings); }
            #[cfg(windows)]
            {
                if self.flight_source.is_some_and(|source| !source.enabled(settings)) { self.stop_flight(settings,false); }
                if self.dragging && self.drag_source.is_some_and(|source| !source.enabled(settings)) { self.drag_effect_enabled=false; }
            }
            self.region_elapsed+=frame_dt;
            #[cfg(windows)]
            super::native::ensure_frameless();
            if !self.active.is_started() && self.incoming.is_none() && self.error.is_none() { self.play(0); }
            match command {
                Some(Command::Sleep) if motion && animation::clip_available(31) && !matches!(self.mode,Mode::Sleep|Mode::SleepStart) => { self.bubble=None; if animation::clip_available(30) { self.mode=Mode::SleepStart; self.play(30); } else { self.mode=Mode::Sleep; self.play(31); } }
                Some(Command::Wake) if motion && matches!(self.mode,Mode::Sleep|Mode::SleepStart) => { if animation::clip_available(32) { self.mode=Mode::Wake; self.play(32); } else { self.mode=Mode::Normal; self.play(0); } }
                Some(Command::Recenter) => {
                    #[cfg(windows)]
                    if let Some((pos,size))=super::native::window_geometry() { super::native::force_visible(pos,size); if let Some((p,_))=super::native::window_geometry() { settings.desktop_pet_position=Some(p); settings.save(); } }
                }
                _ => {}
            }
            let entered_focus=focus && !self.work_active;
            if focus!=self.work_active { self.prepared=None; }
            self.work_active=focus;
            if let Some(index)=preview { if animation::clip_available(index) && !self.failed.contains(&index) && motion { self.mode=if index==animation::DIZZY_CLIP { Mode::Dizzy } else { Mode::Normal }; self.play(index); } }
            if preview.is_none() && self.mode==Mode::Normal && motion && !self.dragging {
                if focus_complete {
                    if let Some(index)=animation::group("工作状态").and_then(|g|g.get(4)).copied().filter(|i|animation::clip_available(*i)) { self.play(index); }
                } else if event.is_some() { let index=self.pick_group("点击回应",&[3,4,5,6,7]); if index!=0 { self.play(index); } }
                else if entered_focus { if let Some(index)=animation::group("工作状态").and_then(|g|g.get(1)).copied().filter(|i|animation::clip_available(*i)) { self.play(index); } }
            }
            let rect=ui.max_rect();
            let response=ui.interact(rect,ui.scope_id().with("human"),Sense::click_and_drag());
            #[cfg(windows)]
            let mut geometry=super::native::window_geometry();
            #[cfg(windows)]
            let cursor=super::native::cursor_screen();
            #[cfg(windows)]
            if self.slingshot.is_some() {
                let old_screens=self.screens.clone();
                self.refresh_screens(now,false);
                let invalid=self.slingshot.as_ref().is_some_and(|s|s.revision!=super::native::display_revision()
                    || geometry.is_none_or(|(_,size)|size!=s.size)) || old_screens!=self.screens || self.screens.is_none();
                if invalid {
                    let original=self.slingshot.as_ref().map(|s|s.original);
                    self.cancel_slingshot();
                    if let Some((pos,size))=super::native::window_geometry() { super::native::ensure_visible(pos,size); }
                    geometry=super::native::window_geometry();
                    if let Some((pos,_))=geometry { if original.is_some_and(|p|p!=pos) { settings.desktop_pet_position=Some(pos); settings.save(); } }
                    if motion { self.play(0); }
                }
                else if !self.dragging { self.mode=Mode::Sling; }
            }
            #[cfg(windows)]
            if primary_press_on_pet(ui,&response) {
                self.stop_flight(settings, false);
                self.reset_travel();
                self.sling_press=if self.slingshot.as_ref().is_some_and(|s|s.is_ready()) { cursor } else { None };
            }
            #[cfg(windows)]
            if response.drag_started() {
                if let (Some((pos,_)),Some(cursor))=(geometry,cursor) {
                    self.last_click=None;
                    if let Some(sling)=self.slingshot.as_mut() {
                        if sling.is_ready() {
                            sling.start_pull(self.sling_press.unwrap_or(cursor));
                            self.dragging=true; self.drag_source=Some(FlightSource::Slingshot); self.drag_effect_enabled=true;
                            if motion && animation::clip_available(2) { self.mode=Mode::Drag; self.play(2); }
                        }
                    } else {
                        self.drag_started_sleeping=matches!(self.mode,Mode::Sleep|Mode::SleepStart);
                        self.leave_perch(settings);
                        self.stop_flight(settings, false);
                        self.drag_source=Some(FlightSource::Throw);
                        self.drag_effect_enabled=settings.desktop_pet_throw;
                        self.trail=super::throw::Trail::default();
                        self.trail.push(now,cursor);
                        self.dragging=true; self.drag_offset=(cursor.0-pos.0,cursor.1-pos.1);
                        self.save_position(settings);
                        self.reset_travel();
                        if motion && animation::clip_available(2) { self.mode=Mode::Drag; self.play(2); }
                    }
                }
            }
            #[cfg(not(windows))]
            if response.drag_started() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag); }
            #[cfg(windows)]
            if self.dragging {
                if self.drag_source==Some(FlightSource::Slingshot) {
                    if super::native::left_button_down() {
                        if let (Some(sling),Some(cursor))=(self.slingshot.as_mut(),cursor) {
                            let target=sling.update_pull(cursor);
                            if !super::native::move_to(target) { self.cancel_slingshot(); }
                            geometry=super::native::window_geometry();
                        }
                    } else {
                        self.dragging=false; self.drag_source=None; self.drag_effect_enabled=false; self.sling_press=None;
                        let mut move_failed=false;
                        if let (Some(sling),Some(cursor))=(self.slingshot.as_mut(),cursor) {
                            let target=sling.update_pull(cursor);
                            if super::native::move_to(target) { geometry=super::native::window_geometry(); } else { move_failed=true; }
                        }
                        if move_failed { self.cancel_slingshot(); geometry=super::native::window_geometry(); }
                        let launch=self.slingshot.as_ref().and_then(|s|s.velocity().map(|v|(v,s.body,s.size)));
                        if let Some((velocity,body,size))=launch {
                            let old_screens=self.screens.clone();
                            self.refresh_screens(now,true);
                            let screens_valid=old_screens==self.screens && self.slingshot.as_ref().is_some_and(|s|s.revision==super::native::display_revision());
                            if let (Some((pos,actual_size)),Some(screens))=(geometry,self.screens.as_ref()) {
                                if screens_valid && !screens.is_empty() && actual_size==size {
                                    self.icon_targets=if settings.desktop_pet_slingshot_icons {
                                        let center=(pos.0 as f64+(body.left+body.right)*0.5,pos.1 as f64+(body.top+body.bottom)*0.5);
                                        screens.iter().find(|s| center.0>=s.bounds.left as f64 && center.0<s.bounds.right as f64 && center.1>=s.bounds.top as f64 && center.1<s.bounds.bottom as f64).map_or_else(Vec::new,|s|super::desktop_icons::snapshot(*s))
                                    } else { Vec::new() };
                                    self.flight=Some(super::throw::Flight::new(pos,size,body,velocity));
                                    self.flight_source=Some(FlightSource::Slingshot);
                                    self.slingshot=None; self.guide=None; self.guide_front=None; self.last_click=None;
                                    self.mode=Mode::Normal;
                                }
                            }
                            if self.flight.is_none() {
                                let original=self.slingshot.as_ref().map(|s|s.original);
                                self.cancel_slingshot();
                                if !screens_valid {
                                    if let Some((pos,size))=super::native::window_geometry() { super::native::ensure_visible(pos,size); }
                                    geometry=super::native::window_geometry();
                                    if let Some((pos,_))=geometry { if original.is_some_and(|p|p!=pos) { settings.desktop_pet_position=Some(pos); settings.save(); } }
                                }
                            }
                        } else if let Some(sling)=self.slingshot.as_mut() {
                            sling.start_return(now); self.mode=Mode::Sling;
                        }
                        if motion { self.play(0); }
                    }
                } else if super::native::left_button_down() {
                    if let Some(cursor)=cursor { self.trail.push(now,cursor); super::native::move_to((cursor.0-self.drag_offset.0,cursor.1-self.drag_offset.1)); geometry=super::native::window_geometry(); }
                } else {
                    self.dragging=false; self.mode=Mode::Normal;
                    if let Some(cursor)=cursor { self.trail.push(now,cursor); }
                    let slow=self.trail.velocity(now).is_none();
                    if let Some((pos,size))=geometry {
                        if motion && self.drag_effect_enabled {
                            let source=self.drag_source.unwrap_or(FlightSource::Throw);
                            let velocity=match source { FlightSource::Throw => self.trail.velocity(now), _ => None };
                            if let Some(velocity)=velocity {
                                self.refresh_screens(now,true);
                                if self.screens.is_some() {
                                    let player=self.incoming.as_ref().filter(|p|p.texture.is_some()).unwrap_or(&self.active);
                                    let mirror=self.facing_right && !animation::clip_info(player.index).is_some_and(|c|c.no_mirror);
                                    let body=super::throw::Body::from_mask(size,player.mask.as_ref().map(|(p,d)|(p.as_slice(),*d)),mirror);
                                    self.flight=Some(super::throw::Flight::new(pos,size,body,velocity));
                                    self.flight_source=Some(source);
                                }
                            }
                        }
                        if self.flight.is_none() && slow && motion && settings.desktop_pet_window_perch {
                            self.refresh_screens(now,true);
                            if let Some(screens)=&self.screens {
                                let player=self.incoming.as_ref().filter(|p|p.texture.is_some()).unwrap_or(&self.active);
                                let mirror=self.facing_right && !animation::clip_info(player.index).is_some_and(|c|c.no_mirror);
                                let body=super::throw::Body::from_mask(size,player.mask.as_ref().map(|(p,d)|(p.as_slice(),*d)),mirror);
                                if let Some((perch,target))=super::window_perch::Perch::find(pos,size,body,screens,now) {
                                    if super::native::move_to(target) {
                                        self.perch=Some(perch); self.mode=Mode::Perch;
                                        geometry=super::native::window_geometry();
                                    }
                                }
                            }
                        }
                        if self.flight.is_none() { settings.desktop_pet_position=Some(geometry.map_or(pos,|(p,_)|p)); settings.save(); }
                    }
                    self.drag_source=None;
                    self.drag_effect_enabled=false;
                    if motion { self.play(0); }
                }
            }
            #[cfg(windows)]
            if let Some(sling)=self.slingshot.as_mut() {
                if let Some(target)=sling.update_return(now) {
                    if !super::native::move_to(target) { self.cancel_slingshot(); }
                    geometry=super::native::window_geometry();
                }
            }
            #[cfg(windows)]
            if self.perch.is_some() {
                self.refresh_screens(now,false);
                let update=if let (Some(perch),Some(screens),Some((_,size)))=(&mut self.perch,&self.screens,geometry) {
                    let idle=self.incoming.is_none() && self.active.index==0;
                    let mask=if idle { self.active.mask.as_ref().map(|(p,d)|(p.as_slice(),*d)) } else { None };
                    let mirror=self.facing_right && !animation::clip_info(0).is_some_and(|c|c.no_mirror);
                    perch.update_body(size,mask,mirror);
                    perch.update(now,screens)
                } else { super::window_perch::Update::Drop };
                match update {
                    super::window_perch::Update::Follow(target) => {
                        if geometry.is_none_or(|(pos,_)|pos!=target) && !super::native::move_to(target) { self.leave_perch(settings); }
                        geometry=super::native::window_geometry();
                    }
                    super::window_perch::Update::Drop => {
                        let perch=self.perch.take();
                        self.mode=Mode::Normal;
                        if let (Some(perch),Some((pos,size)))=(perch,geometry) {
                            if self.screens.is_some() {
                                self.flight=Some(super::throw::Flight::new(pos,size,perch.body(),(0.0,0.0)));
                                self.flight_source=Some(FlightSource::WindowDrop);
                            } else { settings.desktop_pet_position=Some(pos); settings.save(); }
                        }
                    }
                }
            }
            #[cfg(windows)]
            if self.flight.is_some() {
                let old_screens=self.screens.clone();
                self.refresh_screens(now,false);
                let geometry_valid=geometry.is_some_and(|(pos,size)|self.flight.as_mut().is_some_and(|f|f.sync_geometry(pos,size)));
                if !geometry_valid || old_screens!=self.screens || self.screens.is_none() {
                    self.stop_flight(settings,true);
                    geometry=super::native::window_geometry();
                } else if let (Some(flight),Some(screens))=(&mut self.flight,&self.screens) {
                    let mut hits=Vec::new();
                    let targets=if settings.desktop_pet_slingshot_icons && self.flight_source==Some(FlightSource::Slingshot) { &self.icon_targets[..] } else { &[] };
                    let step=flight.step_with_targets(frame_dt as f64,screens,targets,super::window_perch::desktop_point_visible,&mut hits);
                    if let Some(point)=hits.last() { self.icon_hit=Some((*point,now)); }
                    let done=step.unwrap_or(true);
                    let target=flight.position();
                    let dizzy=step==Some(true) && self.flight_source==Some(FlightSource::Throw) && !self.drag_started_sleeping
                        && flight.peak_landing_impact().is_some_and(|impact|impact>=settings.desktop_pet_dizzy_impact_speed as f64)
                        && motion && !self.failed.contains(&animation::DIZZY_CLIP);
                    if !super::native::move_to(target) { self.stop_flight(settings,true); }
                    else if done { self.stop_flight(settings,false); if dizzy { self.mode=Mode::Dizzy; self.play(animation::DIZZY_CLIP); } }
                    geometry=super::native::window_geometry();
                }
            }
            if response.clicked() && !self.dragging {
                #[cfg(windows)]
                let mut sling_handled=false;
                #[cfg(windows)]
                if motion && preview.is_none() && command.is_none() && settings.desktop_pet_slingshot && cursor.is_some_and(|point|self.completed_double_click(now,point)) {
                    sling_handled=true;
                    if self.slingshot.is_some() {
                        self.cancel_slingshot(); geometry=super::native::window_geometry(); self.play(0);
                    } else {
                        self.leave_perch(settings);
                        self.refresh_screens(now,true);
                        if let (Some((pos,size)),Some(screens))=(geometry,self.screens.as_deref()) {
                            let player=self.incoming.as_ref().filter(|p|p.texture.is_some()).unwrap_or(&self.active);
                            let mirror=self.facing_right && !animation::clip_info(player.index).is_some_and(|c|c.no_mirror);
                            let body=super::throw::Body::from_mask(size,player.mask.as_ref().map(|(p,d)|(p.as_slice(),*d)),mirror);
                            let scale=ui.ctx().pixels_per_point() as f64;
                            if let (Some(session),true,Some(back),Some(front))=(super::slingshot::Session::new(pos,size,body,scale,screens,super::native::display_revision()),super::guide::available(),super::guide::Window::new(),super::guide::Window::new()) {
                                if super::native::move_to(session.origin) && super::native::window_geometry()==Some((session.origin,size)) {
                                    self.icon_targets=if settings.desktop_pet_slingshot_icons {
                                        let center=(session.origin.0 as f64+(body.left+body.right)*0.5,session.origin.1 as f64+(body.top+body.bottom)*0.5);
                                        screens.iter().find(|s|center.0>=s.bounds.left as f64 && center.0<s.bounds.right as f64 && center.1>=s.bounds.top as f64 && center.1<s.bounds.bottom as f64).map_or_else(Vec::new,|s|super::desktop_icons::snapshot(*s))
                                    } else { Vec::new() };
                                    self.save_position(settings);
                                    self.guide=Some(back); self.guide_front=Some(front); self.slingshot=Some(session);
                                    self.bubble=None; self.mode=Mode::Sling; self.play(0); geometry=super::native::window_geometry();
                                } else { super::native::move_to(pos); }
                            }
                        }
                    }
                }
                #[cfg(not(windows))]
                let sling_handled=false;
                #[cfg(windows)]
                let sling_active=self.slingshot.is_some();
                #[cfg(not(windows))]
                let sling_active=false;
                if !sling_handled && !sling_active {
                    if matches!(self.mode,Mode::Sleep|Mode::SleepStart) { if animation::clip_available(32) { self.mode=Mode::Wake; self.play(32); } else { self.mode=Mode::Normal; self.play(0); } }
                    else if motion { let index=self.pick_group("点击回应",&[3,4,5,6,7]); if index!=0 { self.play(index); } }
                }
            }
            if preview.is_none() && command.is_none() && event.is_none() && !focus_complete && !entered_focus && !self.dragging && !response.clicked() && self.whisper_due(dt,motion) {
                let text=WHISPERS[self.random(WHISPERS.len())]; self.bubble=Some((text,5.0));
                let index=self.pick_group("碎碎念",&[27,28,29]); if index!=0 { self.play(index); }
            }
            if let Some((_,remaining))=&mut self.bubble { *remaining-=dt; if *remaining<=0.0 { self.bubble=None; } }
            let changed=if motion || self.incoming.is_some() || self.active.texture.is_none() { self.animate(ui.ctx(),frame_dt) } else { false };
            if motion && self.mode==Mode::Normal && self.incoming.is_none() && self.prepared.is_none() && self.active.texture.is_some() {
                if let Some(clip)=animation::clip_info(self.active.index) { if self.elapsed >= (clip.duration-0.7).max(0.0) {
                    let index=self.choose_normal();
                    let mut player=Player::default(); player.play(index); self.prepared=Some(player);
                } }
            }
            if let Some(prepared)=&mut self.prepared { if prepared.texture.is_none() { prepared.step(ui.ctx(),0.0); } }
            #[cfg(windows)]
            let mut navigating = false;
            #[cfg(windows)]
            if motion && self.flight.is_none() && self.slingshot.is_none() && !self.dragging && !response.is_pointer_button_down_on() && self.mode==Mode::Normal && self.incoming.is_none() && matches!(self.active.index,8|9|10) {
                if let (Some((pos,size)),Some(clip))=(geometry,animation::clip_info(self.active.index)) {
                    let duration=clip.duration;
                    let movement=animation::movement(self.active.index);
                    let lead=movement.map_or(if self.active.index==10 {1.75} else {2.0},|m|m.lead);
                    let tail=movement.map_or(if self.active.index==10 {4.8} else {2.0},|m|m.tail);
                    let progress=((self.elapsed-lead)/(duration-lead-tail).max(0.5)).clamp(0.0,1.0);
                    let starting = self.travel.distance.is_none();
                    if starting { if let Some(m)=movement { let range=m.max_dist.saturating_sub(m.min_dist) as usize+1; let distance=m.min_dist as usize+self.random(range); self.travel.distance=Some(((distance as f32)*(size.0.max(0) as f32)/462.0) as i32); } }
                    self.refresh_screens(now, starting);
                    let (delta, completed) = self.travel.advance(progress,size.0,self.facing_right);
                    let size_changed = self.last_move_size.is_some_and(|previous| previous != size);
                    let mut crossed = false;
                    if let Some(screens) = &self.screens {
                        if let Some(step) = self.navigator.advance(screens,pos,size,delta) {
                            navigating = true;
                            if step.teleported { self.teleported_at = Some(now); }
                            let fit_x = step.teleported || (size_changed && self.teleported_at.is_some_and(|at| now.duration_since(at) < Duration::from_secs(1)));
                            if step.position == pos || super::native::move_to(step.position) {
                                geometry = super::native::window_geometry();
                                if let Some((actual,actual_size)) = geometry {
                                    let fit_x = fit_x || (step.teleported && actual_size != size);
                                    let corrected = self.navigator.fit_after_resize(screens,actual,actual_size,fit_x);
                                    if corrected != actual && super::native::move_to(corrected) { geometry = super::native::window_geometry(); }
                                    crossed = step.crossed;
                                }
                            } else {
                                navigating = false;
                                self.navigator.reset();
                            }
                        }
                    }
                    if !navigating {
                        super::native::move_clamped((pos.0.saturating_add(delta),pos.1),size);
                        geometry=super::native::window_geometry();
                    }
                    if let Some((actual,actual_size))=geometry {
                        self.last_move_size=Some(actual_size);
                        if actual!=pos { settings.desktop_pet_position=Some(actual); self.position_dirty=true; }
                        if crossed || completed { self.save_position(settings); }
                    }
                }
            }
            #[cfg(windows)]
            if !navigating { self.save_position(settings); }
            #[cfg(windows)]
            let guiding = self.slingshot.is_some();
            let visible_player=self.incoming.as_ref().filter(|p|p.texture.is_some()).unwrap_or(&self.active);
            #[cfg(windows)]
            if let Some((pos,size))=geometry {
                let no_mirror=animation::clip_info(visible_player.index).is_some_and(|c|c.no_mirror);
                let region_state=(size,visible_player.index,self.facing_right && !no_mirror);
                if !self.dragging && ((changed && self.region_elapsed>=0.1) || !self.visible || self.region_state!=Some(region_state)) { if let Some((pixels,dims))=&visible_player.mask { super::native::set_alpha_region(size,pixels,*dims,region_state.2); self.region_state=Some(region_state); self.region_elapsed=0.0; } }
                if !navigating && !guiding { super::native::ensure_visible(pos,size); }
                bubble_pos=Some((pos,size));
                if let Some(sling)=self.slingshot.as_ref() {
                    let dots=if sling.is_pulling() {
                        sling.velocity().zip(self.screens.as_deref()).map_or_else(Vec::new,|(velocity,screens)|super::throw::foot_guide(pos,size,sling.body,velocity,screens,sling.scale,if settings.desktop_pet_slingshot_icons {&self.icon_targets} else {&[]}))
                    } else { Vec::new() };
                    let screens=self.screens.as_deref().unwrap_or(&[]).iter().map(|screen|screen.bounds).collect();
                    guide=Some(super::guide::Scene {base:sling.base(),pouch:sling.pouch(),unit:sling.unit,scale:sling.scale,dots,screens});
                }
            }
            if !self.visible && visible_player.texture.is_some() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true)); self.visible=true; }
            let uv=|index:usize| if self.facing_right && !animation::clip_info(index).is_some_and(|c|c.no_mirror) { Rect::from_min_max(Pos2::new(1.0,0.0),Pos2::new(0.0,1.0)) } else { Rect::from_min_max(Pos2::ZERO,Pos2::new(1.0,1.0)) };
            if let Some(texture)=&self.active.texture { ui.painter().image(texture.id(),rect,uv(self.active.index),Color32::WHITE); }
            if let Some(next)=&self.incoming { if let Some(texture)=&next.texture { let a=if self.active.texture.is_some() {(self.fade/0.16).clamp(0.0,1.0)} else {1.0}; ui.painter().image(texture.id(),rect,uv(next.index),Color32::from_white_alpha((a*255.0) as u8)); } }
            ui.ctx().request_repaint_after(Duration::from_millis(20));
        });
        #[cfg(windows)]
        if let Some(scene)=guide {
            let back=self.guide.as_mut().is_some_and(|window|window.show(&scene,false));
            let front=self.guide_front.as_mut().is_some_and(|window|window.show(&scene,true));
            if !back || !front { self.cancel_slingshot(); if motion { self.play(0); } ctx.request_repaint(); }
        } else if self.slingshot.is_some() { self.cancel_slingshot(); if motion { self.play(0); } }
        else { self.guide=None; self.guide_front=None; }
        #[cfg(windows)]
        if let Some((point,started))=self.icon_hit {
            let progress=started.elapsed().as_secs_f64()/0.6;
            if progress>=1.0 || hidden || !settings.desktop_pet_slingshot_icons { self.icon_hit=None; self.icon_hit_window=None; }
            else {
                if self.icon_hit_window.is_none() { self.icon_hit_window=super::guide::Window::new(); }
                if self.icon_hit_window.as_mut().is_some_and(|window|window.show_hit(point,progress)) { ctx.request_repaint_after(Duration::from_millis(20)); }
                else { self.icon_hit=None; self.icon_hit_window=None; }
            }
        } else { self.icon_hit_window=None; }
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

pub(crate) fn preview_categories() -> Vec<(&'static str,Vec<(usize,String)>)> {
    let labels=["待机","转向","拖拽","点击回应","移动","小动作","玩耍","吃什么","时节","文字","余额档位","碎碎念","工作状态","睡觉","玩法"];
    let mut categories: Vec<_>=labels.into_iter().filter_map(|label| {
        let legacy=match label { "吃什么"=>"吃东西", "碎碎念"=>"说话", _=>label };
        let indices=animation::group(label).or_else(||CATEGORIES.iter().find(|(name,_)|*name==legacy).map(|(_,indices)|*indices))?;
        let entries:Vec<_>=indices.iter().filter_map(|&i|animation::clip_info(i).map(|c|(i,c.name.clone()))).collect();
        if entries.is_empty() { None } else { Some((label,entries)) }
    }).collect();
    categories.push(("晕眩",vec![(animation::DIZZY_CLIP,"甩出落地晕眩".into())]));
    categories
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::animation::{self, Decoder, Message};
    #[test]
    fn pet_press_is_detected_before_drag_threshold() {
        use crate::headless::ContextDiscardExt;
        let ctx=egui::Context::default();
        let point=Pos2::new(50.0,50.0);
        let rect=Rect::from_min_size(Pos2::ZERO,Vec2::splat(100.0));
        let raw=|events| egui::RawInput { screen_rect:Some(Rect::from_min_size(Pos2::ZERO,Vec2::splat(200.0))), events, ..Default::default() };
        let mut pressed=false;
        ctx.run_ui_discard(raw(vec![egui::Event::PointerMoved(point)]),|ui| { ui.interact(rect,egui::Id::new("pet-press"),Sense::click_and_drag()); });
        ctx.run_ui_discard(raw(vec![egui::Event::PointerButton {pos:point,button:egui::PointerButton::Primary,pressed:true,modifiers:egui::Modifiers::NONE}]),|ui| {
            let response=ui.interact(rect,egui::Id::new("pet-press"),Sense::click_and_drag());
            pressed=primary_press_on_pet(ui,&response);
            assert!(!response.drag_started());
        });
        assert!(pressed, "the initial press must be captured before dragging starts");
    }
    #[test]
    fn travel_progress_is_incremental_through_resize_and_interruption() {
        let mut travel=TravelProgress::default();
        assert_eq!(travel.advance(0.5,200,true),(45,false));
        assert_eq!(travel.advance(0.5,200,true),(0,false)); // Pausing must not replay movement.
        assert_eq!(travel.advance(0.75,400,true),(22,false));
        assert_eq!(travel.advance(1.0,400,true),(23,true));
        assert_eq!(travel.advance(1.0,400,true),(0,false));
        travel=TravelProgress::default(); // A new animation starts from the actual window position.
        assert_eq!(travel.advance(0.5,200,false),(-45,false));
        assert_eq!(travel.advance(0.25,200,false),(0,false));
    }
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
        if animation::clip_available(31) { assert!(pet.mode==Mode::Sleep); assert_eq!(pet.target,31); }
        else { assert!(pet.mode==Mode::Normal); assert_eq!(pet.target,0); }
        pet.mode=Mode::Wake; pet.next();
        assert!(pet.mode==Mode::Normal); assert_eq!(pet.target,0);
    }
    #[test]
    fn failed_actions_are_not_selected() {
        let mut pet=Human::default(); pet.failed=vec![3,4,5,6];
        for _ in 0..20 { assert_eq!(pet.pick(&[3,4,5,6,7]),if animation::clip_available(7) {7} else {0}); }
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
