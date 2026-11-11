//! 刘海面板轮廓：顶部向外翻的「肩」+ 竖直侧边 + 底部大圆角。
//!
//! 轮廓左右对称、自上而下单调，所以按行切成梯形条填充（可处理凹肩），
//! 再沿外轮廓补一圈 1 物理像素的羽化带做抗锯齿——边缘不再有锯齿台阶。

use eframe::egui::{self, Color32, Pos2, Rect, Vec2};

/// (y, 左 x, 右 x) 轮廓采样行。
pub fn sections(rect: Rect, flare: f32, flare_h: f32, radius: f32) -> Vec<(f32, f32, f32)> {
    let (x0, y0, x1, y1) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let r = radius
        .min(rect.height() * 0.5)
        .min(rect.width() * 0.5)
        .max(0.0);
    let fh = flare_h.min(rect.height() * 0.5).max(0.5);
    let mut rows = Vec::with_capacity(48);
    const N_FLARE: usize = 14;
    for i in 0..=N_FLARE {
        let t = i as f32 / N_FLARE as f32;
        // 肩部曲线：(1-t)^2.4 比三次方更饱满，过渡更柔。
        let wing = flare * (1.0 - t).powf(2.4);
        rows.push((y0 + fh * t, x0 - wing, x1 + wing));
    }
    if y1 - r > y0 + fh {
        rows.push((y1 - r, x0, x1));
    }
    const N_CORNER: usize = 20;
    for i in 1..=N_CORNER {
        let a = i as f32 / N_CORNER as f32 * std::f32::consts::FRAC_PI_2;
        rows.push((
            y1 - r + r * a.sin(),
            x0 + r - r * a.cos(),
            x1 - r + r * a.cos(),
        ));
    }
    rows
}

/// 闭合外轮廓（顺时针：右侧自上而下，再左侧自下而上）。
pub fn outline(rect: Rect, flare: f32, flare_h: f32, radius: f32) -> Vec<Pos2> {
    let rows = sections(rect, flare, flare_h, radius);
    rows.iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect()
}

// 顶边不参与羽化；端点只取肩部切线，避免整条顶边扭曲法线。
fn edge_normal(pts: &[Pos2], i: usize) -> Vec2 {
    let normal = |a: Pos2, b: Pos2| {
        let d = (b - a).normalized();
        Vec2::new(d.y, -d.x)
    };
    if i == 0 {
        return normal(pts[0], pts[1]);
    }
    if i == pts.len() - 1 {
        return normal(pts[i - 1], pts[i]);
    }
    let prev = normal(pts[i - 1], pts[i]);
    let next = normal(pts[i], pts[i + 1]);
    // 相邻单位法线的交点补偿，保持两段的垂直羽化宽度一致。
    (prev + next) / (1.0 + prev.dot(next))
}

/// 抗锯齿填充。`feather` 是羽化宽度（逻辑点，通常 = 1 / ppp）。
pub fn fill(
    painter: &egui::Painter,
    rect: Rect,
    flare: f32,
    flare_h: f32,
    radius: f32,
    color: Color32,
    feather: f32,
) {
    let rows = sections(rect, flare, flare_h, radius);
    let mut mesh = egui::Mesh::default();
    // 1) 实心梯形条
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(la, ya), color);
        mesh.colored_vertex(Pos2::new(ra, ya), color);
        mesh.colored_vertex(Pos2::new(rb, yb), color);
        mesh.colored_vertex(Pos2::new(lb, yb), color);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    }
    // 2) 外轮廓羽化带（不含顶边：贴着屏幕顶）
    let pts: Vec<Pos2> = rows
        .iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect();
    let n = pts.len();
    let clear = Color32::TRANSPARENT;
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        // 顶边两端（右上角 → 左上角那条闭合边）不羽化
        if a.y <= rect.top() + 0.01 && b.y <= rect.top() + 0.01 {
            continue;
        }
        let (na, nb) = (edge_normal(&pts, i), edge_normal(&pts, i + 1));
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, color);
        mesh.colored_vertex(b, color);
        mesh.colored_vertex(b + nb * feather, clear);
        mesh.colored_vertex(a + na * feather, clear);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// 柔和的落影：多层半透明轮廓叠加，近似高斯模糊（不依赖 GPU 模糊）。
pub fn shadow(
    painter: &egui::Painter,
    rect: Rect,
    flare: f32,
    flare_h: f32,
    radius: f32,
    strength: f32,
) {
    if strength <= 0.01 {
        return;
    }
    for (grow, a) in [(10.0f32, 0.035f32), (6.5, 0.05), (3.5, 0.07), (1.5, 0.09)] {
        let r = Rect::from_min_max(
            Pos2::new(rect.left() - grow * 0.6, rect.top()),
            Pos2::new(rect.right() + grow * 0.6, rect.bottom() + grow),
        );
        let c = Color32::from_black_alpha((255.0 * a * strength) as u8);
        fill(
            painter,
            r,
            flare,
            flare_h,
            radius + grow * 0.8,
            c,
            grow * 0.9,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inside(points: &[Pos2], p: Pos2) -> bool {
        let mut inside = false;
        let mut prev = *points.last().unwrap();
        for &next in points {
            if (next.y > p.y) != (prev.y > p.y)
                && p.x < (prev.x - next.x) * (p.y - next.y) / (prev.y - next.y) + next.x
            {
                inside = !inside;
            }
            prev = next;
        }
        inside
    }

    #[test]
    fn shoulder_feather_keeps_one_pixel_perpendicular_to_edge() {
        for (width, height, flare, flare_h, radius) in [
            (143.0, 25.0, 5.0, 8.0, 12.5),
            (419.0, 140.0, 17.6, 16.0, 33.0),
        ] {
            let pts = outline(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(width, height)),
                flare,
                flare_h,
                radius,
            );
            for ppp in [1.0, 1.25, 1.5, 2.0] {
                for i in 0..pts.len() - 1 {
                    let d = (pts[i + 1] - pts[i]).normalized();
                    let normal = Vec2::new(d.y, -d.x);
                    for j in [i, i + 1] {
                        let offset = edge_normal(&pts, j) / ppp;
                        assert!(
                            (offset.dot(normal) * ppp - 1.0).abs() < 0.001,
                            "羽化必须沿每段轮廓保持一物理像素：边 {i}，顶点 {j}，缩放 {ppp}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn wings_extend_outside_body_only_at_top() {
        let rect = Rect::from_min_size(Pos2::new(110.0, 0.0), Vec2::new(419.0, 140.0));
        let pts = outline(rect, 17.6, 16.0, 33.0);
        assert!(pts.iter().any(|p| p.x < rect.left() - 17.0));
        assert!(inside(&pts, Pos2::new(rect.left() + 0.2, 70.0)));
        assert!(!inside(&pts, Pos2::new(rect.left() - 0.5, 70.0)));
        // 底部圆角外侧是空的
        assert!(!inside(
            &pts,
            Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0)
        ));
    }
}

const ROW_STEP: f32 = 6.0;
const COL_STEP: f32 = 9.0;
const COLS_MIN: usize = 10;
const COLS_MAX: usize = 64;

/// 把行表补密：相邻两行之间按需插入等距行，左右边界线性插值。
fn densify(rows: Vec<(f32, f32, f32)>) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::with_capacity(rows.len() * 3);
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        out.push(w[0]);
        let n = ((yb - ya) / ROW_STEP).ceil() as usize;
        for i in 1..n {
            let t = i as f32 / n as f32;
            out.push((ya + (yb - ya) * t, la + (lb - la) * t, ra + (rb - ra) * t));
        }
    }
    if let Some(last) = rows.last() {
        out.push(*last);
    }
    out
}

pub(crate) fn shade_mesh(
    rect: Rect,
    rows: Vec<(f32, f32, f32)>,
    feather: f32,
    shade: &dyn Fn(f32, f32) -> Color32,
) -> egui::Mesh {
    let rows = densify(rows);
    let cols = ((rect.width() / COL_STEP).ceil() as usize).clamp(COLS_MIN, COLS_MAX);
    let mut mesh = egui::Mesh::default();
    mesh.vertices.reserve(rows.len() * cols * 4);
    mesh.indices.reserve(rows.len() * cols * 6);
    // 1) 二维网格：每行横向切 cols 段，四个角各自取色，光斑才不会跨行被抹平
    for w in rows.windows(2) {
        let (ya, la, ra) = w[0];
        let (yb, lb, rb) = w[1];
        for i in 0..cols {
            let t0 = i as f32 / cols as f32;
            let t1 = (i + 1) as f32 / cols as f32;
            let ax0 = la + (ra - la) * t0;
            let ax1 = la + (ra - la) * t1;
            let bx0 = lb + (rb - lb) * t0;
            let bx1 = lb + (rb - lb) * t1;
            let id = mesh.vertices.len() as u32;
            mesh.colored_vertex(Pos2::new(ax0, ya), shade(ax0, ya));
            mesh.colored_vertex(Pos2::new(ax1, ya), shade(ax1, ya));
            mesh.colored_vertex(Pos2::new(bx1, yb), shade(bx1, yb));
            mesh.colored_vertex(Pos2::new(bx0, yb), shade(bx0, yb));
            mesh.add_triangle(id, id + 1, id + 2);
            mesh.add_triangle(id, id + 2, id + 3);
        }
    }
    // 2) 外轮廓羽化带（不含顶边）：外沿保持同色但完全透明
    let pts: Vec<Pos2> = rows
        .iter()
        .map(|&(y, _, r)| Pos2::new(r, y))
        .chain(rows.iter().rev().map(|&(y, l, _)| Pos2::new(l, y)))
        .collect();
    let n = pts.len();
    for i in 0..n - 1 {
        let (a, b) = (pts[i], pts[i + 1]);
        if a.y <= rect.top() + 0.01 && b.y <= rect.top() + 0.01 {
            continue;
        }
        let (na, nb) = (edge_normal(&pts, i), edge_normal(&pts, i + 1));
        let clear = |p: Pos2| {
            let c = shade(p.x, p.y);
            Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 0)
        };
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(a, shade(a.x, a.y));
        mesh.colored_vertex(b, shade(b.x, b.y));
        mesh.colored_vertex(b + nb * feather, clear(b + nb * feather));
        mesh.colored_vertex(a + na * feather, clear(a + na * feather));
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh
}
