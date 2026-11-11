//! 麻薯桌宠的表情、状态和独立调度；不依赖窗口和信息岛的可见性。
use crate::pet::{MouseShake, MusicPose, PetState};
use crate::pet_expressions::Expression;
use crate::weather::Kind;
use eframe::egui::{self, Pos2, Vec2};

#[derive(Clone, Copy)]
pub(crate) enum Preview {
    Expression(Expression),
    State(PetState),
    Weather(Kind, bool),
    Music(MusicPose),
}

pub(crate) const STATES: [(PetState, &str); 12] = [
    (PetState::Awake, "待机"), (PetState::Curious, "好奇"), (PetState::Waking, "唤醒"),
    (PetState::Happy, "开心"), (PetState::Satisfied, "满足"), (PetState::Dizzy, "眩晕"),
    (PetState::Pulling, "拉扯"), (PetState::Music, "听歌"), (PetState::Worried, "担忧"),
    (PetState::Focused, "专注"), (PetState::Dozing, "打盹"), (PetState::Sleeping, "睡眠"),
];
pub(crate) const WEATHER: [(Kind, &str); 7] = [
    (Kind::Clear, "晴"), (Kind::PartlyCloudy, "少云"), (Kind::Cloudy, "多云"),
    (Kind::Fog, "雾"), (Kind::Rain, "雨"), (Kind::Snow, "雪"), (Kind::Thunder, "雷雨"),
];
pub(crate) const MUSIC: [(MusicPose, &str); 6] = [
    (MusicPose::Microphone, "麦克风"), (MusicPose::Hum, "哼唱"), (MusicPose::SideSing, "侧身唱"),
    (MusicPose::Squint, "眯眼唱"), (MusicPose::HighNote, "高音"), (MusicPose::Smile, "微笑唱"),
];

#[derive(Clone, Copy, Default)]
pub(crate) struct Context {
    pub idle: f64,
    pub idle_threshold: f64,
    pub focus: bool,
    pub worried: bool,
    pub music: bool,
    pub weather: Option<(Kind, bool)>,
    pub animations: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Event { Happy, Satisfied }

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mouth { Smile, Wide, Small, Round, Flat, Wavy }
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Effect { None, Heart, Sparkle, Sleep, Dizzy, Sun, Moon, Cloud, CloudSun, CloudMoon, Fog, Rain, Snow, Thunder, Music, Pull }

#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub eyes: [f32; 2],
    pub tilt: [f32; 2],
    pub gaze: Vec2,
    pub gaze_override: Option<Vec2>,
    pub mouth: Mouth,
    pub blush: f32,
    pub effect: Effect,
}

impl Default for Pose {
    fn default() -> Self { Self { eyes: [1.0; 2], tilt: [0.0; 2], gaze: Vec2::ZERO, gaze_override: None, mouth: Mouth::Smile, blush: 0.5, effect: Effect::None } }
}

#[derive(Clone, Copy)]
struct Clip { item: Preview, age: f32, duration: f32, priority: u8 }

pub(super) struct Animator {
    pub time: f32,
    gaze: Vec2,
    clip: Option<Clip>,
    motion_cue: Option<(Expression, f32, f32)>,
    idle_wait: f32,
    idle_index: usize,
    hover: bool,
    hover_cooldown: f32,
    music_time: f32,
    last_idle: f64,
    waking: f32,
    weather_key: Option<(Kind, bool)>,
    weather_pending: bool,
    weather_idle: f32,
    pub shake: MouseShake,
    pub cursor_scale: Option<f32>,
}

impl Default for Animator {
    fn default() -> Self { Self { time: 0.0, gaze: Vec2::ZERO, clip: None, motion_cue: None, idle_wait: 8.0, idle_index: 0, hover: false, hover_cooldown: 0.0, music_time: 0.0, last_idle: 0.0, waking: 0.0, weather_key: None, weather_pending: false, weather_idle: 0.0, shake: MouseShake::default(), cursor_scale: None } }
}

impl Animator {
    pub fn is_playing(&self) -> bool { self.clip.is_some() }
    pub fn motion_cue(&self) -> Option<(Expression, f32, f32)> { self.motion_cue }
    fn play(&mut self, item: Preview, duration: f32, priority: u8) {
        if self.clip.is_none_or(|clip| priority >= clip.priority) {
            self.clip = Some(Clip { item, age: 0.0, duration, priority });
        }
    }
    pub fn preview(&mut self, item: Preview) {
        let duration = match item { Preview::Expression(e) => e.duration(), _ => 4.0 };
        self.play(item, duration, 4);
    }
    pub fn react(&mut self, event: Event) {
        let expression = match event { Event::Happy => Expression::Delighted, Event::Satisfied => Expression::Heart };
        self.play(Preview::Expression(expression), expression.duration(), 3);
    }
    pub fn dizzy(&mut self) { self.play(Preview::State(PetState::Dizzy), 4.0, 3); }
    pub fn click(&mut self) { self.react(Event::Happy); }
    pub fn pause(&mut self) { self.shake = MouseShake::default(); self.cursor_scale = None; self.hover = false; }

    pub fn step(&mut self, dt: f32, cursor: Option<Pos2>, hovered: bool, dragging: bool, context: Context) -> Pose {
        let dt = dt.clamp(0.0, 0.1);
        self.time += dt;
        self.hover_cooldown = (self.hover_cooldown - dt).max(0.0);
        if self.last_idle >= context.idle_threshold && context.idle < 1.0 { self.waking = 2.2; }
        self.last_idle = context.idle;
        self.waking = (self.waking - dt).max(0.0);
        if context.music { self.music_time += dt; }
        if self.weather_key != context.weather { self.weather_key = context.weather; self.weather_pending = context.weather.is_some(); self.weather_idle = 0.0; }

        if !dragging {
            if let Some(clip) = &mut self.clip { clip.age += dt; if clip.age >= clip.duration { self.clip = None; } }
        }
        let base = if self.waking > 0.0 { PetState::Waking }
            else if context.focus { PetState::Focused }
            else if context.worried { PetState::Worried }
            else if context.music { PetState::Music }
            else if hovered { PetState::Curious }
            else if context.idle >= 180.0 { PetState::Sleeping }
            else if context.idle >= context.idle_threshold { PetState::Dozing }
            else { PetState::Awake };

        if !dragging && self.clip.is_none() && context.animations {
            if base == PetState::Awake {
                self.weather_idle += dt;
                if let Some((kind, day)) = context.weather.filter(|_| self.weather_pending || self.weather_idle >= 300.0) {
                    self.play(Preview::Weather(kind, day), 4.0, 1);
                    self.weather_pending = false;
                    self.weather_idle = 0.0;
                } else {
                    self.idle_wait -= dt;
                    if self.idle_wait <= 0.0 {
                        let expression = Expression::ALL[self.idle_index % Expression::ALL.len()];
                        self.idle_index += 1;
                        self.play(Preview::Expression(expression), expression.duration(), 1);
                        self.idle_wait = 22.0 + (self.idle_index % 4) as f32 * 3.0;
                    }
                }
            } else if base == PetState::Curious && hovered && !self.hover && self.hover_cooldown <= 0.0 {
                let options = [Expression::Wink, Expression::Blush, Expression::SideGlance, Expression::ShyPeek];
                let expression = options[self.idle_index % options.len()];
                self.idle_index += 1;
                self.play(Preview::Expression(expression), expression.duration(), 2);
                self.hover_cooldown = 18.0;
            }
        }
        self.hover = hovered;
        // 持续状态优先于自动待机/天气；手动预览和即时反馈可覆盖它们。
        let clip = self.clip.filter(|clip| clip.priority >= 3 || matches!(base, PetState::Awake | PetState::Curious));
        self.motion_cue = if dragging { None } else { clip.and_then(|clip| match clip.item { Preview::Expression(e) => Some((e,clip.age,clip.duration)), _ => None }) };
        let mut pose = if dragging { state_pose(PetState::Pulling, self.time) }
            else if let Some(clip) = clip { pose_for(clip.item, clip.age, clip.duration) }
            else if base == PetState::Music && context.animations { music_sequence(self.music_time) }
            else if base == PetState::Music { music_pose(MusicPose::Hum, self.music_time) }
            else { state_pose(base, self.time) };
        if clip.is_none() && matches!(base, PetState::Awake | PetState::Curious | PetState::Focused | PetState::Worried) {
            let start = 2.0 + crate::anim::hash01((self.time / 5.6).floor() as u64) * 2.7;
            let blink = (self.time % 5.6 - start) / 0.18;
            if (0.0..1.0).contains(&blink) { let v = 1.0 - (blink * std::f32::consts::PI).sin(); pose.eyes[0] *= v; pose.eyes[1] *= v; }
        }
        let target = pose.gaze_override.unwrap_or_else(|| cursor.map(|p| egui::vec2(((p.x-80.0)/28.0).clamp(-3.0,3.0), ((p.y-68.0)/40.0).clamp(-1.0,1.5))).unwrap_or_else(|| egui::vec2((self.time*0.35).sin()*0.8,0.0)));
        self.gaze.x = crate::anim::approach(self.gaze.x, target.x, 10.0, dt);
        self.gaze.y = crate::anim::approach(self.gaze.y, target.y, 10.0, dt);
        pose.gaze = self.gaze;
        pose
    }
}

fn pose_for(item: Preview, time: f32, duration: f32) -> Pose {
    match item {
        Preview::Expression(e) => expression_pose(e, time, duration),
        Preview::State(s) => state_pose(s, time),
        Preview::Weather(kind, day) => weather_pose(kind, day, time),
        Preview::Music(m) => music_pose(m, time),
    }
}

fn expression_pose(expression: Expression, time: f32, duration: f32) -> Pose {
    use Expression::*;
    let mut p = Pose::default();
    let envelope = (time / 0.2).min(1.0).min(((duration - time) / 0.3).clamp(0.0, 1.0));
    match expression {
        Wink => { p.eyes[0] = 1.0 - 0.92 * envelope; p.mouth = Mouth::Wide; }
        Smug => { p.eyes = [1.0 - 0.35*envelope, 1.0 - 0.5*envelope]; p.tilt = [0.18*envelope, -0.1*envelope]; p.mouth = Mouth::Small; }
        Blush => { p.blush = 0.5 + 0.5*envelope; p.eyes = [1.0 - 0.25*envelope; 2]; }
        Delighted => { p.eyes = [1.0 - 0.65*envelope; 2]; p.mouth = Mouth::Wide; }
        SideGlance => { p.gaze_override = Some(egui::vec2(3.0*envelope, 0.0)); p.mouth = Mouth::Small; }
        Praised => { p.effect = Effect::Sparkle; p.blush = 0.9; p.eyes = [1.0 - 0.35*envelope; 2]; p.mouth = Mouth::Wide; }
        Heart => { p.effect = Effect::Heart; p.blush = 0.9; p.mouth = Mouth::Wide; }
        Giggle => { p.eyes = [1.0 - 0.72*envelope; 2]; p.mouth = Mouth::Wide; }
        ShyPeek => { p.eyes = [1.0 - 0.5*envelope, 1.0 - 0.2*envelope]; p.blush = 1.0; p.gaze_override = Some(egui::vec2(-2.0*envelope, 1.0)); }
        Scan => { p.gaze_override = Some(egui::vec2((time*2.0).sin()*3.0, -0.5)); }
        Peek => { p.eyes = [1.0 + 0.2*envelope; 2]; p.gaze_override = Some(egui::vec2(0.0,-1.0)); }
        Search => { p.gaze_override = Some(egui::vec2((time*3.0).sin()*3.0, 0.0)); p.mouth = Mouth::Round; }
    }
    p
}

fn state_pose(state: PetState, time: f32) -> Pose {
    let mut p = Pose::default();
    match state {
        PetState::Awake => {},
        PetState::Curious => { p.eyes = [1.12,1.18]; p.mouth = Mouth::Round; },
        PetState::Waking => { p.eyes = [0.55,0.7]; p.mouth = Mouth::Round; },
        PetState::Happy => { p.eyes = [0.4;2]; p.mouth = Mouth::Wide; },
        PetState::Satisfied => { p.eyes = [0.5;2]; p.effect = Effect::Heart; p.blush = 1.0; },
        PetState::Dizzy => { p.eyes = [0.3;2]; p.mouth = Mouth::Wavy; p.effect = Effect::Dizzy; p.gaze_override = Some(egui::vec2((time*4.0).sin()*2.0,0.0)); },
        PetState::Pulling => { p.eyes = [0.5;2]; p.tilt = [-0.2;2]; p.mouth = Mouth::Wavy; p.effect = Effect::Pull; },
        PetState::Music => return music_pose(MusicPose::Hum, time),
        PetState::Worried => { p.eyes = [0.75,0.55]; p.tilt = [0.15,-0.15]; p.mouth = Mouth::Wavy; },
        PetState::Focused => { p.eyes = [0.55;2]; p.mouth = Mouth::Flat; },
        PetState::Dozing => { p.eyes = [0.2;2]; p.mouth = Mouth::Small; p.gaze_override = Some(Vec2::ZERO); },
        PetState::Sleeping => { p.eyes = [0.08;2]; p.mouth = Mouth::Small; p.effect = Effect::Sleep; p.gaze_override = Some(Vec2::ZERO); },
    }
    p
}

fn music_pose(music: MusicPose, _time: f32) -> Pose {
    let mut p = Pose::default();
    p.effect = Effect::Music;
    p.mouth = match music { MusicPose::Microphone | MusicPose::HighNote => Mouth::Round, MusicPose::SideSing => Mouth::Wide, MusicPose::Hum => Mouth::Small, MusicPose::Squint => Mouth::Flat, MusicPose::Smile => Mouth::Smile };
    match music { MusicPose::Squint | MusicPose::Smile => p.eyes = [0.35;2], MusicPose::HighNote => p.eyes = [0.7,1.0], MusicPose::SideSing => p.gaze_override = Some(egui::vec2(2.5,0.0)), _ => {} }
    p
}

fn music_sequence(age: f32) -> Pose {
    let cycle: f32 = Expression::ALL.iter().map(|e| 16.0 + e.duration()).sum();
    let mut at = age % cycle;
    for (index, expression) in Expression::ALL.into_iter().enumerate() {
        if at < 16.0 { return music_pose(MUSIC[index % MUSIC.len()].0, at); }
        at -= 16.0;
        if at < expression.duration() { return expression_pose(expression, at, expression.duration()); }
        at -= expression.duration();
    }
    music_pose(MusicPose::Hum, age)
}

fn weather_pose(kind: Kind, day: bool, time: f32) -> Pose {
    let mut p = Pose::default();
    p.effect = match kind { Kind::Clear => if day {Effect::Sun} else {Effect::Moon}, Kind::PartlyCloudy => if day {Effect::CloudSun} else {Effect::CloudMoon}, Kind::Cloudy => Effect::Cloud, Kind::Fog => Effect::Fog, Kind::Rain => Effect::Rain, Kind::Snow => Effect::Snow, Kind::Thunder => Effect::Thunder };
    match kind { Kind::Clear | Kind::Snow => { p.eyes = [0.45;2]; p.mouth = Mouth::Wide; }, Kind::Fog => p.gaze_override = Some(egui::vec2((time*2.0).sin()*2.0,0.0)), Kind::Rain => { p.eyes=[0.7;2]; p.mouth=Mouth::Small; }, Kind::Thunder => {p.eyes=[1.25;2]; p.mouth=Mouth::Round;}, _ => {} }
    p
}
