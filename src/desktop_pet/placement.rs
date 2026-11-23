//! Desktop-pet movement in virtual-desktop pixel coordinates.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    fn valid(self) -> bool {
        self.left < self.right && self.top < self.bottom
    }

    fn width(self) -> i64 {
        i64::from(self.right) - i64::from(self.left)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Screen {
    pub bounds: Rect,
    pub work: Rect,
}

/// The order is independent of the order in which Windows enumerates monitors.
pub(super) fn sort_screens(screens: &mut Vec<Screen>) {
    screens.retain(|screen| screen.bounds.valid() && screen.work.valid());
    screens.sort_by_key(|screen| {
        let b = screen.bounds;
        (b.left, b.top, b.right, b.bottom)
    });
    screens.dedup_by_key(|screen| screen.bounds);
}

fn overlap(rect: Rect, pos: (i32, i32), size: (i32, i32)) -> i128 {
    let left = i64::from(pos.0).max(i64::from(rect.left));
    let top = i64::from(pos.1).max(i64::from(rect.top));
    let right = (i64::from(pos.0) + i64::from(size.0)).min(i64::from(rect.right));
    let bottom = (i64::from(pos.1) + i64::from(size.1)).min(i64::from(rect.bottom));
    i128::from((right - left).max(0)) * i128::from((bottom - top).max(0))
}

fn nearest_screen(screens: &[Screen], pos: (i32, i32), size: (i32, i32)) -> usize {
    let center = (
        i64::from(pos.0) + i64::from(size.0) / 2,
        i64::from(pos.1) + i64::from(size.1) / 2,
    );
    screens
        .iter()
        .enumerate()
        .min_by_key(|(_, screen)| {
            let area = overlap(screen.work, pos, size);
            let dx = (i64::from(screen.work.left) - center.0)
                .max(0)
                .max(center.0 - i64::from(screen.work.right));
            let dy = (i64::from(screen.work.top) - center.1)
                .max(0)
                .max(center.1 - i64::from(screen.work.bottom));
            (
                -area,
                i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy),
            )
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn clamp_y(y: i64, height: i64, work: Rect) -> i64 {
    y.clamp(
        i64::from(work.top),
        (i64::from(work.bottom) - height).max(i64::from(work.top)),
    )
}

fn as_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Step {
    pub position: (i32, i32),
    pub crossed: bool,
    pub teleported: bool,
}

/// Remembers which side of a shared edge owns a window while it straddles it.
#[derive(Default)]
pub(super) struct Navigator {
    current: Option<Rect>,
}

impl Navigator {
    pub fn reset(&mut self) {
        self.current = None;
    }

    pub fn advance(
        &mut self,
        screens: &[Screen],
        pos: (i32, i32),
        size: (i32, i32),
        delta: i32,
    ) -> Option<Step> {
        if screens.is_empty() || size.0 <= 0 || size.1 <= 0 {
            return None;
        }

        let mut current = self
            .current
            .and_then(|bounds| screens.iter().position(|screen| screen.bounds == bounds))
            .filter(|&index| overlap(screens[index].bounds, pos, size) > 0)
            .unwrap_or_else(|| nearest_screen(screens, pos, size));
        let width = i64::from(size.0);
        let height = i64::from(size.1);
        let mut x = i64::from(pos.0);
        let mut y = clamp_y(i64::from(pos.1), height, screens[current].work);
        if !screens
            .iter()
            .any(|screen| overlap(screen.work, pos, size) > 0)
        {
            let work = screens[current].work;
            x = x.clamp(
                i64::from(work.left),
                (i64::from(work.right) - width).max(i64::from(work.left)),
            );
        }
        let mut remaining = i64::from(delta).abs();
        let right = delta > 0;
        let mut crossed = false;
        let mut teleported = false;
        let mut transitions = 0usize;

        while remaining > 0 {
            let source = screens[current];
            // There is no horizontal room to walk when the window fills the screen.
            // Keep it at the entering edge rather than repeatedly wrapping in place.
            if width >= source.work.width() {
                x = i64::from(source.work.left);
                break;
            }
            let next = if right {
                (current + 1) % screens.len()
            } else {
                (current + screens.len() - 1) % screens.len()
            };
            let target = screens[next];
            let wraps = if right {
                next <= current
            } else {
                next >= current
            };
            let shared_edge = if right {
                source.work.right == target.work.left
            } else {
                source.work.left == target.work.right
            };
            let shared_top = i64::from(source.work.top.max(target.work.top));
            let shared_bottom = i64::from(source.work.bottom.min(target.work.bottom));
            let continuous = !wraps
                && shared_edge
                && shared_bottom - shared_top >= height
                && width < target.work.width();

            if continuous {
                let edge = if right {
                    source.work.right
                } else {
                    source.work.left
                };
                let lead = if right {
                    i64::from(edge) - width
                } else {
                    i64::from(edge)
                };
                let end = if right { x + remaining } else { x - remaining };
                if (right && end > lead) || (!right && end < lead) {
                    // Adjust before any part of the window enters the other work area.
                    y = y.clamp(shared_top, shared_bottom - height);
                }
                let switch = i64::from(edge) - width / 2;
                let to_switch = if right { switch - x } else { x - switch }.max(0);
                if remaining < to_switch {
                    x = end;
                    break;
                }
                x += if right { to_switch } else { -to_switch };
                remaining -= to_switch;
                current = next;
                crossed = true;
            } else {
                let edge = if right {
                    i64::from(source.work.right) - width
                } else {
                    i64::from(source.work.left)
                };
                let to_edge = if right { edge - x } else { x - edge }.max(0);
                if remaining <= to_edge {
                    x += if right { remaining } else { -remaining };
                    break;
                }
                remaining -= to_edge;
                x = if right {
                    i64::from(target.work.left)
                } else {
                    (i64::from(target.work.right) - width).max(i64::from(target.work.left))
                };
                y = clamp_y(y, height, target.work);
                current = next;
                crossed = true;
                teleported = true;
            }
            transitions += 1;
            // Avoid an unbounded loop for degenerate layouts or unrealistic deltas.
            if transitions > screens.len().saturating_mul(4).max(16) {
                break;
            }
        }

        self.current = Some(screens[current].bounds);
        Some(Step {
            position: (as_i32(x), as_i32(y)),
            crossed,
            teleported,
        })
    }

    /// Reapply vertical placement after Windows changes the physical window size.
    pub fn fit_after_resize(
        &self,
        screens: &[Screen],
        pos: (i32, i32),
        size: (i32, i32),
        fit_x: bool,
    ) -> (i32, i32) {
        let Some(screen) = self
            .current
            .and_then(|bounds| screens.iter().find(|s| s.bounds == bounds))
        else {
            return pos;
        };
        let work = screen.work;
        let x = if fit_x {
            i64::from(pos.0).clamp(
                i64::from(work.left),
                (i64::from(work.right) - i64::from(size.0)).max(i64::from(work.left)),
            )
        } else {
            i64::from(pos.0)
        };
        (
            as_i32(x),
            as_i32(clamp_y(i64::from(pos.1), i64::from(size.1), work)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn screen(left: i32, top: i32, right: i32, bottom: i32) -> Screen {
        let bounds = rect(left, top, right, bottom);
        Screen {
            bounds,
            work: bounds,
        }
    }

    fn step(nav: &mut Navigator, screens: &[Screen], pos: (i32, i32), delta: i32) -> Step {
        nav.advance(screens, pos, (100, 100), delta).unwrap()
    }

    #[test]
    fn sorting_uses_desktop_coordinates_and_discards_invalid_rectangles() {
        let mut screens = vec![
            screen(0, 800, 1000, 1600),
            screen(1000, 0, 1800, 600),
            screen(-800, 0, 0, 600),
            screen(0, 0, 1000, 800),
            screen(0, 0, 0, 800),
        ];
        sort_screens(&mut screens);
        assert_eq!(
            screens
                .iter()
                .map(|s| (s.bounds.left, s.bounds.top))
                .collect::<Vec<_>>(),
            vec![(-800, 0), (0, 0), (0, 800), (1000, 0)]
        );
    }

    #[test]
    fn adjacent_screens_cross_smoothly_and_adjust_to_the_shorter_height() {
        let screens = [screen(0, 0, 1000, 800), screen(1000, 0, 1800, 600)];
        let mut nav = Navigator::default();
        let first = step(&mut nav, &screens, (880, 650), 30);
        assert_eq!(
            first,
            Step {
                position: (910, 500),
                crossed: false,
                teleported: false
            }
        );
        let second = step(&mut nav, &screens, first.position, 50);
        assert_eq!(
            second,
            Step {
                position: (960, 500),
                crossed: true,
                teleported: false
            }
        );
        assert_eq!(
            step(&mut nav, &screens, second.position, 30).position,
            (990, 500)
        );

        let mut reverse = Navigator::default();
        let back = step(&mut reverse, &screens, (1020, 500), -80);
        assert_eq!(
            back,
            Step {
                position: (940, 500),
                crossed: true,
                teleported: false
            }
        );
        assert_eq!(
            step(&mut reverse, &screens, back.position, -20).position,
            (920, 500)
        );
    }

    #[test]
    fn outside_edges_wrap_in_both_directions_without_losing_the_remainder() {
        let screens = [screen(0, 0, 1000, 800), screen(1000, 0, 1800, 600)];
        let mut nav = Navigator::default();
        let right = step(&mut nav, &screens, (1690, 400), 25);
        assert_eq!(
            right,
            Step {
                position: (15, 400),
                crossed: true,
                teleported: true
            }
        );
        assert_eq!(
            step(&mut nav, &screens, right.position, 20).position,
            (35, 400)
        );

        let mut nav = Navigator::default();
        let left = step(&mut nav, &screens, (10, 650), -20);
        assert_eq!(
            left,
            Step {
                position: (1690, 500),
                crossed: true,
                teleported: true
            }
        );
        assert_eq!(
            step(&mut nav, &screens, left.position, -15).position,
            (1675, 500)
        );
    }

    #[test]
    fn single_screen_wraps_only_when_moving_past_the_edge() {
        let screens = [screen(0, 0, 1000, 800)];
        let mut nav = Navigator::default();
        assert_eq!(
            step(&mut nav, &screens, (890, 200), 10).position,
            (900, 200)
        );
        let wrapped = step(&mut nav, &screens, (900, 200), 10);
        assert_eq!(
            wrapped,
            Step {
                position: (10, 200),
                crossed: true,
                teleported: true
            }
        );
        let back = step(&mut nav, &screens, wrapped.position, -20);
        assert_eq!(back.position, (890, 200));
    }

    #[test]
    fn gaps_and_disjoint_heights_jump_to_the_next_work_area() {
        let screens = [screen(0, 0, 1000, 800), screen(1100, 0, 1900, 600)];
        let mut nav = Navigator::default();
        let next = step(&mut nav, &screens, (890, 650), 20);
        assert_eq!(
            next,
            Step {
                position: (1110, 500),
                crossed: true,
                teleported: true
            }
        );
        let back = step(&mut nav, &screens, next.position, -20);
        assert_eq!(back.position, (890, 500));

        let stacked = [screen(0, 0, 1000, 600), screen(1000, 900, 1800, 1500)];
        let mut nav = Navigator::default();
        assert_eq!(
            step(&mut nav, &stacked, (890, 400), 20).position,
            (1010, 900)
        );
    }

    #[test]
    fn height_clamping_uses_work_areas_and_handles_oversized_windows() {
        let screens = [Screen {
            bounds: rect(-1000, -100, 0, 800),
            work: rect(-1000, -50, 0, 700),
        }];
        let mut nav = Navigator::default();
        assert_eq!(
            nav.advance(&screens, (-500, 680), (100, 100), 0)
                .unwrap()
                .position,
            (-500, 600)
        );
        assert_eq!(
            nav.advance(&screens, (-500, 100), (100, 1000), 0)
                .unwrap()
                .position,
            (-500, -50)
        );
        let wide = nav.advance(&screens, (-500, 100), (1100, 100), 50).unwrap();
        assert_eq!(wide.position, (-1000, 100));
        assert!(!wide.crossed);
        assert!(
            !nav.advance(&screens, wide.position, (1100, 100), 50)
                .unwrap()
                .crossed
        );
    }

    #[test]
    fn interrupting_or_changing_layout_reselects_a_visible_screen() {
        let original = [screen(-1000, 0, 0, 800), screen(0, 0, 1000, 800)];
        let mut nav = Navigator::default();
        let partial = step(&mut nav, &original, (-120, 300), 40);
        assert_eq!(partial.position, (-80, 300));
        nav.reset(); // A click or drag interrupts the movement animation.
        assert_eq!(
            step(&mut nav, &original, partial.position, -20).position,
            (-100, 300)
        );

        let changed = [screen(0, 100, 1000, 700)];
        let after_disconnect = step(&mut nav, &changed, (-100, 750), 10);
        assert_eq!(after_disconnect.position, (10, 600));
        assert!(!after_disconnect.teleported);
    }

    #[test]
    fn resize_fit_keeps_the_window_inside_after_a_wrap() {
        let screens = [screen(0, 0, 1000, 800), screen(1000, 0, 1800, 600)];
        let mut nav = Navigator::default();
        let wrapped = step(&mut nav, &screens, (10, 500), -20);
        assert!(wrapped.teleported);
        assert_eq!(
            nav.fit_after_resize(&screens, wrapped.position, (200, 250), true),
            (1600, 350)
        );
    }
}
