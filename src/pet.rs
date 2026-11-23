//! 呱的原生状态、矢量表情与表情图集播放。
use crate::anim::{approach, lerp, smoothstep};
use crate::paint::{Card, H};
use crate::pet_expressions::Expression;
use crate::theme::{self, W};
use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PetState {
    Awake,
    Curious,
    Waking,
    Happy,
    Satisfied,
    Dizzy,
    Pulling,
    Music,
    Worried,
    Focused,
    Dozing,
    Sleeping,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shake_x(t: f64, amplitude: f32, frequency: f64) -> f32 {
        amplitude * (t * std::f64::consts::TAU * frequency).sin() as f32
    }

    #[test]
    fn mouse_shake_requires_five_seconds_and_rearms_after_a_pause() {
        let mut shake = MouseShake::default();
        let mut triggers = Vec::new();
        for sample in 0..=240 {
            let t = sample as f64 / 20.0;
            if shake.observe(Some(shake_x(t, 80.0, 1.0)), t) {
                triggers.push(t);
            }
        }
        assert_eq!(triggers.len(), 1, "持续摇晃只能触发一次");
        assert!(
            (5.0..5.3).contains(&triggers[0]),
            "必须持续约五秒后触发：{triggers:?}"
        );

        let held = shake_x(12.0, 80.0, 1.0);
        for sample in 241..=264 {
            let t = sample as f64 / 20.0;
            assert!(!shake.observe(Some(held), t));
        }
        for sample in 265..=390 {
            let t = sample as f64 / 20.0;
            if shake.observe(Some(shake_x(t - 13.2, 80.0, 1.0)), t) {
                triggers.push(t);
            }
        }
        assert_eq!(triggers.len(), 2, "停顿后再次持续摇晃应重新触发");
        assert!(triggers[1] >= 18.2);
    }

    #[test]
    fn mouse_shake_ignores_slow_small_and_one_way_movement() {
        for movement in 0..3 {
            let mut shake = MouseShake::default();
            for sample in 0..=400 {
                let t = sample as f64 / 20.0;
                let x = match movement {
                    0 => shake_x(t, 19.0, 1.0),
                    1 => shake_x(t, 100.0, 0.35),
                    _ => t as f32 * 100.0,
                };
                assert!(
                    !shake.observe(Some(x), t),
                    "普通鼠标移动不应触发：{movement}"
                );
            }
        }
    }

    #[test]
    fn mouse_shake_discards_partial_gestures_after_a_pause_or_missing_cursor() {
        let mut shake = MouseShake::default();
        for sample in 0..=60 {
            let t = sample as f64 / 20.0;
            assert!(!shake.observe(Some(shake_x(t, 80.0, 1.0)), t));
        }
        for sample in 61..=84 {
            let t = sample as f64 / 20.0;
            assert!(!shake.observe(Some(0.0), t));
        }
        for sample in 85..=180 {
            let t = sample as f64 / 20.0;
            assert!(
                !shake.observe(Some(shake_x(t - 4.2, 80.0, 1.0)), t),
                "停顿后必须重新累计五秒"
            );
        }
        assert!(!shake.observe(None, 9.05));
        for sample in 182..=260 {
            let t = sample as f64 / 20.0;
            assert!(
                !shake.observe(Some(shake_x(t - 9.05, 80.0, 1.0)), t),
                "无法取得光标时应重置"
            );
        }
    }

    #[test]
    fn singing_keeps_each_expression_for_fifteen_seconds_after_the_transition() {
        let mut pet = Pet::default();
        pet.step(PetState::Music, 0.0, 0.0, [0.0; 2], true);
        let pose = pet.music_pose();
        for frame in 1..960 {
            pet.step(
                PetState::Music,
                frame as f64 / 60.0,
                1.0 / 60.0,
                [0.0; 2],
                true,
            );
            assert_eq!(
                pet.music_pose(),
                pose,
                "一秒过渡和十五秒停留期间不能频繁换表情"
            );
        }
        pet.step(PetState::Music, 16.1, 0.2, [0.0; 2], true);
        assert_eq!(
            pet.expressions.current(),
            Some(Expression::Wink),
            "演唱一段后应先播放新表情"
        );
        pet.step(PetState::Music, 19.5, 3.4, [0.0; 2], true);
        assert_ne!(pet.music_pose(), pose);
        assert!(pet.expressions.current().is_none());
    }
    #[test]
    fn singing_rotates_expressions_and_starts_with_the_microphone() {
        let mut pet = Pet::default();
        let mut poses = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for frame in 0..15000 {
            pet.step(
                PetState::Music,
                frame as f64 / 60.0,
                1.0 / 60.0,
                [0.0; 2],
                true,
            );
            if let Some(expression) = pet.expressions.current() {
                seen.insert(expression);
            }
            let pose = pet.music_pose();
            if poses.last() != Some(&pose) {
                poses.push(pose);
            }
        }
        assert_eq!(
            &poses[..7],
            &[
                MusicPose::Microphone,
                MusicPose::Hum,
                MusicPose::SideSing,
                MusicPose::Squint,
                MusicPose::HighNote,
                MusicPose::Smile,
                MusicPose::Microphone
            ]
        );
        assert_eq!(seen.len(), 12, "连续听歌应穿插全部十二组表情");
    }
    #[test]
    fn interrupted_music_resumes_at_the_same_expression_and_reactions_take_priority() {
        let mut pet = Pet::default();
        pet.step(PetState::Music, 16.6, 16.6, [0.0; 2], true);
        assert_eq!(pet.expressions.current(), Some(Expression::Wink));
        let music_age = pet.music_age;
        pet.step(PetState::Music, 40.0, 20.0, [0.0; 2], false);
        assert_eq!(pet.music_age, music_age, "隐藏时听歌进度应暂停");
        for state in [
            PetState::Awake,
            PetState::Focused,
            PetState::Worried,
            PetState::Dizzy,
            PetState::Pulling,
        ] {
            pet.step(state, 41.0, 1.0, [0.0; 2], true);
            assert_eq!(pet.music_age, music_age, "其他状态不能推进听歌进度");
            pet.step(PetState::Music, 42.0, 0.0, [0.0; 2], true);
            assert_eq!(pet.expressions.current(), Some(Expression::Wink));
            assert_eq!(pet.music_age, music_age, "恢复时应从原来的表情帧继续");
        }
        pet.react(PetState::Satisfied);
        pet.step(PetState::Satisfied, 43.0, 0.1, [0.0; 2], true);
        assert_eq!(
            pet.expressions.current(),
            Some(Expression::Heart),
            "喝水反馈应优先展示"
        );
        assert_eq!(pet.music_age, music_age);
        pet.step(PetState::Music, 44.0, 0.0, [0.0; 2], true);
        assert_eq!(pet.expressions.current(), Some(Expression::Wink));
    }
    #[test]
    fn microphone_moves_gently_while_the_head_keeps_facing_the_mouth() {
        let mut heights = Vec::new();
        let mut angles = Vec::new();
        for frame in 0..600 {
            let (head, angle) = microphone_pose(frame as f32 / 60.0);
            let tip = [head[0] + 7.0 * angle.cos(), head[1] + 7.0 * angle.sin()];
            let distance = |p: [f32; 2]| ((p[0] + 0.8).powi(2) + (p[1] - 20.7).powi(2)).sqrt();
            assert!(distance(head) < distance(tip), "麦头应在嘴边，麦柄朝外");
            assert!(
                head[0] - 2.6 > 1.4 && head[1] - 2.6 > 17.0,
                "不能遮住嘴或眼睛"
            );
            assert!(
                tip[0] + 1.0 < 16.0 && tip[1] + 1.0 < 28.0,
                "摆动不能超出常驻槽位"
            );
            heights.push(head[1]);
            angles.push(angle);
        }
        let range = |values: Vec<f32>| {
            values.iter().copied().fold(f32::NEG_INFINITY, f32::max)
                - values.iter().copied().fold(f32::INFINITY, f32::min)
        };
        assert!((0.6..1.1).contains(&range(heights)), "麦头应有细微起伏");
        assert!((0.10..0.18).contains(&range(angles)), "麦柄应有轻微转动");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MusicPose {
    Microphone,
    Hum,
    SideSing,
    Squint,
    HighNote,
    Smile,
}

const SHAKE_SWING: f32 = 40.0;
const SHAKE_MAX_GAP: f64 = 0.8;
const SHAKE_DURATION: f64 = 5.0;
const SHAKE_REVERSALS: u32 = 8;

/// 只保留当前摆动的极值，不记录或保存鼠标轨迹。
#[derive(Default)]
pub(crate) struct MouseShake {
    anchor: Option<f32>,
    direction: i8,
    extreme: f32,
    started_at: Option<f64>,
    last_reversal: f64,
    last_sample: Option<f64>,
    reversals: u32,
    triggered: bool,
}

impl MouseShake {
    pub(crate) fn observe(&mut self, x: Option<f32>, now: f64) -> bool {
        let Some(x) = x.filter(|x| x.is_finite() && now.is_finite()) else {
            *self = Self::default();
            return false;
        };
        if self
            .last_sample
            .is_some_and(|last| now < last || now - last > SHAKE_MAX_GAP)
            || self
                .started_at
                .is_some_and(|_| now - self.last_reversal > SHAKE_MAX_GAP)
        {
            *self = Self::default();
        }
        self.last_sample = Some(now);

        if self.direction == 0 {
            let anchor = *self.anchor.get_or_insert(x);
            let delta = x - anchor;
            if delta.abs() >= SHAKE_SWING {
                self.direction = if delta > 0.0 { 1 } else { -1 };
                self.extreme = x;
                self.started_at = Some(now);
                self.last_reversal = now;
            }
        } else if self.direction > 0 {
            if x > self.extreme {
                self.extreme = x;
            } else if self.extreme - x >= SHAKE_SWING {
                self.direction = -1;
                self.extreme = x;
                self.last_reversal = now;
                self.reversals += 1;
            }
        } else if x < self.extreme {
            self.extreme = x;
        } else if x - self.extreme >= SHAKE_SWING {
            self.direction = 1;
            self.extreme = x;
            self.last_reversal = now;
            self.reversals += 1;
        }

        if !self.triggered
            && self.reversals >= SHAKE_REVERSALS
            && self
                .started_at
                .is_some_and(|start| now - start >= SHAKE_DURATION)
        {
            self.triggered = true;
            return true;
        }
        false
    }
}

const MUSIC_PERIOD: f32 = 16.0;
const MUSIC_EXPRESSIONS: [Expression; 12] = [
    Expression::Wink,
    Expression::Delighted,
    Expression::Smug,
    Expression::Blush,
    Expression::Heart,
    Expression::Scan,
    Expression::SideGlance,
    Expression::Giggle,
    Expression::Praised,
    Expression::Peek,
    Expression::ShyPeek,
    Expression::Search,
];

struct MusicSegment {
    pose: MusicPose,
    elapsed: f32,
    expression: Option<(Expression, f32)>,
}

fn music_segment_at(age: f32) -> MusicSegment {
    let cycle = MUSIC_EXPRESSIONS
        .iter()
        .map(|expression| MUSIC_PERIOD + expression.duration())
        .sum::<f32>();
    let mut time = age % cycle;
    for (index, expression) in MUSIC_EXPRESSIONS.into_iter().enumerate() {
        let pose = match index % 6 {
            0 => MusicPose::Microphone,
            1 => MusicPose::Hum,
            2 => MusicPose::SideSing,
            3 => MusicPose::Squint,
            4 => MusicPose::HighNote,
            _ => MusicPose::Smile,
        };
        if time < MUSIC_PERIOD {
            return MusicSegment {
                pose,
                elapsed: time,
                expression: None,
            };
        }
        time -= MUSIC_PERIOD;
        if time < expression.duration() {
            return MusicSegment {
                pose,
                elapsed: MUSIC_PERIOD,
                expression: Some((expression, time)),
            };
        }
        time -= expression.duration();
    }
    MusicSegment {
        pose: MusicPose::Microphone,
        elapsed: 0.0,
        expression: None,
    }
}

fn microphone_pose(age: f32) -> ([f32; 2], f32) {
    (
        [
            5.9 + (age * 0.95).sin() * 0.2,
            20.8 + (age * 1.35).sin() * 0.45,
        ],
        35.0_f32.to_radians() + (age * 1.1).sin() * 0.07,
    )
}

fn microphone(c: &Card, cx: f32, y: f32, age: f32) {
    let age = if crate::bar_layout::animations_enabled() {
        age
    } else {
        0.0
    };
    let (head, angle) = microphone_pose(age);
    let point = |s: f32, v: f32| {
        (
            cx + head[0] + s * angle.cos() - v * angle.sin(),
            y + head[1] + s * angle.sin() + v * angle.cos(),
        )
    };
    let capsule = |start: f32, end: f32, width: f32, color: Color32| {
        let a = point(start, 0.0);
        let b = point(end, 0.0);
        c.seg(a, b, width, color);
        c.circle(a.0, a.1, width / 2.0, color);
        c.circle(b.0, b.1, width / 2.0, color);
    };
    let line = |a: (f32, f32), b: (f32, f32), width: f32, color: Color32| {
        c.seg(point(a.0, a.1), point(b.0, b.1), width, color)
    };
    capsule(2.3, 7.0, 1.9, Color32::from_rgb(176, 183, 197));
    capsule(2.5, 6.8, 1.45, Color32::from_rgb(76, 84, 101));
    line(
        (3.4, -0.53),
        (6.4, -0.53),
        0.28,
        Color32::from_rgb(218, 225, 237),
    );
    capsule(2.05, 2.65, 2.6, Color32::from_rgb(126, 137, 157));
    let head = point(0.0, 0.0);
    c.circle(head.0, head.1, 2.6, Color32::from_rgb(222, 228, 238));
    c.circle(head.0, head.1, 2.15, Color32::from_rgb(164, 176, 194));
    for s in [-1.0f32, 0.0, 1.0] {
        let v = (4.0 - s * s).sqrt() * 0.88;
        line((s, -v), (s, v), 0.32, Color32::from_rgb(92, 105, 126));
    }
    for v in [-1.0f32, 0.0, 1.0] {
        let s = (4.0 - v * v).sqrt() * 0.88;
        line((-s, v), (s, v), 0.3, Color32::from_rgb(226, 232, 243));
    }
}

#[derive(Clone, Copy)]
struct Eye {
    height: f32,
    spread: f32,
    curve: f32,
    y: f32,
    tilt: f32,
}

impl Eye {
    fn open(height: f32) -> Self {
        Self {
            height,
            spread: 0.0,
            curve: 0.0,
            y: 0.0,
            tilt: 0.0,
        }
    }
    fn arc(spread: f32, curve: f32) -> Self {
        Self {
            spread,
            curve,
            ..Self::open(0.0)
        }
    }
    fn follow(&mut self, goal: Self, dt: f32) {
        self.height = approach(self.height, goal.height, 17.0, dt);
        self.spread = approach(self.spread, goal.spread, 17.0, dt);
        self.curve = approach(self.curve, goal.curve, 17.0, dt);
        self.y = approach(self.y, goal.y, 12.0, dt);
        self.tilt = approach(self.tilt, goal.tilt, 12.0, dt);
    }
}

pub(crate) struct Pet {
    pub state: PetState,
    pub reaction: Option<(PetState, f32)>,
    pub waking_until: f64,
    pub last_idle: f64,
    pub hovered: bool,
    pub worried: bool,
    pub pull: Option<(f32, f32)>,
    pub pull_offset: f32,
    pub expressions: crate::pet_expressions::Expressions,
    shake: MouseShake,
    age: f32,
    music_age: f32,
    weather_key: Option<(crate::weather::Kind, bool)>,
    weather_pending: bool,
    weather_clip: Option<((crate::weather::Kind, bool), f32)>,
    weather_idle: f32,
    eyes: [Eye; 2],
    gaze: [f32; 2],
    y: f32,
}

impl Default for Pet {
    fn default() -> Self {
        Self {
            state: PetState::Awake,
            reaction: None,
            waking_until: 0.0,
            last_idle: 0.0,
            hovered: false,
            worried: false,
            pull: None,
            pull_offset: 0.0,
            expressions: Default::default(),
            shake: MouseShake::default(),
            age: 0.0,
            music_age: 0.0,
            weather_key: None,
            weather_pending: false,
            weather_clip: None,
            weather_idle: 0.0,
            eyes: [Eye::open(4.4); 2],
            gaze: [0.0; 2],
            y: 0.0,
        }
    }
}

impl Pet {
    pub fn react(&mut self, state: PetState) {
        self.reaction = Some((state, self.expressions.react(state)));
        self.age = 0.0;
    }
    pub fn observe_shake(&mut self, x: Option<f32>, now: f64) -> bool {
        self.shake.observe(x, now)
    }
    /// 反馈只在实际可见时计时，避免用户还在面板里操作时就播完。
    pub fn advance(&mut self, dt: f32, visible: bool) {
        if visible {
            if let Some((_, remaining)) = &mut self.reaction {
                *remaining -= dt;
                if *remaining <= 0.0 {
                    self.reaction = None;
                }
            }
        }
    }
    pub fn music_pose(&self) -> MusicPose {
        music_segment_at(self.music_age).pose
    }

    /// Weather reactions yield to existing states; their clocks run only while visible and idle.
    pub fn update_weather(
        &mut self,
        key: Option<(crate::weather::Kind, bool)>,
        state: PetState,
        dt: f32,
        visible: bool,
        animations: bool,
        blocked: bool,
    ) {
        if key != self.weather_key {
            self.weather_key = key;
            self.weather_pending = key.is_some();
            self.weather_clip = None;
            self.weather_idle = 0.0;
        }
        if key.is_none() {
            self.weather_pending = false;
            self.weather_clip = None;
            self.weather_idle = 0.0;
            return;
        }
        if !animations {
            self.weather_clip = None;
            return;
        }
        if !visible {
            return;
        }
        if state != PetState::Awake || blocked {
            self.weather_clip = None;
            return;
        }
        if let Some((_, age)) = &mut self.weather_clip {
            *age += dt;
            if *age >= 4.0 {
                self.weather_clip = None;
                self.weather_idle = 0.0;
            }
            return;
        }
        self.weather_idle += dt;
        if (self.weather_pending || self.weather_idle >= 300.0)
            && self.expressions.current().is_none()
        {
            self.weather_clip = key.map(|weather| (weather, 0.0));
            self.weather_pending = false;
            self.weather_idle = 0.0;
        }
    }

    pub fn step(&mut self, state: PetState, t: f64, dt: f32, gaze: [f32; 2], visible: bool) {
        if self.state == PetState::Music && matches!(state, PetState::Awake | PetState::Curious) {
            self.expressions.clear();
        }
        if self.weather_clip.is_none() {
            self.expressions
                .step(state, dt, visible, crate::bar_layout::animations_enabled());
        }
        if state != self.state {
            self.state = state;
            self.age = 0.0;
        }
        if visible {
            self.age += dt;
        }
        if state == PetState::Music && visible {
            self.music_age += dt;
            if let Some((expression, time)) = music_segment_at(self.music_age).expression {
                self.expressions.play_at(expression, time);
            }
        }
        let age = self.age;
        let mut eyes = [Eye::open(4.4); 2];
        let mut y = (t as f32 * 1.7).sin() * 0.35;
        let mut look = gaze;
        match state {
            PetState::Awake => {}
            PetState::Curious => {
                eyes[0].height = 5.6;
                eyes[1].height = 6.3;
                eyes[0].y = 0.6;
                eyes[1].y = -0.6;
            }
            PetState::Waking => {
                eyes[0].height = lerp(0.0, 4.4, smoothstep(0.1, 0.65, age));
                eyes[1].height = lerp(0.0, 4.4, smoothstep(0.4, 1.1, age));
                y = 1.0 - smoothstep(0.0, 1.2, age);
                look = [0.0; 2];
            }
            PetState::Happy => {
                eyes = [Eye::arc(2.6, -1.8); 2];
                y = -(age * 7.5).sin().abs() * 2.0 * (-age * 0.65).exp();
            }
            PetState::Satisfied => {
                eyes = [Eye::arc(2.2, -1.2); 2];
                y = (age * 8.0).sin() * 1.2 * (-age * 0.8).exp();
            }
            PetState::Dizzy => {
                look = [(age * 4.0).sin() * 0.7, 0.0];
                y = (age * 3.5).sin() * 0.55;
            }
            PetState::Pulling => {
                let (direction, phase) = self.pull.unwrap_or((1.0, 0.0));
                eyes = [Eye::open(3.0); 2];
                eyes[0].tilt = -direction * 0.35;
                eyes[1].tilt = -direction * 0.35;
                look = [direction * 0.7, 0.0];
                y = (phase * 6.0).sin() * 0.45;
            }
            PetState::Music => {
                let age = self.music_age;
                let sway = (age * 1.4).sin();
                look = [0.0, 0.0];
                y = -0.5 + sway * 0.25;
                match self.music_pose() {
                    MusicPose::Microphone => {
                        eyes = [Eye::arc(2.5, 0.7); 2];
                    }
                    MusicPose::SideSing => {
                        eyes = [Eye::open(1.6); 2];
                        eyes[0].y = -0.5;
                        eyes[1].y = 0.5;
                        look = [0.4, 0.0];
                        y = sway * 0.35;
                    }
                    MusicPose::HighNote => {
                        eyes = [Eye::arc(2.3, -0.7); 2];
                        eyes[0].tilt = -0.3;
                        eyes[1].tilt = 0.3;
                        y = -1.5 + sway * 0.2;
                    }
                    MusicPose::Hum => {
                        eyes = [Eye::arc(2.5, 0.7); 2];
                        y = sway * 0.3;
                    }
                    MusicPose::Squint => {
                        eyes = [Eye::arc(1.6, 0.0); 2];
                        y = sway * 0.45;
                    }
                    MusicPose::Smile => {
                        eyes = [Eye::arc(2.3, -0.8); 2];
                        eyes[0].y = -0.5;
                        eyes[1].y = 0.5;
                        look = [sway * 0.5, 0.0];
                    }
                }
            }
            PetState::Worried => {
                eyes[0] = Eye {
                    spread: -0.6,
                    tilt: -0.7,
                    y: 0.4,
                    ..Eye::open(2.8)
                };
                eyes[1] = Eye {
                    spread: 0.6,
                    tilt: 0.7,
                    y: 1.0,
                    ..Eye::open(1.8)
                };
            }
            PetState::Focused => {
                eyes = [Eye::open(1.8); 2];
                look = [gaze[0] * 0.2, 0.0];
                y = 0.0;
            }
            PetState::Dozing => {
                eyes = [Eye::open(0.6); 2];
                y = 1.0;
                look = [0.0; 2];
            }
            PetState::Sleeping => {
                eyes = [Eye::arc(2.4, 1.0); 2];
                y = 0.8 + (t as f32 * 1.2).sin() * 0.4;
                look = [0.0; 2];
            }
        }
        if state == PetState::Awake {
            if let Some(((kind, _), weather_age)) = self.weather_clip {
                use crate::weather::Kind;
                match kind {
                    Kind::Clear => {
                        eyes = [Eye::arc(2.4, -1.0); 2];
                        look = [0.0, 0.0];
                        y = (weather_age * 2.0).sin() * 0.25;
                    }
                    Kind::PartlyCloudy => {
                        eyes = [Eye::open(4.0); 2];
                        look = [(weather_age * 1.7).sin() * 1.3, -0.4];
                        y = (weather_age * 1.5).sin() * 0.5;
                    }
                    Kind::Cloudy => {
                        eyes = [Eye::open(3.5); 2];
                        look = [0.3, -1.0];
                    }
                    Kind::Fog => {
                        eyes = [Eye::open(5.0); 2];
                        look = [(weather_age * 2.0).sin() * 1.8, 0.0];
                    }
                    Kind::Rain => {
                        eyes = [Eye::open(if (weather_age * 2.8).sin() > 0.9 {
                            0.5
                        } else {
                            4.0
                        }); 2];
                        look = [1.0, -0.4];
                    }
                    Kind::Snow => {
                        eyes = [Eye::arc(2.5, -1.4); 2];
                        look = [0.0, 0.0];
                        y = (weather_age * 2.2).sin() * 0.4;
                    }
                    Kind::Thunder => {
                        eyes = [Eye::open(6.2); 2];
                        look = [0.4, -0.6];
                        y = if weather_age < 0.7 {
                            -(weather_age * 9.0).sin().abs()
                        } else {
                            0.0
                        };
                    }
                }
            }
        }
        let expression_active = matches!(
            state,
            PetState::Awake
                | PetState::Curious
                | PetState::Happy
                | PetState::Satisfied
                | PetState::Music
        ) && self.expressions.current().is_some();
        if expression_active {
            eyes = [Eye::open(4.4); 2];
            look = [0.0; 2];
            y = 0.0;
        }
        if !expression_active
            && self.weather_clip.is_none()
            && matches!(
                state,
                PetState::Awake | PetState::Curious | PetState::Focused | PetState::Worried
            )
        {
            let start = 2.0 + crate::anim::hash01((t / 5.6).floor() as u64) * 2.7;
            let blink = ((t % 5.6) as f32 - start) / 0.18;
            if (0.0..1.0).contains(&blink) {
                for eye in &mut eyes {
                    eye.height *= 1.0 - (blink * std::f32::consts::PI).sin();
                }
            }
        }
        let eye_dt = if state == PetState::Music {
            dt * 0.2
        } else {
            dt
        };
        for (eye, goal) in self.eyes.iter_mut().zip(eyes) {
            eye.follow(goal, eye_dt);
        }
        for (axis, goal) in self.gaze.iter_mut().zip(look) {
            *axis = approach(*axis, goal, 10.0, dt);
        }
        self.y = approach(self.y, y, 14.0, dt);
    }

    pub fn draw(&mut self, c: &Card, cx: f32, t: f64, light: bool, pull_top: f32) {
        if matches!(
            self.state,
            PetState::Awake
                | PetState::Curious
                | PetState::Happy
                | PetState::Satisfied
                | PetState::Music
        ) && self.expressions.draw(c, cx, light)
        {
            return;
        }
        if self.state == PetState::Dizzy {
            for dx in [-5.6, 5.6] {
                let x = cx + dx + self.gaze[0];
                let y = 14.5 + self.y;
                let points: Vec<_> = (0..=40)
                    .map(|n| {
                        let q = n as f32 / 40.0;
                        let angle = q * std::f32::consts::PI * 3.4 + self.age * 3.0;
                        let radius = 0.35 + q * 2.8;
                        (x + angle.cos() * radius, y + angle.sin() * radius)
                    })
                    .collect();
                c.path(&points, 1.0, theme::WHITE);
            }
        } else {
            for (i, eye) in self.eyes.iter().enumerate() {
                let x = cx + if i == 0 { -5.6 } else { 5.6 } + self.gaze[0];
                let y = 14.5 + self.y + eye.y + self.gaze[1];
                let points: Vec<_> = (0..=12)
                    .map(|n| {
                        let q = n as f32 / 6.0 - 1.0;
                        (
                            x + q * eye.spread,
                            y + q * (eye.height / 2.0 + eye.tilt) + (1.0 - q * q) * eye.curve,
                        )
                    })
                    .collect();
                let stroke = if self.state == PetState::Music {
                    3.6 - 0.8 * smoothstep(0.5, 1.5, eye.spread.abs())
                } else {
                    3.6
                };
                c.path(&points, stroke, theme::WHITE);
                // 圆端点保持胶囊眼睛的轮廓。
                for p in [points[0], points[12]] {
                    c.circle(p.0, p.1, stroke / 2.0, theme::WHITE);
                }
            }
        }
        match self.state {
            PetState::Dozing | PetState::Sleeping => {
                let bob = ((t as f32 * 1.4).sin() + 1.0) * 0.7;
                c.text(
                    cx + 14.0,
                    13.0 - bob,
                    "z",
                    7.0,
                    W::Bold,
                    Color32::from_rgb(170, 160, 255),
                    H::Left,
                );
                if self.state == PetState::Sleeping {
                    c.text(
                        cx + 19.0,
                        9.0 - bob,
                        "z",
                        5.5,
                        W::Bold,
                        Color32::from_rgb(170, 160, 255),
                        H::Left,
                    );
                }
            }
            PetState::Happy => {
                let k = smoothstep(0.0, 0.2, self.age) * (1.0 - smoothstep(1.5, 2.5, self.age));
                let c = c.faded(k);
                for (dx, y, r) in [(-18.0, 9.0, 2.1), (18.0, 8.0, 2.8)] {
                    c.seg((cx + dx - r, y), (cx + dx + r, y), 1.0, theme::GREEN);
                    c.seg((cx + dx, y - r), (cx + dx, y + r), 1.0, theme::GREEN);
                }
            }
            PetState::Satisfied => crate::paint::droplet(c, cx + 18.0, 12.0, 5.0, theme::CYAN),
            PetState::Pulling => {
                let (direction, phase) = self.pull.unwrap_or((1.0, 0.0));
                let reach = smoothstep(0.0, 0.5, phase);
                let c = c.faded(reach);
                let stride = 8.0;
                for (i, side) in [-1.0_f32, 1.0].into_iter().enumerate() {
                    // 抓住时抵消窗口位移，松手时再向前伸出；两只手交替。
                    let step = (direction * self.pull_offset / stride + i as f32).rem_euclid(2.0);
                    let (shift, lift) = if step < 1.0 {
                        (0.5 - step, 0.0)
                    } else {
                        (
                            step - 1.5,
                            ((step - 1.0) * std::f32::consts::PI).sin() * 2.2,
                        )
                    };
                    let hand_x = cx + side * 15.5 + direction * shift * stride;
                    let hand_y = lerp(10.0, pull_top + 1.5 + lift, reach);
                    c.path(
                        &[
                            (cx + side * 10.5, 17.0 + self.y),
                            (cx + side * 13.0, 11.0 + self.y),
                            (hand_x, hand_y + 2.0),
                        ],
                        1.5,
                        theme::WHITE,
                    );
                    c.circle(hand_x, hand_y + 1.2, 1.5, theme::WHITE);
                    for finger in [-1.2_f32, 0.0, 1.2] {
                        c.seg(
                            (hand_x + finger, hand_y + 0.5),
                            (hand_x + finger, hand_y - 1.5),
                            0.8,
                            theme::WHITE,
                        );
                    }
                }
            }
            PetState::Dizzy => {
                for (dx, y, r) in [(-16.0, 9.0, 2.2), (16.0, 8.0, 2.6)] {
                    let bob = (self.age * 3.0 + dx).sin() * 0.6;
                    c.seg(
                        (cx + dx - r, y + bob),
                        (cx + dx + r, y + bob),
                        1.0,
                        theme::YELLOW,
                    );
                    c.seg(
                        (cx + dx, y + bob - r),
                        (cx + dx, y + bob + r),
                        1.0,
                        theme::YELLOW,
                    );
                }
            }
            PetState::Music => {
                let segment = music_segment_at(self.music_age);
                self.draw_music(
                    &c.faded(smoothstep(0.0, 1.0, segment.elapsed)),
                    cx,
                    segment.pose,
                );
            }
            PetState::Worried => {
                c.fill(cx + 17.0, 8.0, 1.6, 4.2, 0.8, theme::YELLOW);
                c.circle(cx + 17.8, 14.5, 0.9, theme::YELLOW);
            }
            _ => {}
        }
        if self.state == PetState::Awake {
            if let Some(((kind, day), age)) = self.weather_clip {
                self.draw_weather(c, cx, kind, day, age);
            }
        }
    }

    fn draw_weather(&self, c: &Card, cx: f32, kind: crate::weather::Kind, day: bool, age: f32) {
        use crate::weather::Kind;
        let fade = smoothstep(0.0, 0.25, age) * (1.0 - smoothstep(3.55, 4.0, age));
        let c = &c.faded(fade);
        let gold = Color32::from_rgb(255, 190, 45);
        let moon = Color32::from_rgb(238, 207, 142);
        let cloud = Color32::from_rgb(161, 190, 229);
        let rain = Color32::from_rgb(85, 169, 238);
        let snow = Color32::from_rgb(145, 209, 249);
        let sun_or_moon = |c: &Card, x: f32, y: f32| {
            if day {
                c.circle(x, y, 2.0, gold);
                for n in 0..8 {
                    let a = n as f32 * std::f32::consts::PI / 4.0;
                    c.seg(
                        (x + a.cos() * 3.0, y + a.sin() * 3.0),
                        (x + a.cos() * 4.0, y + a.sin() * 4.0),
                        0.8,
                        gold,
                    );
                }
            } else {
                c.path(
                    &[
                        (x + 1.8, y - 3.1),
                        (x - 0.8, y - 3.0),
                        (x - 2.4, y - 1.3),
                        (x - 2.5, y + 1.1),
                        (x - 0.8, y + 3.0),
                        (x + 1.8, y + 2.6),
                        (x + 0.2, y + 1.0),
                        (x - 0.1, y - 1.0),
                        (x + 1.8, y - 3.1),
                    ],
                    1.0,
                    moon,
                );
            }
        };
        let cloudlet = |c: &Card, x: f32, y: f32| {
            c.path(
                &[
                    (x - 3.0, y + 1.5),
                    (x - 3.7, y + 0.5),
                    (x - 3.3, y - 0.8),
                    (x - 1.9, y - 1.2),
                    (x - 1.1, y - 2.6),
                    (x + 0.6, y - 2.9),
                    (x + 2.1, y - 1.6),
                    (x + 2.3, y - 0.7),
                    (x + 3.4, y - 0.4),
                    (x + 3.8, y + 0.8),
                    (x + 3.0, y + 1.5),
                    (x - 3.0, y + 1.5),
                ],
                1.0,
                cloud,
            );
        };
        match kind {
            Kind::Clear => {
                sun_or_moon(c, cx + 12.0, 7.0 + (age * 1.8).sin() * 0.4);
                c.path(
                    &[(cx - 2.2, 21.0), (cx, 22.0), (cx + 2.2, 21.0)],
                    0.9,
                    theme::WHITE,
                );
            }
            Kind::PartlyCloudy => {
                sun_or_moon(c, cx + 11.5, 5.5);
                cloudlet(c, cx + 12.0, 10.0 + (age * 1.5).sin() * 0.5);
            }
            Kind::Cloudy => cloudlet(c, cx + 12.0, 8.0 + (age * 1.6).sin() * 0.8),
            Kind::Fog => {
                let drift = (age * 1.6).sin() * 0.7;
                for (y, dx) in [(7.0, 0.0), (21.5, 0.6)] {
                    c.seg(
                        (cx - 15.0 + drift + dx, y),
                        (cx - 10.0 + drift + dx, y),
                        1.0,
                        cloud,
                    );
                    c.seg(
                        (cx + 10.0 - drift - dx, y),
                        (cx + 15.0 - drift - dx, y),
                        1.0,
                        cloud,
                    );
                }
            }
            Kind::Rain => {
                cloudlet(c, cx + 12.0, 6.0);
                for i in 0..2 {
                    let y = 10.0 + (age * 5.0 + i as f32 * 4.0).rem_euclid(9.0);
                    c.seg(
                        (cx + 11.0 + i as f32 * 3.0, y),
                        (cx + 10.4 + i as f32 * 3.0, y + 2.0),
                        1.0,
                        rain,
                    );
                }
            }
            Kind::Snow => {
                for i in 0..2 {
                    let x = cx + 11.0 + i as f32 * 3.0 + (age * 1.5 + i as f32).sin() * 0.5;
                    let y = 6.0 + (age * 2.5 + i as f32 * 6.0).rem_euclid(14.0);
                    c.seg((x - 1.0, y), (x + 1.0, y), 0.8, snow);
                    c.seg((x, y - 1.0), (x, y + 1.0), 0.8, snow);
                }
                c.path(
                    &[(cx - 2.2, 21.0), (cx, 22.0), (cx + 2.2, 21.0)],
                    0.9,
                    theme::WHITE,
                );
            }
            Kind::Thunder => {
                cloudlet(c, cx + 12.0, 6.0);
                let flash = if (age * 2.5).sin() > 0.0 { 1.0 } else { 0.55 };
                c.faded(flash).path(
                    &[
                        (cx + 13.0, 8.0),
                        (cx + 10.8, 12.0),
                        (cx + 13.2, 12.0),
                        (cx + 11.4, 16.0),
                    ],
                    1.2,
                    gold,
                );
                let y = 13.0 + (age * 4.0).rem_euclid(7.0);
                c.seg((cx + 14.5, y), (cx + 14.0, y + 1.5), 0.9, rain);
            }
        }
    }

    fn draw_music(&self, c: &Card, cx: f32, pose: MusicPose) {
        let y = self.y + self.gaze[1];
        let pulse = (self.music_age * 2.0).sin() * 0.5 + 0.5;
        let mouth = |rx: f32, ry: f32| {
            let points: Vec<_> = (0..=24)
                .map(|n| {
                    let angle = n as f32 / 24.0 * std::f32::consts::TAU;
                    (cx - 0.8 + angle.cos() * rx, 20.7 + y + angle.sin() * ry)
                })
                .collect();
            c.path(&points, 0.9, theme::WHITE);
        };
        match pose {
            MusicPose::Microphone => mouth(1.1, 0.8 + pulse * 0.5),
            MusicPose::SideSing => mouth(1.5, 0.8 + pulse * 0.3),
            MusicPose::HighNote => mouth(1.7, 1.6 + pulse * 0.5),
            MusicPose::Hum => c.path(
                &[
                    (cx - 2.7, 20.7 + y),
                    (cx - 1.8, 21.4 + y),
                    (cx - 0.8, 21.0 + y),
                    (cx + 0.2, 21.4 + y),
                    (cx + 1.1, 20.7 + y),
                ],
                0.9,
                theme::WHITE,
            ),
            MusicPose::Squint => mouth(1.2, 0.4 + pulse * 0.6),
            MusicPose::Smile => c.path(
                &[
                    (cx - 2.5, 20.6 + y),
                    (cx - 1.6, 21.6 + y),
                    (cx - 0.5, 21.9 + y),
                    (cx + 0.6, 21.6 + y),
                    (cx + 1.5, 20.6 + y),
                ],
                1.0,
                theme::WHITE,
            ),
        }
        if matches!(
            pose,
            MusicPose::Microphone | MusicPose::SideSing | MusicPose::HighNote
        ) {
            microphone(c, cx, y, self.music_age);
        } else {
            let y = 8.5 + (self.music_age * 1.4).sin() * 0.6;
            c.seg(
                (cx + 16.0, y - 3.0),
                (cx + 16.0, y + 3.0),
                1.1,
                theme::WHITE,
            );
            c.seg(
                (cx + 16.0, y - 3.0),
                (cx + 19.0, y - 2.0),
                1.1,
                theme::WHITE,
            );
            c.circle(cx + 14.8, y + 3.0, 1.5, theme::WHITE);
        }
    }
}
