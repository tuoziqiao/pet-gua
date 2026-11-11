//! 表情图集及日常互动调度；计时只在吉祥物可见时推进。
use crate::paint::Card;
use crate::pet::PetState;
use eframe::egui::{self, Color32, Rect, TextureHandle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Expression {
    Wink,
    Smug,
    Blush,
    Delighted,
    SideGlance,
    Praised,
    Heart,
    Giggle,
    ShyPeek,
    Scan,
    Peek,
    Search,
}

impl Expression {
    pub fn label(self) -> &'static str {
        match self {
            Self::Wink => "眨眼", Self::Smug => "得意", Self::Blush => "脸红",
            Self::Delighted => "开心", Self::SideGlance => "侧目", Self::Praised => "被夸",
            Self::Heart => "比心", Self::Giggle => "偷笑", Self::ShyPeek => "害羞探头",
            Self::Scan => "巡视", Self::Peek => "探头", Self::Search => "左右张望",
        }
    }
    pub const ALL: [Self; 12] = [
        Self::Wink,
        Self::Smug,
        Self::Blush,
        Self::Delighted,
        Self::SideGlance,
        Self::Praised,
        Self::Heart,
        Self::Giggle,
        Self::ShyPeek,
        Self::Scan,
        Self::Peek,
        Self::Search,
    ];

    fn times(self) -> [f32; 16] {
        if self as usize >= 9 {
            [
                0.35, 0.13, 0.15, 0.18, 0.35, 0.28, 0.16, 0.11, 0.11, 0.16, 0.35, 0.28, 0.16, 0.14,
                0.20, 0.35,
            ]
        } else {
            [
                0.35, 0.12, 0.12, 0.12, 0.13, 0.13, 0.14, 0.40, 0.18, 0.14, 0.12, 0.12, 0.12, 0.14,
                0.14, 0.45,
            ]
        }
    }

    pub fn duration(self) -> f32 {
        self.times().iter().sum()
    }

    fn frame(self, time: f32, animations: bool) -> usize {
        if !animations {
            return 7;
        }
        let mut end = 0.0;
        for (index, seconds) in self.times().into_iter().enumerate() {
            end += seconds;
            if time < end {
                return index;
            }
        }
        15
    }
}

pub(crate) struct Expressions {
    clip: Option<(Expression, f32)>,
    wait: f32,
    cooldown: f32,
    hovered: bool,
    next: [usize; 4],
    atlas: Option<TextureHandle>,
    light_atlas: Option<TextureHandle>,
}

impl Default for Expressions {
    fn default() -> Self {
        Self {
            clip: None,
            wait: 8.0,
            cooldown: 0.0,
            hovered: false,
            next: [0; 4],
            atlas: None,
            light_atlas: None,
        }
    }
}

impl Expressions {
    pub fn current(&self) -> Option<Expression> {
        self.clip.map(|clip| clip.0)
    }
    pub fn play(&mut self, expression: Expression) {
        self.clip = Some((expression, 0.0));
    }
    pub fn play_at(&mut self, expression: Expression, time: f32) {
        self.clip = Some((expression, time));
    }

    pub fn clear(&mut self) {
        self.clip = None;
        self.wait = 8.0;
        self.cooldown = 0.0;
        self.hovered = false;
    }

    fn choose(&mut self, bucket: usize, pool: &[Expression]) -> Expression {
        let expression = pool[self.next[bucket] % pool.len()];
        self.next[bucket] = self.next[bucket].wrapping_add(1);
        expression
    }

    pub fn react(&mut self, state: PetState) -> f32 {
        use Expression::*;
        let expression = match state {
            PetState::Happy => self.choose(2, &[Praised, Smug, Delighted, Giggle]),
            PetState::Satisfied => self.choose(3, &[Heart, Delighted]),
            _ => return 2.6,
        };
        self.play(expression);
        expression.duration()
    }

    pub fn step(&mut self, state: PetState, dt: f32, visible: bool, animations: bool) {
        use Expression::*;
        if !visible {
            return;
        }
        if !matches!(
            state,
            PetState::Awake | PetState::Curious | PetState::Happy | PetState::Satisfied
        ) {
            self.clear();
            return;
        }
        self.cooldown = (self.cooldown - dt).max(0.0);
        if let Some((expression, time)) = &mut self.clip {
            *time += dt;
            if *time >= expression.duration() {
                self.clip = None;
                self.wait = 22.0 + (self.next[0] % 4) as f32 * 3.0;
            }
        }
        let hovered = state == PetState::Curious;
        if hovered && !self.hovered && self.cooldown <= 0.0 && self.clip.is_none() {
            let expression = self.choose(1, &[Wink, Blush, SideGlance, ShyPeek]);
            self.play(expression);
            self.cooldown = 18.0;
        }
        self.hovered = hovered;
        if state == PetState::Awake && self.clip.is_none() && animations {
            self.wait -= dt;
            if self.wait <= 0.0 {
                let expression = self.choose(
                    0,
                    &[
                        Scan, Delighted, Peek, Smug, Search, Giggle, Praised, ShyPeek,
                    ],
                );
                self.play(expression);
            }
        }
    }

    pub fn draw(&mut self, c: &Card, cx: f32, light: bool) -> bool {
        let Some((expression, time)) = self.clip else {
            return false;
        };
        let atlas = if light {
            &mut self.light_atlas
        } else {
            &mut self.atlas
        };
        let atlas = atlas.get_or_insert_with(|| {
            let mut decoded =
                image::load_from_memory(include_bytes!("../assets/pet/expressions.png"))
                    .expect("内置表情图集")
                    .into_rgba8();
            if light {
                for pixel in decoded.pixels_mut() {
                    let [r, g, b, a] = pixel.0;
                    if a > 0 && r.max(g).max(b) - r.min(g).min(b) < 24 {
                        let value = 255u8.saturating_sub(r).saturating_add(22);
                        pixel.0 = [value, value, value, a];
                    }
                }
            }
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [decoded.width() as usize, decoded.height() as usize],
                decoded.as_raw(),
            );
            let texture = c.ui.ctx().load_texture(
                if light {
                    "gua-expressions-light"
                } else {
                    "gua-expressions"
                },
                image,
                egui::TextureOptions::LINEAR,
            );
            crate::skin_paint::mark_colored(texture.id());
            texture
        });
        let index = expression as usize;
        let frame = expression.frame(time, crate::bar_layout::animations_enabled());
        let columns = Expression::ALL.len() as f32;
        let uv = Rect::from_min_max(
            egui::pos2(index as f32 / columns, frame as f32 / 16.0),
            egui::pos2((index + 1) as f32 / columns, (frame + 1) as f32 / 16.0),
        );
        c.painter.image(
            atlas.id(),
            c.rect(cx - 24.0, 0.0, 48.0, 30.0),
            uv,
            Color32::WHITE,
        );
        true
    }
}
