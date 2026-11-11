//! Independent desktop companion. Geometry and expressions are independent of window handling.
//! Mochi outline and capsule proportions adapted from Aora Bot, copyright 2026 sam70361.
//! See docs/AORA-BOT-LICENSE.txt and README for the pinned upstream revision.
use crate::model::{DesktopPetColor, Settings, DESKTOP_PET_MAX_SIZE, DESKTOP_PET_MIN_SIZE};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::time::{Duration, Instant};
pub(crate) mod expressions;
pub(crate) mod motion;
use expressions::{Animator, Context, Effect, Mouth, Pose};
use motion::{BodyPose, Motion, MotionKind};
pub(crate) use expressions::{Event, Preview};

const SIZE: Vec2 = Vec2::new(160.0, 140.0);
const TITLE: &str = "瓜瓜桌宠 · Mochi";

// Upstream mochi: M50 26 C74 26 88 44 88 63 C88 80 73 88 50 88
// C27 88 12 80 12 63 C12 44 26 26 50 26 Z.
fn outline() -> Vec<Pos2> {
    let curves = [([50.,26.],[74.,26.],[88.,44.],[88.,63.]), ([88.,63.],[88.,80.],[73.,88.],[50.,88.]), ([50.,88.],[27.,88.],[12.,80.],[12.,63.]), ([12.,63.],[12.,44.],[26.,26.],[50.,26.])];
    let mut result = Vec::with_capacity(64);
    for (a,b,c,d) in curves { for i in 0..16 { let t=i as f32 / 16.0; let u=1.0-t; let f=|n| u*u*u*a[n]+3.0*u*u*t*b[n]+3.0*u*t*t*c[n]+t*t*t*d[n]; result.push(egui::pos2(8.0+(f(0)-12.0)*144.0/76.0, 8.0+(f(1)-26.0)*124.0/62.0)); } }
    result
}

fn paint(ui: &egui::Ui, rect: Rect, pose: Pose, body: BodyPose, color: DesktopPetColor, time: f32) {
    let p = ui.painter();
    let center = rect.center();
    let scale_x = rect.width() / SIZE.x;
    let scale_y = rect.height() / SIZE.y;
    let scale = scale_x.min(scale_y);
    let map = |v: Pos2| { let v=body.map(v); egui::pos2(rect.left()+v.x*scale_x, center.y+(v.y-SIZE.y/2.0)*scale_y) };
    let [r, g, b] = color.rgb();
    p.add(egui::Shape::convex_polygon(outline().into_iter().map(map).collect(), Color32::from_rgb(r, g, b), Stroke::NONE));
    let ink=Color32::from_rgb(42,35,32);
    let disk = |x:f32,y:f32,rx:f32,ry:f32,color:Color32| {
        let points=(0..24).map(|i| {let a=i as f32*std::f32::consts::TAU/24.0; map(egui::pos2(x+rx*a.cos(),y+ry*a.sin()))}).collect();
        p.add(egui::Shape::convex_polygon(points,color,Stroke::NONE));
    };
    for (i, x) in [62.0, 98.0].into_iter().enumerate() {
        let height=22.0*pose.eyes[i].max(0.08);
        let width=13.0;
        let radius=height.min(width)/2.0;
        let reach=(height-width).abs()/2.0;
        let tilt=pose.tilt[i];
        let mut points=Vec::with_capacity(26);
        for side in 0..2 { for n in 0..=12 {
            let a=if side==0 {std::f32::consts::PI+std::f32::consts::PI*n as f32/12.0} else {std::f32::consts::PI*n as f32/12.0};
            let vx=radius*a.cos();
            let vy=if side==0 {-reach+radius*a.sin()} else {reach+radius*a.sin()};
            let (dx,dy)=if height>=width {(vx,vy)} else {(-vy,vx)};
            let (sin,cos)=tilt.sin_cos();
            points.push(map(egui::pos2(x+pose.gaze.x+dx*cos-dy*sin,68.0+pose.gaze.y+dx*sin+dy*cos)));
        } }
        p.add(egui::Shape::convex_polygon(points,ink,Stroke::NONE));
    }
    let blush = Color32::from_rgba_unmultiplied(239,195,181,(pose.blush.clamp(0.0,1.0)*255.0) as u8);
    for x in [49.0,111.0] { disk(x,91.0,6.5,3.5,blush); }
    let mut mouth=Vec::new();
    for i in 0..=16 {
        let t=i as f32/16.0;
        let (width, curvature) = match pose.mouth { Mouth::Smile => (20.0,4.5), Mouth::Wide => (24.0,8.0), Mouth::Small => (11.0,2.0), Mouth::Flat => (14.0,0.0), Mouth::Wavy => (17.0,-2.0), Mouth::Round => (12.0,5.0) };
        let y = if pose.mouth == Mouth::Round { 91.0 + (t*std::f32::consts::TAU).sin()*curvature } else { 89.0+curvature*4.0*t*(1.0-t) };
        let x = if pose.mouth == Mouth::Round { 80.0 + (t*std::f32::consts::TAU).cos()*width/2.0 } else {80.0-width/2.0+width*t};
        mouth.push(map(egui::pos2(x,y)));
    }
    p.add(egui::Shape::line(mouth, Stroke::new(3.8*scale,ink)));
    let accent = Color32::from_rgb(216, 109, 126);
    let line = |points: &[(f32,f32)], color: Color32, width: f32| {
        p.add(egui::Shape::line(points.iter().map(|&(x,y)|map(egui::pos2(x,y))).collect(),Stroke::new(width*scale,color)));
    };
    match pose.effect {
        Effect::None => {},
        Effect::Heart => { line(&[(76.,103.),(69.,98.),(68.,94.),(71.,92.),(75.,94.),(78.,92.),(82.,94.),(81.,98.),(76.,103.)],accent,2.5); },
        Effect::Sparkle => { let r=3.0+(time*5.0).sin().abs()*2.0; for (x,y) in [(44.,53.),(117.,53.)] { line(&[(x-r,y),(x+r,y)],accent,2.0); line(&[(x,y-r),(x,y+r)],accent,2.0); } },
        Effect::Dizzy => { let mut points=Vec::new(); for i in 0..=20 { let t=i as f32/20.0; let a=t*std::f32::consts::TAU*2.0+time*3.0; points.push((112.0+a.cos()*t*6.0,48.0+a.sin()*t*6.0)); } line(&points,accent,1.7); },
        Effect::Sleep => { line(&[(103.,48.),(111.,48.),(103.,55.),(111.,55.)],ink,2.0); },
        Effect::Music => { line(&[(109.,51.),(109.,42.),(116.,40.),(116.,48.)],ink,2.2); disk(107.,52.,2.7,2.7,ink); disk(114.,49.,2.7,2.7,ink); },
        Effect::Pull => { line(&[(39.,78.),(46.,82.),(40.,87.)],ink,2.0); line(&[(121.,78.),(114.,82.),(120.,87.)],ink,2.0); },
        Effect::Sun | Effect::Moon => { disk(110.,48.,6.,6.,if pose.effect==Effect::Sun {Color32::from_rgb(245,181,80)} else {Color32::from_rgb(195,195,225)}); },
        Effect::Cloud | Effect::CloudSun | Effect::CloudMoon | Effect::Rain | Effect::Thunder => {
            if matches!(pose.effect, Effect::CloudSun | Effect::CloudMoon) { disk(112.,43.,5.,5.,if pose.effect==Effect::CloudSun {Color32::from_rgb(245,181,80)} else {Color32::from_rgb(195,195,225)}); }
            let cloud=Color32::from_rgb(165,175,190); disk(108.,47.,5.,5.,cloud); disk(115.,48.,4.,4.,cloud);
            if pose.effect==Effect::Rain { let y=(time*5.0).rem_euclid(4.0); line(&[(106.,55.+y),(104.,60.+y)],Color32::from_rgb(100,160,210),2.0); line(&[(114.,55.+y),(112.,60.+y)],Color32::from_rgb(100,160,210),2.0); }
            if pose.effect==Effect::Thunder { line(&[(112.,52.),(108.,58.),(112.,58.),(109.,64.)],Color32::from_rgb(230,170,45),2.0); }
        },
        Effect::Fog => { line(&[(103.,47.),(120.,47.)],ink,1.5); line(&[(100.,52.),(117.,52.)],ink,1.5); },
        Effect::Snow => { let fall=(time*2.0).rem_euclid(5.0); for (x,y) in [(106.,47.+fall),(116.,55.+fall)] { line(&[(x-3.,y),(x+3.,y)],ink,1.4); line(&[(x,y-3.),(x,y+3.)],ink,1.4); } },
    }
}

#[derive(Default)]
pub struct DesktopPet { animator: Animator, motion: Motion, last_frame: Option<Instant>, initialized: bool, visible: bool, last_position: Option<(i32,i32)>, moved_at: Option<Instant>, pending_event: Option<Event>, pending_preview: Option<Preview>, pending_motion_preview: Option<MotionKind>, #[cfg(windows)] region_size: Option<(i32, i32)>, #[cfg(windows)] region_points: Option<Vec<(i32,i32)>>, #[cfg(windows)] drag: Option<Drag> }
#[cfg(windows)]
struct Drag { offset: (i32,i32), last_x: i32, delta_x: f32 }
impl DesktopPet {
    fn show(&mut self, ctx: &egui::Context, settings: &mut Settings, fullscreen: bool, context: Context, event: Option<Event>, preview: Option<Preview>, motion_preview: Option<MotionKind>) {
        if !settings.desktop_pet {
            #[cfg(windows)] if self.drag.take().is_some() { if let Some((pos,_))=native::window_geometry() { self.last_position=Some(pos); self.moved_at=Some(Instant::now()); } }
            if self.moved_at.take().is_some() {
                settings.desktop_pet_position = self.last_position;
                settings.save();
            }
            self.initialized=false; self.visible=false; self.last_frame=None;
            self.animator=Animator::default(); self.motion=Motion::default(); self.pending_event=None; self.pending_preview=None; self.pending_motion_preview=None;
            #[cfg(windows)] { self.region_size = None; self.region_points=None; self.drag=None; }
            return;
        }
        if event.is_some() { self.pending_event=event; }
        if preview.is_some() { self.pending_preview=preview; }
        if motion_preview.is_some() && settings.desktop_pet_motion { self.pending_motion_preview=motion_preview; }
        if !settings.desktop_pet_motion { self.pending_motion_preview=None; }
        let hidden = fullscreen && settings.hide_fullscreen;
        let requested_size = SIZE * (settings.desktop_pet_size_pct.clamp(DESKTOP_PET_MIN_SIZE, DESKTOP_PET_MAX_SIZE) as f32 / 100.0);
        let id=egui::ViewportId::from_hash_of("gua-desktop-pet");
        ctx.show_viewport_immediate(id, egui::ViewportBuilder::default().with_title(TITLE).with_inner_size(requested_size).with_decorations(false).with_transparent(true).with_resizable(false).with_taskbar(false).with_always_on_top().with_active(false).with_visible(false), |ui, _| {
            let now=Instant::now();
            if hidden {
                if self.visible { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false)); self.visible=false; }
                self.last_frame=None; self.animator.pause();
                #[cfg(windows)] if self.drag.take().is_some() { if let Some((pos,_))=native::window_geometry() { settings.desktop_pet_position=Some(pos); self.last_position=Some(pos); settings.save(); } self.motion.released(); }
                return;
            }
            #[cfg(windows)]
            if !self.initialized {
                if let Some((position, size))=native::initialize(settings.desktop_pet_position) { self.initialized=true; self.last_position=Some(position); self.region_size=Some(size); }
                ui.ctx().request_repaint_after(Duration::from_millis(50));
            }
            #[cfg(not(windows))]
            if !self.initialized { self.initialized=true; }
            if self.initialized {
                #[cfg(windows)]
                native::ensure_frameless();
                if !self.visible { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true)); self.visible=true; }
                let dt=self.last_frame.replace(now).map_or(0.0, |old| now.duration_since(old).as_secs_f32());
                #[cfg(windows)]
                let mut geometry = native::window_geometry();
                #[cfg(windows)]
                let cursor_screen = native::cursor_screen();
                #[cfg(windows)]
                let cursor_data = geometry.zip(cursor_screen).and_then(|(geometry,cursor)| native::cursor_in_character(geometry,cursor));
                #[cfg(windows)]
                let cursor = cursor_data.map(|(point, _)| point);
                #[cfg(not(windows))]
                let cursor = None;
                let rect=ui.max_rect();
                let response=ui.interact(rect, ui.scope_id().with("mochi"), Sense::click_and_drag());
                #[cfg(windows)]
                if response.drag_started() && self.drag.is_none() {
                    if let (Some((pos,_)),Some(cursor))=(geometry,cursor_screen) {
                        self.drag=Some(Drag { offset:(cursor.0-pos.0,cursor.1-pos.1), last_x:cursor.0, delta_x:0.0 });
                    }
                }
                #[cfg(not(windows))]
                if response.drag_started() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag); }
                #[cfg(windows)]
                let mut released=false;
                #[cfg(windows)]
                if let Some(drag)=&mut self.drag {
                    if native::left_button_down() {
                        if let Some(cursor)=cursor_screen {
                            drag.delta_x=(cursor.0-drag.last_x) as f32 / ui.ctx().pixels_per_point().max(0.1);
                            drag.last_x=cursor.0;
                            let pos=(cursor.0.saturating_sub(drag.offset.0),cursor.1.saturating_sub(drag.offset.1));
                            native::move_to(pos);
                            geometry=native::window_geometry();
                        }
                    } else {
                        released=true;
                    }
                }
                #[cfg(windows)]
                if released {
                    self.drag=None; self.motion.released();
                    geometry=native::window_geometry();
                    if let Some((pos,_))=geometry { self.last_position=Some(pos); self.moved_at=None; settings.desktop_pet_position=Some(pos); settings.save(); }
                }
                #[cfg(windows)]
                let dragging=self.drag.is_some();
                #[cfg(windows)]
                let delta_x=self.drag.as_ref().map_or(0.0,|drag|drag.delta_x);
                #[cfg(not(windows))]
                let dragging=response.dragged();
                #[cfg(not(windows))]
                let delta_x=response.drag_delta().x;
                if let Some(event)=self.pending_event.take() { self.animator.react(event); }
                #[cfg(windows)]
                let clicked=response.clicked() && !released && !dragging;
                #[cfg(not(windows))]
                let clicked=response.clicked();
                if clicked { self.animator.click(); self.motion.click(); }
                if let Some(preview)=self.pending_preview.take() { self.animator.preview(preview); }
                if let Some(preview)=self.pending_motion_preview.take() { self.motion.preview(preview); }
                #[cfg(windows)]
                {
                    let scale=ui.ctx().pixels_per_point();
                    if self.animator.cursor_scale.is_some_and(|previous| (previous-scale).abs()>0.001) { self.animator.shake.observe(None,self.animator.time as f64); }
                    self.animator.cursor_scale=Some(scale);
                    let x=cursor_data.map(|(_, x)| x/scale);
                    if self.animator.shake.observe(x,self.animator.time as f64) { self.animator.dizzy(); }
                }
                let pose=self.animator.step(dt,cursor,response.hovered(),dragging,context);
                let body=self.motion.step(dt,settings.desktop_pet_motion,context,cursor,dragging,delta_x,self.animator.is_playing(),self.animator.motion_cue());
                #[cfg(windows)]
                if let Some((pos, size))=geometry {
                    let points=native::region_points(size,body);
                    if self.region_points.as_ref()!=Some(&points) && native::set_region(&points) {
                        self.region_points=Some(points);
                    }
                    if self.region_size != Some(size) { self.region_size=Some(size); native::ensure_visible(pos, size); }
                    if self.last_position != Some(pos) { self.last_position=Some(pos); if !dragging { self.moved_at=Some(now); } }
                    if !dragging && self.moved_at.is_some_and(|t| now.duration_since(t)>Duration::from_millis(400)) { settings.desktop_pet_position=Some(pos); settings.save(); self.moved_at=None; }
                }
                paint(ui,rect,pose,body,settings.desktop_pet_color,self.animator.time);
                ui.ctx().request_repaint_after(Duration::from_millis(33));
            }
        });
    }
}

impl crate::app::App {
    pub(crate) fn react_pet(&mut self, state: crate::pet::PetState) {
        self.pet.react(state);
        if self.settings.desktop_pet {
            self.desktop_pet_event = match state {
                crate::pet::PetState::Happy => Some(Event::Happy),
                crate::pet::PetState::Satisfied => Some(Event::Satisfied),
                _ => None,
            };
        }
    }

    pub fn show_desktop_pet(&mut self, ctx: &egui::Context, desktop: &mut DesktopPet, fullscreen: bool, env: &crate::app::Env) {
        let context = Context {
            idle: self.idle_for(),
            idle_threshold: self.opts.pet_idle,
            focus: self.pomo.running && self.pomo.phase == crate::app::Phase::Focus,
            worried: (self.settings.show_battery && env.battery.is_some_and(|v| v <= 20) && !env.charging) || env.network == Some(false),
            music: self.media.playing && self.media.is_music(),
            weather: if self.settings.show_weather { self.weather.reading().map(|reading| (reading.kind, reading.day)) } else { None },
            animations: crate::bar_layout::animations_enabled(),
        };
        let event=self.desktop_pet_event.take();
        let preview=self.desktop_pet_preview.take();
        let motion_preview=self.desktop_pet_motion_preview.take();
        desktop.show(ctx,&mut self.settings,fullscreen,context,event,preview,motion_preview);
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{CombineRgn, CreatePolygonRgn, DeleteObject, GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, OffsetRgn, SetWindowRgn, ALTERNATE, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY, RGN_OR};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::*;
    const SUBCLASS_ID: usize = 0x4d4f_4348; // "MOCH"

    fn frameless_style(style: u32) -> u32 {
        (style | WS_POPUP.0) & !(WS_CAPTION.0 | WS_THICKFRAME.0 | WS_SYSMENU.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0)
    }

    unsafe extern "system" fn frameless_subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
        match msg {
            // winit 在调整可见性等属性时可能重新写入标题栏样式；在生效前移除。
            WM_STYLECHANGING if wp.0 as i32 == GWL_STYLE.0 && lp.0 != 0 => {
                let result=DefSubclassProc(hwnd,msg,wp,lp);
                let style=&mut *(lp.0 as *mut STYLESTRUCT);
                style.styleNew=frameless_style(style.styleNew);
                result
            }
            // 与信息岛一致：客户区占满窗口，并阻止系统绘制非客户区。
            WM_NCCALCSIZE | WM_NCPAINT => LRESULT(0),
            WM_NCACTIVATE => DefSubclassProc(hwnd,msg,wp,LPARAM(-1)),
            WM_DWMCOMPOSITIONCHANGED | WM_DISPLAYCHANGE | WM_DWMCOLORIZATIONCOLORCHANGED => {
                let result=DefSubclassProc(hwnd,msg,wp,lp);
                let _=crate::native::enable_alpha_composition(hwnd);
                result
            }
            WM_SHOWWINDOW if wp.0 != 0 => {
                let result=DefSubclassProc(hwnd,msg,wp,lp);
                let _=crate::native::enable_alpha_composition(hwnd);
                result
            }
            WM_NCDESTROY => {
                let result=DefSubclassProc(hwnd,msg,wp,lp);
                let _=RemoveWindowSubclass(hwnd,Some(frameless_subclass),SUBCLASS_ID);
                result
            }
            _ => DefSubclassProc(hwnd,msg,wp,lp),
        }
    }

    unsafe fn apply_frameless(hwnd: HWND) {
        let current=GetWindowLongPtrW(hwnd,GWL_STYLE);
        let desired=frameless_style(current as u32);
        if current as u32 != desired {
            SetWindowLongPtrW(hwnd,GWL_STYLE,desired as i32 as isize);
            let _=SetWindowPos(hwnd,HWND_TOPMOST,0,0,0,0,SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE|SWP_NOZORDER|SWP_FRAMECHANGED);
        }
    }

    fn hwnd() -> Option<HWND> { unsafe { FindWindowW(None, w!("瓜瓜桌宠 · Mochi")).ok() } }
    pub fn ensure_frameless() { if let Some(hwnd)=hwnd() { unsafe { apply_frameless(hwnd); } } }
    pub fn window_geometry() -> Option<((i32,i32), (i32,i32))> { unsafe { let mut r=RECT::default(); GetWindowRect(hwnd()?, &mut r).ok()?; Some(((r.left,r.top), (r.right-r.left,r.bottom-r.top))) } }
    pub fn cursor_screen() -> Option<(i32,i32)> { let mut cursor=POINT::default(); unsafe { GetCursorPos(&mut cursor) }.ok()?; Some((cursor.x,cursor.y)) }
    pub fn left_button_down() -> bool { unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0 } }
    pub fn move_to((x,y): (i32,i32)) { if let Some(hwnd)=hwnd() { unsafe { let _=SetWindowPos(hwnd,HWND_TOPMOST,x,y,0,0,SWP_NOSIZE|SWP_NOACTIVATE|SWP_NOZORDER); } } }
    pub fn cursor_in_character(((left, top), (width, height)): ((i32, i32), (i32, i32)), cursor: (i32,i32)) -> Option<(Pos2,f32)> {
        if width <= 0 || height <= 0 { return None; }
        // 鼠标和窗口都以屏幕物理像素表示，避免跨显示器时混用 DPI。
        Some((egui::pos2(
            ((cursor.0 as f64 - left as f64) * SIZE.x as f64 / width as f64) as f32,
            ((cursor.1 as f64 - top as f64) * SIZE.y as f64 / height as f64) as f32,
        ),cursor.0 as f32))
    }
    pub fn ensure_visible((x, y): (i32, i32), (width, height): (i32, i32)) { unsafe {
        let Some(hwnd) = hwnd() else { return; };
        if width <= 0 || height <= 0 { return; }
        let rect = RECT { left: x, top: y, right: x.saturating_add(width), bottom: y.saturating_add(height) };
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() { return; }
        let work = info.rcWork;
        if rect.left < work.right && rect.right > work.left && rect.top < work.bottom && rect.bottom > work.top { return; }
        let new_x = x.clamp(work.left, (work.right - width).max(work.left));
        let new_y = y.clamp(work.top, (work.bottom - height).max(work.top));
        let _ = SetWindowPos(hwnd, HWND_TOPMOST, new_x, new_y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    } }
    pub fn region_points((width,height): (i32,i32), body: BodyPose) -> Vec<(i32,i32)> {
        if width<=0 || height<=0 { return Vec::new(); }
        outline().into_iter().map(|p| { let p=body.map(p); ((p.x*width as f32/SIZE.x).round() as i32,(p.y*height as f32/SIZE.y).round() as i32) }).collect()
    }
    pub fn set_region(points: &[(i32,i32)]) -> bool { unsafe {
        let Some(hwnd) = hwnd() else { return false; };
        if points.len()<3 { return false; }
        let points:Vec<POINT>=points.iter().map(|&(x,y)| POINT{x,y}).collect();
        let region=CreatePolygonRgn(&points,ALTERNATE);
        if region.is_invalid() { return false; }
        let shifted=CreatePolygonRgn(&points,ALTERNATE);
        if shifted.is_invalid() { let _=DeleteObject(region); return false; }
        // 窗口区域只能按整像素裁剪；向外留两像素，避免截掉 egui 的抗锯齿边缘。
        let mut previous=(0,0);
        for (x,y) in [(-2,-2),(0,-2),(2,-2),(-2,0),(2,0),(-2,2),(0,2),(2,2)] {
            if OffsetRgn(shifted,x-previous.0,y-previous.1).0==0 || CombineRgn(region,region,shifted,RGN_OR).0==0 {
                let _=DeleteObject(shifted); let _=DeleteObject(region); return false;
            }
            previous=(x,y);
        }
        let _=DeleteObject(shifted);
        if SetWindowRgn(hwnd,region,true)==0 { let _=DeleteObject(region); return false; }
        true
    } }
    pub fn initialize(saved: Option<(i32,i32)>) -> Option<((i32,i32),(i32,i32))> { unsafe {
        let hwnd=hwnd()?;
        if !SetWindowSubclass(hwnd,Some(frameless_subclass),SUBCLASS_ID,0).as_bool() { return None; }
        apply_frameless(hwnd);
        let _=crate::native::enable_alpha_composition(hwnd);
        let mut window=RECT::default(); GetWindowRect(hwnd,&mut window).ok()?;
        let width=window.right-window.left; let height=window.bottom-window.top;
        if width<=0 || height<=0 { return None; }
        let mut info=MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let mut pos=None;
        if let Some((x,y))=saved { let mon=MonitorFromPoint(POINT{x,y},MONITOR_DEFAULTTONEAREST); if GetMonitorInfoW(mon,&mut info).as_bool() && x < info.rcWork.right && x.saturating_add(width) > info.rcWork.left && y < info.rcWork.bottom && y.saturating_add(height) > info.rcWork.top { pos=Some((x,y)); } }
        let (x,y)=if let Some(p)=pos { p } else { let mon=MonitorFromPoint(POINT{x:0,y:0},MONITOR_DEFAULTTOPRIMARY); if !GetMonitorInfoW(mon,&mut info).as_bool() { return None; } (info.rcWork.right-width-24, info.rcWork.bottom-height-24) };
        if !set_region(&region_points((width,height),BodyPose::default())) { return None; }
        let _=SetWindowPos(hwnd,HWND_TOPMOST,x,y,0,0,SWP_NOSIZE|SWP_NOACTIVATE);
        Some(((x,y),(width,height)))
    } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn happy_returns_to_idle() {
        let mut a=Animator::default();
        let context=Context { idle_threshold:45.0, animations:true, ..Default::default() };
        a.react(Event::Happy);
        assert!(matches!(a.step(0.1,None,false,false,context).mouth,Mouth::Wide));
        for _ in 0..50 { a.step(0.1,None,false,false,context); }
        assert!(matches!(a.step(0.0,None,false,false,context).mouth,Mouth::Smile));
    }
    #[test] fn blink_and_idle_are_time_driven() {
        let mut a=Animator::default();
        let context=Context { idle_threshold:45.0, animations:true, ..Default::default() };
        a.time=2.0+crate::anim::hash01(0)*2.7+0.09;
        assert!(a.step(0.0,None,false,false,context).eyes[0]<0.1);
        a.time+=0.2;
        assert!(a.step(0.0,None,false,false,context).eyes[0]>0.9);
    }
}
