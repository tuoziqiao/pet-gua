use crate::paint::{self, Card};
use eframe::egui::{self, Color32};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Sun,
    Moon,
    Partly,
    PartlyNight,
    Cloud,
    Fog,
    Drizzle,
    Rain,
    Shower,
    Snow,
    Thunder,
}
pub(super) fn kind(code: i64, day: bool) -> Kind {
    match code {
        0 => {
            if day {
                Kind::Sun
            } else {
                Kind::Moon
            }
        }
        1 | 2 => {
            if day {
                Kind::Partly
            } else {
                Kind::PartlyNight
            }
        }
        45 | 48 => Kind::Fog,
        51 | 53 | 55 | 56 | 57 => Kind::Drizzle,
        61 | 63 | 65 | 66 | 67 => Kind::Rain,
        80..=82 => Kind::Shower,
        71..=77 | 85 | 86 => Kind::Snow,
        95..=99 => Kind::Thunder,
        _ => Kind::Cloud,
    }
}
const SUN: Color32 = Color32::from_rgb(255, 196, 72);
const SUN_CORE: Color32 = Color32::from_rgb(255, 168, 44);
const SUN_LIGHT: Color32 = Color32::from_rgb(255, 224, 120);
const MOON: Color32 = Color32::from_rgb(246, 236, 196);
const CLOUD: Color32 = Color32::from_rgb(238, 242, 250);
const CLOUD_SHADE: Color32 = Color32::from_rgb(160, 174, 200);
/// 雾带：深色皮肤下与云几乎同色，单列出来是为了浅色皮肤能换成更实的灰蓝。
const FOG: Color32 = Color32::from_rgb(228, 234, 244);
const STORM: Color32 = Color32::from_rgb(184, 192, 214);
const STORM_SHADE: Color32 = Color32::from_rgb(110, 118, 146);
const RAIN: Color32 = Color32::from_rgb(96, 170, 255);
const SNOW: Color32 = Color32::from_rgb(222, 238, 255);
const BOLT: Color32 = Color32::from_rgb(255, 208, 64);
const NIGHT: Color32 = Color32::from_rgb(150, 160, 255);

const LIGHT_TONES: [(Color32, Color32); 14] = [
    (SUN, Color32::from_rgb(255, 178, 36)),
    (SUN_CORE, Color32::from_rgb(255, 146, 28)),
    (SUN_LIGHT, Color32::from_rgb(255, 202, 64)),
    (MOON, Color32::from_rgb(244, 176, 52)),
    (CLOUD, Color32::from_rgb(200, 212, 230)),
    (CLOUD_SHADE, Color32::from_rgb(136, 154, 188)),
    (FOG, Color32::from_rgb(132, 148, 176)),
    (STORM, Color32::from_rgb(164, 176, 200)),
    (STORM_SHADE, Color32::from_rgb(110, 122, 152)),
    (RAIN, Color32::from_rgb(46, 144, 250)),
    (SNOW, Color32::from_rgb(92, 170, 238)),
    (BOLT, Color32::from_rgb(255, 176, 18)),
    (NIGHT, Color32::from_rgb(244, 176, 52)),
    (Color32::WHITE, Color32::WHITE),
];

/// 天气图标的换肤函数：浅色皮肤按 `LIGHT_TONES` 换成浅色版，保留原透明度；
/// 表外颜色与深色皮肤仍走通用换色。
pub(crate) fn light_tone(color: Color32, p: &crate::skin_paint::Palette) -> Color32 {
    let a = color.a();
    let light = p.paper.iter().map(|&v| v as u32).sum::<u32>() > 3 * 128;
    if a == 0 || !light {
        return crate::skin_paint::map_color(color, p);
    }
    let af = a as f32 / 255.0;
    let c = [color.r(), color.g(), color.b()].map(|v| (v as f32 / af).min(255.0));
    let near = |t: Color32| {
        (c[0] - t.r() as f32).abs() <= 6.0
            && (c[1] - t.g() as f32).abs() <= 6.0
            && (c[2] - t.b() as f32).abs() <= 6.0
    };
    match LIGHT_TONES.iter().find(|(dark, _)| near(*dark)) {
        Some(&(_, to)) => Color32::from_rgba_premultiplied(
            (to.r() as f32 * af).round() as u8,
            (to.g() as f32 * af).round() as u8,
            (to.b() as f32 * af).round() as u8,
            a,
        ),
        None => crate::skin_paint::map_color(color, p),
    }
}

/// 把 `paint` 画出的图形登记为天气图标配色，浅色皮肤下不被通用换色压暗。
pub(super) fn toned(c: &Card, paint: impl FnOnce()) {
    let (ctx, layer) = (c.ui.ctx(), c.painter.layer_id());
    let from = crate::skin_paint::shape_count(ctx, layer);
    paint();
    crate::skin_paint::tone_since(ctx, layer, from, light_tone);
}

pub(super) fn sun_at(c: &Card, cx: f32, cy: f32, s: f32, spin: f32, detail: bool) {
    if detail {
        c.glow(cx, cy, s * 1.35, paint::alpha(SUN, 0.24));
    }
    let width = (s * 0.15).clamp(1.1, 3.0);
    for i in 0..8 {
        let a = spin + i as f32 * TAU / 8.0;
        let (r0, r1) = if i % 2 == 0 {
            (0.64, 1.0)
        } else {
            (0.64, 0.86)
        };
        let (a0, a1) = (
            (cx + a.cos() * s * r0, cy + a.sin() * s * r0),
            (cx + a.cos() * s * r1, cy + a.sin() * s * r1),
        );
        c.seg(a0, a1, width, SUN);
        if detail {
            c.circle(a0.0, a0.1, width / 2.0, SUN);
            c.circle(a1.0, a1.1, width / 2.0, SUN);
        }
    }
    c.circle(cx, cy, s * 0.48, SUN_CORE);
    c.circle(cx - s * 0.04, cy - s * 0.04, s * 0.39, SUN_LIGHT);
    if detail {
        c.glow(
            cx - s * 0.14,
            cy - s * 0.14,
            s * 0.26,
            Color32::from_white_alpha(110),
        );
    }
}

/// 弯月：外圆与偏右内圆之间的三角带，保留透明月牙缺口。
fn crescent(c: &Card, cx: f32, cy: f32, s: f32, color: Color32) {
    let (outer, inner, offset) = (0.62 * s, 0.55 * s, 0.30 * s);
    let x = (offset * offset + outer * outer - inner * inner) / (2.0 * offset);
    let y = (outer * outer - x * x).max(0.0).sqrt();
    let (outer_top, inner_top) = (y.atan2(x), y.atan2(x - offset));
    let mut mesh = egui::Mesh::default();
    let steps = 24;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = outer_top + (TAU - 2.0 * outer_top) * t;
        let b = inner_top + (TAU - 2.0 * inner_top) * t;
        mesh.colored_vertex(c.pos(cx + a.cos() * outer, cy + a.sin() * outer), color);
        mesh.colored_vertex(
            c.pos(cx + offset + b.cos() * inner, cy + b.sin() * inner),
            color,
        );
        if i > 0 {
            let n = i * 2;
            mesh.add_triangle(n - 2, n, n + 1);
            mesh.add_triangle(n - 2, n + 1, n - 1);
        }
    }
    // 网格本身没有抗锯齿，小尺寸时边缘发毛；沿内外两条弧各描一道约一像素的同色细线补出平滑边缘。
    // 两条弧分开画、不闭合：月牙尖角极锐，闭合折线在尖角处的斜接会拉出长尖刺。
    let outer_arc: Vec<_> = mesh.vertices.iter().step_by(2).map(|v| v.pos).collect();
    let inner_arc: Vec<_> = mesh
        .vertices
        .iter()
        .skip(1)
        .step_by(2)
        .map(|v| v.pos)
        .collect();
    c.painter.add(egui::Shape::mesh(mesh));
    let stroke = egui::Stroke::new(1.0 / c.ui.ctx().pixels_per_point(), color);
    c.painter.add(egui::Shape::line(outer_arc, stroke));
    c.painter.add(egui::Shape::line(inner_arc, stroke));
}

/// 四角星：夜空点缀。
pub(super) fn sparkle(c: &Card, x: f32, y: f32, r: f32, color: Color32) {
    c.poly(
        &[(x, y - r), (x + r * 0.28, y), (x, y + r), (x - r * 0.28, y)],
        color,
    );
    c.poly(
        &[(x - r, y), (x, y - r * 0.28), (x + r, y), (x, y + r * 0.28)],
        color,
    );
}

/// 月亮：淡月光 + 月牙；大尺寸时旁边点两颗会闪的星。
pub(super) fn moon_at(c: &Card, cx: f32, cy: f32, s: f32, t: f32, detail: bool) {
    if detail {
        c.glow(cx, cy, s * 1.25, paint::alpha(NIGHT, 0.20));
    }
    crescent(c, cx - s * 0.06, cy, s, MOON);
    if detail {
        let twinkle = |k: f32| 0.55 + 0.45 * (t * 2.2 + k).sin();
        sparkle(
            c,
            cx + s * 0.52,
            cy - s * 0.46,
            s * 0.17,
            paint::alpha(MOON, twinkle(0.0)),
        );
        sparkle(
            c,
            cx + s * 0.74,
            cy + s * 0.06,
            s * 0.10,
            paint::alpha(MOON, twinkle(2.1)),
        );
    }
}

/// 云朵轮廓：三团云 + 平底，四个角都被圆覆盖，没有方角外露。
fn cloud_shape(c: &Card, cx: f32, cy: f32, s: f32, color: Color32) {
    c.circle(cx - s * 0.40, cy + s * 0.08, s * 0.30, color);
    c.circle(cx - s * 0.06, cy - s * 0.16, s * 0.42, color);
    c.circle(cx + s * 0.36, cy + s * 0.06, s * 0.32, color);
    c.fill(cx - s * 0.40, cy - s * 0.02, s * 0.76, s * 0.40, 0.0, color);
}

/// 立体云：底部错开一层阴影色，形成厚度。
fn cloud(c: &Card, cx: f32, cy: f32, s: f32, top: Color32, shade: Color32) {
    cloud_shape(c, cx + s * 0.03, cy + s * 0.09, s, shade);
    cloud_shape(c, cx, cy, s, top);
}

/// 雨滴：大尺寸画水滴形，小尺寸退化为斜线保证清晰。
fn drops(
    c: &Card,
    cx: f32,
    top: f32,
    s: f32,
    count: usize,
    len: f32,
    color: Color32,
    t: Option<f32>,
    detail: bool,
) {
    let gap = s * if count == 2 { 0.40 } else { 0.32 };
    for i in 0..count {
        let x = cx - gap * (count as f32 - 1.0) / 2.0 + i as f32 * gap;
        let (fall, fade) = match t {
            Some(t) => {
                let k = (t * 1.6 + i as f32 * 0.37).fract();
                (k * s * 0.22, 1.0 - k * k)
            }
            None => (if i % 2 == 1 { s * 0.08 } else { 0.0 }, 1.0),
        };
        let y = top + fall;
        let color = paint::alpha(color, fade);
        if detail {
            paint::droplet(c, x - s * 0.04, y + s * len * 0.5, s * len, color);
        } else {
            c.seg(
                (x, y),
                (x - s * 0.10, y + s * len),
                (s * 0.20).clamp(1.2, 2.2),
                color,
            );
        }
    }
}

/// 雪花：六根主枝，每根在外侧带一对 V 形侧枝，中心一点；整体缓慢自转。
/// 常驻 15px 时侧枝更靠外、更短更粗，主枝末端不加点，避免糊成星号。
fn snowflake(c: &Card, cx: f32, cy: f32, r: f32, spin: f32, detail: bool) {
    let (width, branch_w, at, len, open) = if detail {
        (
            (r * 0.13).clamp(1.1, 2.4),
            (r * 0.11).clamp(1.0, 2.0),
            0.55,
            0.32,
            0.75,
        )
    } else {
        (1.35, 1.15, 0.62, 0.40, 0.85)
    };
    for k in 0..6 {
        let a = spin + k as f32 * PI / 3.0 - FRAC_PI_2;
        let (dx, dy) = (a.cos(), a.sin());
        c.seg((cx, cy), (cx + dx * r, cy + dy * r), width, SNOW);
        let (bx, by) = (cx + dx * r * at, cy + dy * r * at);
        for side in [-1.0_f32, 1.0] {
            let b = a + side * open;
            c.seg(
                (bx, by),
                (bx + b.cos() * r * len, by + b.sin() * r * len),
                branch_w,
                SNOW,
            );
        }
        if detail {
            c.circle(cx + dx * r, cy + dy * r, width * 0.55, SNOW);
        }
    }
    c.circle(cx, cy, width * if detail { 1.3 } else { 0.9 }, SNOW);
}

/// 闪电：平行四边形 + 三角拼成折线，均为凸多边形，避免凹多边形填充错位。
/// 比例偏矮胖，和其他实心图标的体量一致；不加光晕，与整套图标风格统一。
fn bolt(c: &Card, cx: f32, cy: f32, s: f32, flash: f32) {
    let p = |x: f32, y: f32| (cx + x * s, cy + y * s);
    let color = paint::alpha(BOLT, 0.6 + 0.4 * flash);
    c.poly(
        &[
            p(-0.08, -0.84),
            p(0.56, -0.84),
            p(0.18, -0.06),
            p(-0.52, 0.06),
        ],
        color,
    );
    c.poly(&[p(-0.24, -0.16), p(0.52, -0.16), p(-0.26, 0.86)], color);
}

/// 雾：四条错落的圆头横带，透明度自上而下递减，动态时左右缓慢漂移。
fn haze(c: &Card, cx: f32, cy: f32, s: f32, t: Option<f32>, detail: bool) {
    let bands: &[(f32, f32)] = if detail {
        &[(-0.10, 1.25), (0.16, 1.55), (-0.18, 1.30), (0.10, 0.95)]
    } else {
        &[(-0.10, 1.40), (0.14, 1.60), (-0.12, 1.20)]
    };
    let h = (s * 0.16).max(1.6);
    let gap = s * if detail { 0.44 } else { 0.58 };
    let top = cy - gap * (bands.len() as f32 - 1.0) / 2.0;
    for (i, &(dx, w)) in bands.iter().enumerate() {
        let shift = t
            .map(|t| (t * 0.7 + i as f32 * 1.4).sin() * s * 0.08)
            .unwrap_or(0.0);
        let y = top + i as f32 * gap;
        c.fill(
            cx + s * (dx - w / 2.0) + shift,
            y - h / 2.0,
            s * w,
            h,
            h / 2.0,
            paint::alpha(FOG, 0.95 - i as f32 * 0.16),
        );
    }
}

/// 细雨：一大两小三颗水滴错落排布，动态时轻轻上下浮动。
fn droplets(c: &Card, cx: f32, cy: f32, s: f32, t: Option<f32>) {
    let bob = |k: f32| t.map(|t| (t * 2.0 + k).sin() * s * 0.05).unwrap_or(0.0);
    paint::droplet(c, cx - s * 0.20, cy + s * 0.12 + bob(0.0), s * 1.05, RAIN);
    paint::droplet(
        c,
        cx + s * 0.46,
        cy - s * 0.42 + bob(1.6),
        s * 0.56,
        paint::alpha(RAIN, 0.85),
    );
    paint::droplet(
        c,
        cx + s * 0.50,
        cy + s * 0.46 + bob(3.1),
        s * 0.44,
        paint::alpha(RAIN, 0.7),
    );
    // 大水滴左上的高光，让它像水而不是色块。
    if s >= 11.0 {
        c.circle(
            cx - s * 0.34,
            cy + s * 0.22,
            s * 0.08,
            Color32::from_white_alpha(150),
        );
    }
}

/// 带动效的图标：`t` 为秒，雨滴下落、雪花自转、雾带漂移、光芒缓转、星星闪烁。
/// `t` 为 None 时绘制静态图标。
/// 只有晴间多云、阴、雨、阵雨用云；雪、雾、细雨、雷各用独立的主体形状，常驻栏一眼能分辨。
pub(crate) fn icon_at(c: &Card, cx: f32, cy: f32, size: f32, kind: Kind, t: Option<f32>) {
    toned(c, || icon_shapes(c, cx, cy, size, kind, t));
}

fn icon_shapes(c: &Card, cx: f32, cy: f32, size: f32, kind: Kind, t: Option<f32>) {
    let s = size / 2.0;
    let detail = size >= 22.0;
    let time = t.unwrap_or(0.0);
    let float = t.map(|t| (t * 0.9).sin() * s * 0.08).unwrap_or(0.0);
    match kind {
        Kind::Sun => sun_at(c, cx, cy, s * 0.92, time * 0.25, detail),
        Kind::Moon => moon_at(c, cx - s * 0.06, cy, s * 1.08, time, detail),
        Kind::Partly => {
            sun_at(
                c,
                cx + s * 0.28,
                cy - s * 0.26,
                s * 0.60,
                time * 0.25,
                detail,
            );
            cloud(
                c,
                cx - s * 0.10 + float,
                cy + s * 0.22,
                s * 0.88,
                CLOUD,
                CLOUD_SHADE,
            );
        }
        Kind::PartlyNight => {
            moon_at(c, cx + s * 0.26, cy - s * 0.28, s * 0.66, time, false);
            cloud(
                c,
                cx - s * 0.10 + float,
                cy + s * 0.22,
                s * 0.88,
                CLOUD,
                CLOUD_SHADE,
            );
        }
        Kind::Cloud => {
            cloud_shape(
                c,
                cx + s * 0.30 - float,
                cy - s * 0.22,
                s * 0.58,
                CLOUD_SHADE,
            );
            cloud(
                c,
                cx - s * 0.06 + float,
                cy + s * 0.12,
                s * 0.90,
                CLOUD,
                CLOUD_SHADE,
            );
        }
        Kind::Fog => haze(c, cx, cy, s, t, detail),
        Kind::Drizzle => droplets(c, cx, cy, s, t),
        Kind::Rain => {
            cloud(c, cx, cy - s * 0.20, s * 0.88, STORM, STORM_SHADE);
            drops(c, cx, cy + s * 0.32, s, 3, 0.36, RAIN, t, detail);
        }
        Kind::Shower => {
            sun_at(
                c,
                cx + s * 0.34,
                cy - s * 0.40,
                s * 0.42,
                time * 0.25,
                false,
            );
            cloud(
                c,
                cx - s * 0.06 + float,
                cy - s * 0.14,
                s * 0.84,
                STORM,
                STORM_SHADE,
            );
            drops(c, cx - s * 0.06, cy + s * 0.36, s, 2, 0.34, RAIN, t, detail);
        }
        Kind::Snow => snowflake(c, cx, cy, s * 0.92, time * 0.35, detail),
        Kind::Thunder => {
            let flash = t
                .map(|t| {
                    let k = (t * 0.45).fract();
                    if k < 0.06 || (0.10..0.13).contains(&k) {
                        1.0
                    } else {
                        0.6
                    }
                })
                .unwrap_or(1.0);
            bolt(c, cx + s * 0.02, cy, s, flash);
        }
    }
}
