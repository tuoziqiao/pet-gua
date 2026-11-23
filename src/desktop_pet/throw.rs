//! Drag release and desktop flight, with physical screen coordinates.
//! Release estimation and physical constants adapted from dsh-pet (MIT),
//! copyright 2026 PC2005-cloud; see THIRD_PARTY_NOTICES.txt.
use super::placement::{Rect, Screen};
use std::time::{Duration, Instant};

/// Tracks two left-button presses and the second press's physical drag origin.
#[cfg(test)]
#[derive(Default)]
pub(super) struct Slingshot {
    last_press: Option<Instant>,
    anchor: Option<((i32, i32), i32)>,
}

#[cfg(test)]
impl Slingshot {
    pub fn press(&mut self, now: Instant, cursor: (i32, i32), width: i32) {
        let double = self.last_press.is_some_and(|last| {
            now.checked_duration_since(last)
                .is_some_and(|d| d < Duration::from_millis(200))
        });
        self.last_press = if double { None } else { Some(now) };
        self.anchor = if double && width > 0 {
            Some((cursor, width))
        } else {
            None
        };
    }

    pub fn drag_started(&mut self) {
        if self.anchor.is_none() {
            self.last_press = None;
        }
    }

    pub fn is_active(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn anchor_cursor(&self) -> Option<(i32, i32)> {
        self.anchor.map(|(cursor, _)| cursor)
    }

    /// Previewing must use the same origin and size as the eventual release.
    pub fn velocity(&self, cursor: (i32, i32)) -> Option<(f64, f64)> {
        let (anchor, width) = self.anchor?;
        let dx = cursor.0 as f64 - anchor.0 as f64;
        let dy = cursor.1 as f64 - anchor.1 as f64;
        if dx == 0.0 && dy == 0.0 {
            return None;
        }
        let factor = 1200.0 / (width as f64 * 0.6);
        Some((-dx * factor, -dy * factor))
    }

    pub fn release(&mut self, cursor: (i32, i32)) -> Option<(f64, f64)> {
        let velocity = self.velocity(cursor);
        self.anchor = None;
        velocity
    }

    pub fn release_without_drag(&mut self) {
        self.anchor = None;
    }
    pub fn cancel(&mut self) {
        *self = Self::default();
    }
}

#[derive(Default)]
pub(super) struct Trail(Vec<(Instant, (i32, i32))>);

impl Trail {
    pub fn push(&mut self, now: Instant, point: (i32, i32)) {
        if self.0.last().is_none_or(|(_, p)| *p != point) {
            self.0.push((now, point));
        }
        self.0
            .retain(|(t, _)| now.saturating_duration_since(*t) <= Duration::from_millis(200));
    }

    pub fn velocity(&self, now: Instant) -> Option<(f64, f64)> {
        let last = self.0.last()?;
        if now.saturating_duration_since(last.0) > Duration::from_millis(150) {
            return None;
        }
        let samples: Vec<_> = self
            .0
            .iter()
            .filter(|(t, _)| now.saturating_duration_since(*t) <= Duration::from_millis(150))
            .collect();
        let first = *samples.first()?;
        let span = last.0.saturating_duration_since(first.0).as_secs_f64();
        if span < 0.020 {
            return None;
        }
        let vx = (last.1 .0 as f64 - first.1 .0 as f64) / span;
        let vy = (last.1 .1 as f64 - first.1 .1 as f64) / span;
        let base = vx.hypot(vy);
        if base < 1e-6 {
            return None;
        }
        let mut segments = Vec::new();
        let mut previous = first;
        for sample in samples.iter().skip(1) {
            let dt = sample.0.saturating_duration_since(previous.0).as_secs_f64();
            if dt >= 0.008 {
                segments.push((
                    ((sample.1 .0 as f64 - previous.1 .0 as f64)
                        .hypot(sample.1 .1 as f64 - previous.1 .1 as f64))
                        / dt,
                    sample.0,
                ));
                previous = sample;
            }
        }
        let peak = segments.iter().map(|s| s.0).fold(base, f64::max);
        let accel = if segments.len() >= 2 {
            let a = segments.first().unwrap();
            let b = segments.last().unwrap();
            (b.0 - a.0) / b.1.saturating_duration_since(a.1).as_secs_f64().max(0.020)
        } else {
            0.0
        };
        let raw = (base + peak) * 0.5 * (1.0 + (accel / 8000.0).clamp(0.0, 1.0) * 0.6);
        let speed = 3600.0 * (1.0 - (-raw / 3600.0).exp());
        if speed < 500.0 {
            None
        } else {
            Some((vx / base * speed, vy / base * speed))
        }
    }
}

/// Visible body bounds, relative to the native window, frozen for one flight.
#[derive(Clone, Copy, Debug)]
pub(super) struct Body {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}
impl Body {
    pub fn from_mask(size: (i32, i32), mask: Option<(&[u8], [usize; 2])>, mirror: bool) -> Self {
        let fallback = Self {
            left: 0.0,
            top: 0.0,
            right: size.0.max(1) as f64,
            bottom: size.1.max(1) as f64,
        };
        let Some((pixels, [w, h])) = mask else {
            return fallback;
        };
        if w == 0
            || h == 0
            || pixels.len() != w.saturating_mul(h).saturating_mul(4)
            || size.0 <= 0
            || size.1 <= 0
        {
            return fallback;
        }
        let (mut l, mut t, mut r, mut b) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if pixels[(y * w + x) * 4 + 3] >= 16 {
                    let x = if mirror { w - 1 - x } else { x };
                    l = l.min(x);
                    t = t.min(y);
                    r = r.max(x + 1);
                    b = b.max(y + 1);
                }
            }
        }
        if l >= r || t >= b {
            return fallback;
        }
        Self {
            left: l as f64 * size.0 as f64 / w as f64,
            top: t as f64 * size.1 as f64 / h as f64,
            right: r as f64 * size.0 as f64 / w as f64,
            bottom: b as f64 * size.1 as f64 / h as f64,
        }
    }
}

pub(super) struct Flight {
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    body: Body,
    pub size: (i32, i32),
    peak_landing_impact: Option<f64>,
}
impl Flight {
    pub fn new(pos: (i32, i32), size: (i32, i32), body: Body, velocity: (f64, f64)) -> Self {
        Self {
            x: pos.0 as f64,
            y: pos.1 as f64,
            vx: velocity.0,
            vy: velocity.1,
            body,
            size,
            peak_landing_impact: None,
        }
    }
    pub fn position(&self) -> (i32, i32) {
        (self.x.round() as i32, self.y.round() as i32)
    }

    pub fn peak_landing_impact(&self) -> Option<f64> {
        self.peak_landing_impact
    }

    /// Reconcile a native resize while preserving velocity and fractional motion
    /// on ordinary frames. Windows may also reposition the window on a DPI change.
    pub fn sync_geometry(&mut self, pos: (i32, i32), size: (i32, i32)) -> bool {
        if size.0 <= 0 || size.1 <= 0 || self.size.0 <= 0 || self.size.1 <= 0 {
            return false;
        }
        if size != self.size {
            let sx = size.0 as f64 / self.size.0 as f64;
            let sy = size.1 as f64 / self.size.1 as f64;
            self.body.left *= sx;
            self.body.right *= sx;
            self.body.top *= sy;
            self.body.bottom *= sy;
            self.x = pos.0 as f64;
            self.y = pos.1 as f64;
            self.size = size;
        }
        true
    }

    /// None means screen data is unavailable. True means the pet settled.
    pub fn step(&mut self, dt: f64, screens: &[Screen]) -> Option<bool> {
        self.step_with_targets(dt, screens, &[], |_| false, &mut Vec::new())
    }

    /// Simulate stationary targets in the same bounded substeps as the flight.
    pub fn step_with_targets<F: FnMut((i32, i32)) -> bool>(
        &mut self, dt: f64, screens: &[Screen], targets: &[super::placement::Rect],
        mut visible: F, hits: &mut Vec<(i32, i32)>,
    ) -> Option<bool> {
        if screens.is_empty() {
            return None;
        }
        let mut remaining = dt.clamp(0.0, 0.05);
        let mut rest = false;
        // Bound each displacement even for an uncapped slingshot. If an extreme
        // input exhausts the work budget, slow simulated time rather than tunnel.
        for _ in 0..256 {
            if remaining <= 0.0 {
                break;
            }
            let speed = self.vx.hypot(self.vy) + 1400.0 * remaining;
            let dt = remaining.min(8.0 / speed.max(1.0));
            remaining -= dt;
            let old = (self.x, self.y);
            self.vy += 1400.0 * dt;
            self.x += self.vx * dt;
            self.y += self.vy * dt;
            for target in targets {
                let (l,t,r,b)=(target.left as f64,target.top as f64,target.right as f64,target.bottom as f64);
                let (pl,pt,pr,pb)=(self.x+self.body.left,self.y+self.body.top,self.x+self.body.right,self.y+self.body.bottom);
                if pl>=r || pr<=l || pt>=b || pb<=t { continue; }
                let previous=(old.0+self.body.left,old.1+self.body.top,old.0+self.body.right,old.1+self.body.bottom);
                let contact = if previous.2<=l && self.vx>0.0 { Some((0, l, ((pt+pb)*0.5).clamp(t,b))) }
                    else if previous.0>=r && self.vx<0.0 { Some((1,r,((pt+pb)*0.5).clamp(t,b))) }
                    else if previous.3<=t && self.vy>0.0 { Some((2,((pl+pr)*0.5).clamp(l,r),t)) }
                    else if previous.1>=b && self.vy<0.0 { Some((3,((pl+pr)*0.5).clamp(l,r),b)) }
                    else { None };
                if let Some((side,x,y))=contact {
                    let point=(x.round() as i32,y.round() as i32);
                    if !visible(point) { continue; }
                    match side {
                        0 => { self.x=l-self.body.right; self.vx=-self.vx.abs()*0.7; },
                        1 => { self.x=r-self.body.left; self.vx=self.vx.abs()*0.7; },
                        2 => { self.y=t-self.body.bottom; self.vy=-self.vy.abs()*0.7; },
                        _ => { self.y=b-self.body.top; self.vy=self.vy.abs()*0.7; },
                    }
                    hits.push(point);
                    break;
                }
            }
            let incoming_velocity = (self.vx, self.vy);
            let cx = self.x + (self.body.left + self.body.right) * 0.5;
            let cy = self.y + (self.body.top + self.body.bottom) * 0.5;
            let screen = screens
                .iter()
                .min_by(|a, b| distance(a.bounds, cx, cy).total_cmp(&distance(b.bounds, cx, cy)))?;
            let work = screen.work;
            let mut horizontal = false;
            let mut vertical = false;
            if self.x + self.body.left < work.left as f64 && !neighbor(screens, screen, 0, cy) {
                self.x = work.left as f64 - self.body.left;
                self.vx = reflected(self.vx, 1.0);
                horizontal = true;
            } else if self.x + self.body.right > work.right as f64
                && !neighbor(screens, screen, 1, cy)
            {
                self.x = work.right as f64 - self.body.right;
                self.vx = reflected(self.vx, -1.0);
                horizontal = true;
            }
            if self.y + self.body.top < work.top as f64 && !neighbor(screens, screen, 2, cx) {
                self.y = work.top as f64 - self.body.top;
                self.vy = reflected(self.vy, 1.0);
                vertical = true;
            } else if self.y + self.body.bottom >= work.bottom as f64
                && !neighbor(screens, screen, 3, cx)
            {
                if incoming_velocity.1 > 0.0 {
                    let impact = incoming_velocity.0.hypot(incoming_velocity.1);
                    self.peak_landing_impact = Some(
                        self.peak_landing_impact.map_or(impact, |peak| peak.max(impact)),
                    );
                }
                self.y = work.bottom as f64 - self.body.bottom;
                self.vy = reflected(self.vy, -1.0);
                vertical = true;
            }
            let friction = (1.0 - 2.5 * dt).max(0.0);
            if horizontal {
                self.vy *= friction;
            }
            if vertical {
                self.vx *= friction;
            }
            rest = vertical && self.vy.abs() < 1.0 && self.vx.abs() < 15.0;
            if rest {
                self.vx = 0.0;
                self.vy = 0.0;
                break;
            }
        }
        Some(rest)
    }
}

/// Short, evenly spaced preview of a point on the pet's leading edge.
/// Positions and the flight simulation both use physical screen coordinates.
pub(super) fn slingshot_guide(
    pos: (i32, i32),
    size: (i32, i32),
    body: Body,
    velocity: (f64, f64),
    screens: &[Screen],
    pixels_per_point: f64,
) -> Vec<(f64, f64)> {
    slingshot_guide_with_targets(pos,size,body,velocity,screens,pixels_per_point,&[])
}

fn slingshot_guide_with_targets(
    pos: (i32, i32), size: (i32, i32), body: Body, velocity: (f64, f64),
    screens: &[Screen], pixels_per_point: f64, targets: &[Rect],
) -> Vec<(f64, f64)> {
    if screens.is_empty()
        || size.0 <= 0
        || size.1 <= 0
        || !pixels_per_point.is_finite()
        || pixels_per_point <= 0.0
        || !velocity.0.is_finite()
        || !velocity.1.is_finite()
    {
        return Vec::new();
    }
    let speed = velocity.0.hypot(velocity.1);
    if speed < 1e-6 {
        return Vec::new();
    }

    let direction = (velocity.0 / speed, velocity.1 / speed);
    let half_width = (body.right - body.left).max(0.0) * 0.5;
    let half_height = (body.bottom - body.top).max(0.0) * 0.5;
    let edge_x = if direction.0.abs() > 1e-9 {
        half_width / direction.0.abs()
    } else {
        f64::INFINITY
    };
    let edge_y = if direction.1.abs() > 1e-9 {
        half_height / direction.1.abs()
    } else {
        f64::INFINITY
    };
    let edge = edge_x.min(edge_y) + 7.0 * pixels_per_point;
    let offset = (
        (body.left + body.right) * 0.5 + direction.0 * edge,
        (body.top + body.bottom) * 0.5 + direction.1 * edge,
    );
    let mut dots = vec![(pos.0 as f64 + offset.0, pos.1 as f64 + offset.1)];
    let spacing = 14.0 * pixels_per_point;
    let mut remaining = spacing;
    let mut flight = Flight::new(pos, size, body, velocity);

    // Bound the work even for extreme drags. Short adaptive steps preserve the
    // existing collision response and give stable spacing along a curved path.
    for _ in 0..600 {
        let start = (flight.x, flight.y);
        let current_speed = flight.vx.hypot(flight.vy);
        let dt = (spacing * 0.25 / current_speed.max(1.0)).min(1.0 / 120.0);
        let settled = flight.step_with_targets(dt, screens, targets, |_| true, &mut Vec::new()).unwrap_or(true);
        let delta = (flight.x - start.0, flight.y - start.1);
        let length = delta.0.hypot(delta.1);
        let mut used = 0.0;
        while length - used >= remaining && dots.len() < 8 {
            used += remaining;
            let fraction = used / length;
            dots.push((
                start.0 + delta.0 * fraction + offset.0,
                start.1 + delta.1 * fraction + offset.1,
            ));
            remaining = spacing;
        }
        remaining = (remaining - (length - used)).max(spacing * 1e-9);
        if dots.len() == 8 || settled {
            break;
        }
    }
    dots
}

/// Sample the same flight path from the pet's feet instead of its leading edge.
#[cfg(windows)]
pub(super) fn foot_guide(pos: (i32,i32), size: (i32,i32), body: Body, velocity: (f64,f64), screens: &[Screen], scale: f64, targets: &[Rect]) -> Vec<(f64,f64)> {
    let mut dots=slingshot_guide_with_targets(pos,size,body,velocity,screens,scale,targets);
    if let Some(first)=dots.first().copied() {
        let foot=(pos.0 as f64+(body.left+body.right)*0.5,pos.1 as f64+body.bottom);
        for point in &mut dots { point.0+=foot.0-first.0; point.1+=foot.1-first.1; }
    }
    dots
}

fn reflected(v: f64, direction: f64) -> f64 {
    if v.abs() < 40.0 {
        0.0
    } else {
        v.abs() * 0.78 * direction
    }
}
fn distance(r: Rect, x: f64, y: f64) -> f64 {
    let dx = (r.left as f64 - x).max(0.0).max(x - r.right as f64);
    let dy = (r.top as f64 - y).max(0.0).max(y - r.bottom as f64);
    dx * dx + dy * dy
}
fn neighbor(screens: &[Screen], current: &Screen, edge: u8, coordinate: f64) -> bool {
    let a = current.bounds;
    screens.iter().any(|s| {
        let b = s.bounds;
        if b == a {
            return false;
        }
        match edge {
            0 => b.right == a.left && coordinate >= b.top as f64 && coordinate < b.bottom as f64,
            1 => b.left == a.right && coordinate >= b.top as f64 && coordinate < b.bottom as f64,
            2 => b.bottom == a.top && coordinate >= b.left as f64 && coordinate < b.right as f64,
            _ => b.top == a.bottom && coordinate >= b.left as f64 && coordinate < b.right as f64,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screen(left: i32, top: i32, right: i32, bottom: i32) -> Screen {
        let bounds = Rect {
            left,
            top,
            right,
            bottom,
        };
        Screen {
            bounds,
            work: bounds,
        }
    }
    fn body() -> Body {
        Body {
            left: 0.0,
            top: 0.0,
            right: 100.0,
            bottom: 100.0,
        }
    }
    #[test]
    fn target_collision_reflects_once_and_respects_visibility() {
        let screens=[screen(0,0,2000,2000)];
        let target=Rect {left:300,top:100,right:350,bottom:200};
        let mut f=Flight::new((100,100),(100,100),body(),(2000.0,0.0));
        let mut hits=Vec::new();
        for _ in 0..10 { f.step_with_targets(0.02,&screens,&[target],|_| true,&mut hits); if !hits.is_empty() { break; } }
        assert_eq!(hits.len(),1);
        assert!(f.vx<0.0);
        assert_eq!(hits[0].0,300);
        let mut hidden=Flight::new((100,100),(100,100),body(),(2000.0,0.0));
        let mut hits=Vec::new();
        for _ in 0..10 { hidden.step_with_targets(0.02,&screens,&[target],|_| false,&mut hits); }
        assert!(hits.is_empty());
        assert!(hidden.x>300.0);
    }
    #[test]
    fn target_collision_handles_vertical_and_initial_overlap() {
        let screens=[screen(0,0,2000,2000)];
        let target=Rect {left:300,top:300,right:350,bottom:350};
        let mut f=Flight::new((275,100),(100,100),body(),(0.0,1500.0));
        let mut hits=Vec::new();
        for _ in 0..8 { f.step_with_targets(0.02,&screens,&[target],|_| true,&mut hits); }
        assert_eq!(hits.len(),1);
        assert!(f.vy<0.0);
        let mut overlap=Flight::new((275,275),(100,100),body(),(500.0,0.0));
        let mut hits=Vec::new();
        overlap.step_with_targets(0.02,&screens,&[target],|_| true,&mut hits);
        assert!(hits.is_empty());
    }
    #[test]
    fn target_collision_detects_fast_right_and_bottom_approaches() {
        let screens=[screen(0,0,2000,2000)];
        let target=Rect {left:300,top:300,right:350,bottom:350};
        for (pos, velocity, horizontal) in [((600,275),(-12000.0,0.0),true),((275,600),(0.0,-12000.0),false)] {
            let mut flight=Flight::new(pos,(100,100),body(),velocity);
            let mut hits=Vec::new();
            flight.step_with_targets(0.04,&screens,&[target],|_| true,&mut hits);
            assert_eq!(hits.len(),1);
            if horizontal { assert!(flight.vx>0.0); } else { assert!(flight.vy>0.0); }
        }
    }
    #[test]
    fn slingshot_double_press_and_drag_uses_distance_not_speed() {
        let start = Instant::now();
        let mut sling = Slingshot::default();
        sling.press(start, (500, 400), 200);
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(199), (500, 400), 200);
        assert!(sling.is_active());
        sling.drag_started();
        assert_eq!(sling.release((380, 400)), Some((1200.0, 0.0)));
        assert!(!sling.is_active());

        sling.press(start + Duration::from_millis(500), (500, 400), 200);
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(550), (500, 400), 200);
        sling.drag_started();
        assert_eq!(sling.release((620, 460)), Some((-1200.0, -600.0)));
    }

    #[test]
    fn slingshot_preview_uses_the_same_velocity_as_release() {
        let start = Instant::now();
        for cursor in [(380, 340), (620, 340), (380, 460), (620, 460), (500, 400)] {
            let mut sling = Slingshot::default();
            assert_eq!(sling.velocity(cursor), None);
            sling.press(start, (500, 400), 200);
            sling.release_without_drag();
            sling.press(start + Duration::from_millis(100), (500, 400), 200);
            sling.drag_started();
            let preview = sling.velocity(cursor);
            assert!(sling.is_active(), "previewing must not consume the anchor");
            assert_eq!(preview, sling.release(cursor));
            assert!(!sling.is_active());
        }
    }

    #[test]
    fn slingshot_guide_follows_gravity_from_the_leading_edge() {
        let screens = [screen(0, 0, 4000, 4000)];
        let dots = slingshot_guide((1000, 1000), (100, 100), body(), (300.0, 0.0), &screens, 1.0);
        assert_eq!(dots.len(), 8);
        assert_eq!(dots[0], (1107.0, 1050.0));
        assert!(dots.windows(2).all(|pair| pair[1].0 > pair[0].0 && pair[1].1 > pair[0].1));
        assert!(dots[7].1 - dots[0].1 > 10.0, "gravity must bend the preview downwards");
        for pair in dots.windows(2) {
            let distance = (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1);
            assert!((13.5..=14.01).contains(&distance), "unexpected dot spacing: {distance}");
        }
    }

    #[test]
    fn slingshot_guide_uses_the_same_screen_boundaries_as_flight() {
        let single = [screen(0, 0, 1000, 800)];
        let dots = slingshot_guide((850, 100), (100, 100), body(), (1000.0, 0.0), &single, 1.0);
        assert_eq!(dots.len(), 8);
        assert!(dots.windows(2).any(|pair| pair[1].0 < pair[0].0), "preview should reflect at the wall");

        let adjacent = [screen(0, 0, 1000, 800), screen(1000, 0, 2000, 800)];
        let dots = slingshot_guide((850, 100), (100, 100), body(), (1000.0, 0.0), &adjacent, 1.0);
        assert!(dots.windows(2).all(|pair| pair[1].0 > pair[0].0), "adjacent screens must not cause a bounce");
        assert!(dots.last().unwrap().0 > 1000.0);
    }

    #[test]
    fn slingshot_guide_handles_missing_screens_and_extreme_velocity() {
        let screens = [screen(0, 0, 2000, 2000)];
        assert!(slingshot_guide((100, 100), (100, 100), body(), (0.0, 0.0), &screens, 1.0).is_empty());
        assert!(slingshot_guide((100, 100), (100, 100), body(), (1000.0, 0.0), &[], 1.0).is_empty());
        let dots = slingshot_guide((100, 100), (100, 100), body(), (1.0e12, -1.0e12), &screens, 2.0);
        assert_eq!(dots.len(), 8);
        assert!(dots.iter().all(|(x, y)| x.is_finite() && y.is_finite()));
    }

    #[test]
    fn slingshot_timeout_triple_click_and_interruption_do_not_arm() {
        let start = Instant::now();
        let mut sling = Slingshot::default();
        sling.press(start, (0, 0), 100);
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(200), (0, 0), 100);
        assert!(
            !sling.is_active(),
            "200 ms is outside the double-press window"
        );
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(250), (0, 0), 100);
        assert!(sling.is_active());
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(280), (0, 0), 100);
        assert!(!sling.is_active(), "third press starts a new pair");
        sling.drag_started();
        sling.press(start + Duration::from_millis(300), (0, 0), 100);
        assert!(
            !sling.is_active(),
            "ordinary dragging must clear the first press"
        );
        sling.cancel();
        sling.press(start + Duration::from_millis(310), (0, 0), 100);
        assert!(!sling.is_active());
    }

    #[test]
    fn slingshot_scales_with_pet_size_and_ignores_window_resize() {
        let start = Instant::now();
        let mut sling = Slingshot::default();
        sling.press(start, (100, 100), 400);
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(50), (100, 100), 400);
        sling.drag_started();
        // Screen coordinates are unchanged by the native window changing DPI.
        assert_eq!(sling.release((220, 100)), Some((-600.0, 0.0)));
        sling.press(start + Duration::from_millis(500), (100, 100), 200);
        sling.release_without_drag();
        sling.press(start + Duration::from_millis(550), (100, 100), 200);
        sling.drag_started();
        assert_eq!(sling.release((220, 100)), Some((-1200.0, 0.0)));
    }

    #[test]
    fn extreme_slingshot_velocity_does_not_skip_a_screen_gap() {
        let screens = [screen(0, 0, 1000, 3000), screen(1020, 0, 2000, 3000)];
        let mut f = Flight::new((800, 100), (100, 100), body(), (1_000_000.0, 0.0));
        assert_eq!(f.step(0.005, &screens), Some(false));
        assert!(
            f.x >= 0.0 && f.x <= 900.0,
            "must remain on its original screen"
        );
    }
    #[test]
    fn release_speed_respects_pause_dead_zone_and_limit() {
        let start = Instant::now();
        let mut trail = Trail::default();
        trail.push(start, (0, 0));
        trail.push(start + Duration::from_millis(100), (20, 0));
        assert!(trail.velocity(start + Duration::from_millis(100)).is_none());
        let mut fast = Trail::default();
        fast.push(start, (0, 0));
        fast.push(start + Duration::from_millis(50), (1000, -500));
        let (x, y) = fast.velocity(start + Duration::from_millis(50)).unwrap();
        assert!(x > 0.0 && y < 0.0 && x.hypot(y) < 3600.0);
        assert!(fast.velocity(start + Duration::from_millis(201)).is_none());
    }
    #[test]
    fn mask_bounds_use_visible_pixels_and_mirroring() {
        let mut pixels = vec![0; 4 * 4 * 4];
        pixels[(1 * 4 + 0) * 4 + 3] = 255;
        pixels[(2 * 4 + 1) * 4 + 3] = 255;
        let a = Body::from_mask((400, 400), Some((&pixels, [4, 4])), false);
        let b = Body::from_mask((400, 400), Some((&pixels, [4, 4])), true);
        assert_eq!(
            (a.left, a.top, a.right, a.bottom),
            (0.0, 100.0, 200.0, 300.0)
        );
        assert_eq!((b.left, b.right), (200.0, 400.0));
    }
    #[test]
    fn gravity_bounces_and_friction_eventually_settles() {
        let screens = [screen(0, 0, 1000, 800)];
        let mut f = Flight::new((850, 100), (100, 100), body(), (1500.0, -900.0));
        let mut wall = false;
        let mut ceiling = false;
        let mut floor = false;
        let mut done = false;
        let mut old_vy = f.vy;
        for _ in 0..3000 {
            done = f.step(0.02, &screens).unwrap();
            wall |= f.vx < 0.0;
            ceiling |= old_vy < 0.0 && f.vy > 0.0 && f.y < 30.0;
            floor |= old_vy > 40.0 && f.vy < 0.0;
            old_vy = f.vy;
            assert!(f.x >= -0.01 && f.x <= 900.01 && f.y >= -0.01 && f.y <= 700.01);
            if done {
                break;
            }
        }
        assert!(
            wall && ceiling && floor && done,
            "wall={wall} ceiling={ceiling} floor={floor} done={done}"
        );
        assert_eq!((f.vx, f.vy), (0.0, 0.0));
    }
    #[test]
    fn flight_crosses_adjacent_screens_but_reflects_at_gaps() {
        let screens = [screen(0, 0, 1000, 800), screen(1000, 0, 2000, 800)];
        let mut f = Flight::new((850, 100), (100, 100), body(), (900.0, 0.0));
        for _ in 0..30 {
            f.step(0.02, &screens);
        }
        assert!(f.x > 1000.0 && f.vx > 0.0);
        let screens = [screen(0, 0, 1000, 800), screen(1000, 500, 2000, 1300)];
        let mut f = Flight::new((850, 100), (100, 100), body(), (900.0, 0.0));
        for _ in 0..10 {
            f.step(0.02, &screens);
        }
        assert!(f.x <= 900.0 && f.vx < 0.0);
        assert!(f.step(0.02, &[]).is_none());
    }

    #[test]
    fn resized_flight_preserves_velocity_and_crosses_then_bounces() {
        let screens = [screen(0, 0, 1000, 2000), screen(1000, 0, 2000, 2000)];
        let body = Body {
            left: 20.0,
            top: 10.0,
            right: 80.0,
            bottom: 90.0,
        };
        let mut f = Flight::new((950, 100), (100, 100), body, (900.0, 0.0));
        assert!(f.sync_geometry((970, 110), (150, 200)));
        assert_eq!(f.position(), (970, 110));
        assert_eq!((f.vx, f.vy), (900.0, 0.0));
        assert_eq!(
            (f.body.left, f.body.top, f.body.right, f.body.bottom),
            (30.0, 20.0, 120.0, 180.0)
        );
        for _ in 0..15 {
            assert_eq!(f.step(0.02, &screens), Some(false));
        }
        assert!(f.x > 1000.0 && f.vx > 0.0);
        let pos = f.position();
        let velocity = (f.vx, f.vy);
        assert!(f.sync_geometry(pos, (75, 100)));
        assert_eq!((f.vx, f.vy), velocity);
        assert_eq!(
            (f.body.left, f.body.top, f.body.right, f.body.bottom),
            (15.0, 10.0, 60.0, 90.0)
        );
        for _ in 0..80 {
            f.step(0.02, &screens);
            if f.vx < 0.0 {
                break;
            }
        }
        assert!(
            f.vx < 0.0,
            "flight should bounce at the far edge after resizing"
        );
    }

    #[test]
    fn unchanged_or_invalid_geometry_does_not_corrupt_flight() {
        let mut f = Flight::new((100, 100), (100, 100), body(), (300.0, 0.0));
        f.x = 100.25;
        assert!(f.sync_geometry((100, 100), (100, 100)));
        assert_eq!(f.x, 100.25, "unchanged size must retain subpixel movement");
        assert!(!f.sync_geometry((0, 0), (0, 100)));
        assert_eq!((f.x, f.vx, f.size), (100.25, 300.0, (100, 100)));
    }

    #[test]
    fn vertical_screens_cross_at_the_panel_seam_and_floor_respects_taskbar() {
        let mut top = screen(0, 0, 1000, 800);
        top.work.bottom = 760;
        let lower = screen(0, 800, 1000, 1600);
        let mut f = Flight::new((450, 620), (100, 100), body(), (0.0, 700.0));
        for _ in 0..20 {
            f.step(0.02, &[top, lower]);
        }
        assert!(
            f.y > 800.0 && f.vy > 0.0,
            "a taskbar at the seam must not block crossing"
        );

        let velocity = (f.vx, f.vy);
        let pos = f.position();
        assert!(f.sync_geometry(pos, (150, 150)));
        assert_eq!((f.vx, f.vy), velocity);
        assert_eq!(f.step(0.02, &[top, lower]), Some(false));
        assert!(f.y > pos.1 as f64 && f.vy > 0.0);

        let mut f = Flight::new((450, 620), (100, 100), body(), (0.0, 700.0));
        for _ in 0..10 {
            f.step(0.02, &[top]);
        }
        assert!(
            f.y <= 660.0 && f.vy < 0.0,
            "without a neighboring screen the taskbar is the floor"
        );
    }

    #[test]
    fn left_edge_reflects_and_applies_friction_to_vertical_velocity() {
        let mut f = Flight::new((5, 300), (100, 100), body(), (-800.0, -200.0));
        f.step(0.02, &[screen(0, 0, 1000, 800)]);
        assert!(f.x >= 0.0 && f.vx > 0.0);
        assert!(f.vy > -200.0);
    }
}
