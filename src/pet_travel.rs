//! 吉祥物带动岛体的随机行程。时间和随机种子由调用方提供，便于录制与测试。
use crate::anim::{ease_in_out, hash01};

const PREPARE: f64 = 0.8;
const OUTBOUND: f64 = 3.5;
const RETURN: f64 = 6.0;
pub(crate) const MIN_SPEED: u32 = 50;
pub(crate) const MAX_SPEED: u32 = 200;
pub(crate) const DEFAULT_SPEED: u32 = 100;
pub(crate) const MIN_DISTANCE: u32 = 80;
pub(crate) const MAX_DISTANCE: u32 = 400;
pub(crate) const DEFAULT_DISTANCE: u32 = 160;
pub(crate) const MIN_INTERVAL_MINUTES: u32 = 2;
pub(crate) const MAX_INTERVAL_MINUTES: u32 = 60;
pub(crate) const DEFAULT_INTERVAL_MINUTES: u32 = 10;

#[derive(Clone, Copy)]
struct Config {
    speed: u32,
    distance: u32,
    interval_minutes: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            speed: DEFAULT_SPEED,
            distance: DEFAULT_DISTANCE,
            interval_minutes: DEFAULT_INTERVAL_MINUTES,
        }
    }
}

impl Config {
    fn duration(self, distance: f32, base: f64) -> f64 {
        base * distance.abs() as f64 / DEFAULT_DISTANCE as f64 * DEFAULT_SPEED as f64
            / self.speed as f64
    }
}

/// 按最宽展开区域预留边界；输入为物理像素，输出为逻辑点。
pub(crate) fn safe_bounds(center_x: f32, left: f32, right: f32, ppp: f32) -> Option<(f32, f32)> {
    if !center_x.is_finite()
        || !left.is_finite()
        || !right.is_finite()
        || !ppp.is_finite()
        || ppp <= 0.0
    {
        return None;
    }
    let half = (crate::app::PW / 2.0 + crate::app::FLARE + 28.0) * ppp;
    let (min, max) = (left + half, right - half);
    if min > max || center_x < min || center_x > max {
        return None;
    }
    Some(((min - center_x) / ppp, (max - center_x) / ppp))
}

#[derive(Clone, Copy, Debug)]
enum Phase {
    Unscheduled,
    Waiting { until: f64 },
    Preparing { at: f64, target: f32, stay: f64 },
    Outbound { at: f64, target: f32, stay: f64 },
    Holding { until: f64, target: f32 },
    Returning { at: f64, from: f32 },
    Paused,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Frame {
    pub offset: f32,
    /// 抓取方向与动作已持续时间；停留和让位时不显示双手。
    pub pulling: Option<(f32, f32)>,
    pub moving: bool,
}

pub(crate) struct Travel {
    phase: Phase,
    seed: Option<u64>,
    random_index: u64,
    offset: f32,
    last_t: Option<f64>,
    next_config: Config,
    active_config: Config,
    /// None 表示尚未安排，或录制模式指定的开始时间。
    scheduled_interval: Option<u32>,
}

impl Default for Travel {
    fn default() -> Self {
        Self {
            phase: Phase::Unscheduled,
            seed: None,
            random_index: 0,
            offset: 0.0,
            last_t: None,
            next_config: Config::default(),
            active_config: Config::default(),
            scheduled_interval: None,
        }
    }
}

impl Travel {
    pub fn cancel(&mut self) {
        self.phase = Phase::Unscheduled;
        self.offset = 0.0;
        self.scheduled_interval = None;
    }

    /// 速度和距离在下一次行程生效；修改间隔会在等待时重新安排。
    pub fn configure(&mut self, speed: u32, distance: u32, interval_minutes: u32) {
        self.next_config = Config {
            speed: speed.clamp(MIN_SPEED, MAX_SPEED),
            distance: distance.clamp(MIN_DISTANCE, MAX_DISTANCE),
            interval_minutes: interval_minutes.clamp(MIN_INTERVAL_MINUTES, MAX_INTERVAL_MINUTES),
        };
    }

    fn random(&mut self) -> f32 {
        self.random_index = self.random_index.wrapping_add(1);
        hash01(
            self.seed
                .unwrap_or(0)
                .wrapping_add(self.random_index.wrapping_mul(0x9E37_79B9)),
        )
    }

    fn schedule(&mut self, now: f64) {
        let interval = self.next_config.interval_minutes;
        let wait = interval as f64 * 60.0 * (0.5 + self.random() as f64);
        self.phase = Phase::Waiting { until: now + wait };
        self.scheduled_interval = Some(interval);
    }

    fn target(&mut self, bounds: (f32, f32)) -> Option<f32> {
        let (left, right) = bounds;
        if !left.is_finite() || !right.is_finite() {
            return None;
        }
        let left_room = (-left).max(0.0);
        let right_room = right.max(0.0);
        let distance = self.active_config.distance as f32 * (0.75 + 0.5 * self.random());
        let prefer_left = self.random() < 0.5;
        let use_left = if prefer_left {
            left_room >= distance || left_room >= right_room
        } else {
            !(right_room >= distance || right_room >= left_room)
        };
        let room = if use_left { left_room } else { right_room };
        // 极窄显示器上避免只产生几个像素的无意义位移。
        if room < 24.0 {
            return None;
        }
        Some(distance.min(room) * if use_left { -1.0 } else { 1.0 })
    }

    /// `blocked` 表示用户正在操作或提醒需要显示；`hidden` 允许在不可见时复位。
    pub fn step(
        &mut self,
        now: f64,
        seed: u64,
        enabled: bool,
        blocked: bool,
        hidden: bool,
        bounds: Option<(f32, f32)>,
    ) -> Frame {
        if !now.is_finite() {
            return Frame::default();
        }
        if self.last_t.is_some_and(|last| now < last) {
            let next_config = self.next_config;
            *self = Self::default();
            self.next_config = next_config;
        }
        self.last_t = Some(now);
        self.seed.get_or_insert(seed);
        if hidden {
            self.phase = Phase::Unscheduled;
            self.offset = 0.0;
            self.scheduled_interval = None;
            return Frame::default();
        }

        // 等待期间修改间隔时立即重新计时；运行中的行程保留原有状态。
        if matches!(self.phase, Phase::Waiting { .. }) {
            if !enabled {
                self.phase = Phase::Unscheduled;
                self.scheduled_interval = None;
            } else if self
                .scheduled_interval
                .is_some_and(|interval| interval != self.next_config.interval_minutes)
            {
                self.schedule(now);
            }
        }

        if blocked
            && matches!(
                self.phase,
                Phase::Preparing { .. }
                    | Phase::Outbound { .. }
                    | Phase::Holding { .. }
                    | Phase::Returning { .. }
            )
        {
            self.phase = Phase::Paused;
        }
        let bounds = bounds.filter(|(left, right)| {
            left.is_finite() && right.is_finite() && *left <= 0.0 && *right >= 0.0
        });
        let Some((left, right)) = bounds else {
            // 工作区暂不可读时保留真实窗口位置，恢复后从当前位置返回。
            if !matches!(self.phase, Phase::Unscheduled | Phase::Waiting { .. }) {
                self.phase = Phase::Paused;
            }
            return Frame {
                offset: self.offset,
                pulling: None,
                moving: false,
            };
        };
        if !blocked {
            let destination = match self.phase {
                Phase::Preparing { target, .. }
                | Phase::Outbound { target, .. }
                | Phase::Holding { target, .. } => Some(target),
                _ => None,
            };
            if destination.is_some_and(|target| target < left || target > right) {
                // 监视器边界改变时平滑回到原位，而不是直接裁剪位置。
                self.phase = Phase::Returning {
                    at: now,
                    from: self.offset,
                };
            }
        }
        if !enabled {
            match self.phase {
                Phase::Unscheduled | Phase::Waiting { .. } => self.phase = Phase::Unscheduled,
                Phase::Paused if !blocked => {
                    self.phase = Phase::Returning {
                        at: now,
                        from: self.offset,
                    }
                }
                Phase::Preparing { .. } | Phase::Outbound { .. } | Phase::Holding { .. }
                    if !blocked =>
                {
                    self.phase = Phase::Returning {
                        at: now,
                        from: self.offset,
                    };
                }
                _ => {}
            }
        }

        // 使用绝对时间，帧率变化不会改变拖动、停留或返回的时长。
        for _ in 0..6 {
            match self.phase {
                Phase::Unscheduled if enabled => {
                    self.schedule(now);
                }
                Phase::Waiting { until } if enabled && !blocked && now >= until => {
                    self.active_config = self.next_config;
                    if let Some(target) = bounds.and_then(|bounds| self.target(bounds)) {
                        let stay = 20.0 + 20.0 * self.random() as f64;
                        self.phase = Phase::Preparing {
                            at: now,
                            target,
                            stay,
                        };
                    } else {
                        self.schedule(now);
                    }
                }
                Phase::Preparing { at, target, stay } if now - at >= PREPARE => {
                    self.phase = Phase::Outbound {
                        at: at + PREPARE,
                        target,
                        stay,
                    };
                    continue;
                }
                Phase::Outbound { at, target, stay } => {
                    let duration = self.active_config.duration(target, OUTBOUND);
                    let progress = if duration <= f64::EPSILON {
                        1.0
                    } else {
                        ((now - at) / duration).clamp(0.0, 1.0) as f32
                    };
                    self.offset = target * ease_in_out(progress);
                    if progress >= 1.0 {
                        self.phase = Phase::Holding {
                            until: at + duration + stay,
                            target,
                        };
                        continue;
                    }
                }
                Phase::Holding { until, target } if now >= until => {
                    self.phase = Phase::Returning {
                        at: until,
                        from: target,
                    };
                    continue;
                }
                Phase::Paused if !blocked => {
                    self.phase = Phase::Returning {
                        at: now,
                        from: self.offset,
                    };
                    continue;
                }
                Phase::Returning { at, from } => {
                    let duration = self.active_config.duration(from, RETURN);
                    let progress = if duration <= f64::EPSILON {
                        1.0
                    } else {
                        ((now - at) / duration).clamp(0.0, 1.0) as f32
                    };
                    self.offset = from * (1.0 - ease_in_out(progress));
                    if progress >= 1.0 {
                        self.offset = 0.0;
                        if enabled {
                            self.schedule(now);
                        } else {
                            self.phase = Phase::Unscheduled;
                        }
                    }
                }
                _ => {}
            }
            break;
        }

        let pulling = match self.phase {
            Phase::Preparing { at, target, .. } => {
                Some((target.signum(), (now - at).max(0.0) as f32))
            }
            Phase::Outbound { at, target, .. } => {
                Some((target.signum(), (now - at + PREPARE).max(0.0) as f32))
            }
            Phase::Returning { at, from } if enabled && from.abs() > 0.01 => {
                Some((-from.signum(), (now - at).max(0.0) as f32))
            }
            _ => None,
        };
        Frame {
            offset: self.offset,
            pulling,
            moving: matches!(
                self.phase,
                Phase::Preparing { .. } | Phase::Outbound { .. } | Phase::Returning { .. }
            ),
        }
    }

    #[cfg(any(test, feature = "record"))]
    pub fn start_demo(&mut self, at: f64) {
        self.phase = Phase::Waiting { until: at };
        self.scheduled_interval = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        travel: &mut Travel,
        t: f64,
        enabled: bool,
        blocked: bool,
        hidden: bool,
        bounds: Option<(f32, f32)>,
    ) -> Frame {
        travel.step(t, 42, enabled, blocked, hidden, bounds)
    }

    #[test]
    fn schedule_travel_hold_and_return_use_the_requested_time_ranges() {
        let mut travel = Travel::default();
        let bounds = Some((-500.0, 500.0));
        assert_eq!(
            run(&mut travel, 10.0, true, false, false, bounds).offset,
            0.0
        );
        let until = match travel.phase {
            Phase::Waiting { until } => until,
            _ => panic!("应先安排随机等待"),
        };
        assert!((310.0..=910.0).contains(&until));
        assert!(!run(&mut travel, until - 0.01, true, false, false, bounds).moving);
        assert!(run(&mut travel, until, true, false, false, bounds)
            .pulling
            .is_some());
        let (target, stay) = match travel.phase {
            Phase::Preparing { target, stay, .. } => (target, stay),
            _ => panic!("应伸手准备"),
        };
        assert!((120.0..=200.0).contains(&target.abs()));
        assert!((20.0..=40.0).contains(&stay));
        let outbound = travel.active_config.duration(target, OUTBOUND);
        let return_time = travel.active_config.duration(target, RETURN);
        let halfway = run(
            &mut travel,
            until + PREPARE + outbound / 2.0,
            true,
            false,
            false,
            bounds,
        )
        .offset;
        assert!((halfway - target / 2.0).abs() < 0.01);
        let end = until + PREPARE + outbound;
        assert!((run(&mut travel, end, true, false, false, bounds).offset - target).abs() < 0.01);
        assert!(
            (run(&mut travel, end + stay - 0.01, true, false, false, bounds).offset - target).abs()
                < 0.01
        );
        let returning = run(
            &mut travel,
            end + stay + return_time / 2.0,
            true,
            false,
            false,
            bounds,
        );
        assert!((returning.offset - target / 2.0).abs() < 0.01);
        assert_eq!(returning.pulling.unwrap().0, -target.signum());
        let returned = run(
            &mut travel,
            end + stay + return_time,
            true,
            false,
            false,
            bounds,
        );
        assert_eq!(returned.offset, 0.0);
        let next = match travel.phase {
            Phase::Waiting { until } => until,
            _ => panic!("返回后应重新安排"),
        };
        assert!((300.0..=900.0).contains(&(next - (end + stay + return_time))));
    }

    #[test]
    fn either_direction_is_possible_and_screen_edges_limit_the_trip() {
        let mut left = false;
        let mut right = false;
        for seed in 0..100 {
            let mut travel = Travel::default();
            travel.seed = Some(seed);
            let target = travel.target((-300.0, 300.0)).unwrap();
            left |= target < 0.0;
            right |= target > 0.0;
            assert!((120.0..=200.0).contains(&target.abs()));
        }
        assert!(left && right);
        let mut travel = Travel::default();
        travel.seed = Some(0);
        let target = travel.target((-10.0, 160.0)).unwrap();
        assert!((120.0..=160.0).contains(&target));
        assert!(travel.target((-10.0, 10.0)).is_none());
        for ppp in [1.0, 1.25, 1.5, 2.0] {
            let bounds = safe_bounds(960.0 * ppp, 0.0, 1920.0 * ppp, ppp).unwrap();
            assert!((bounds.0 + bounds.1).abs() < 0.01);
            assert!(bounds.1 > 200.0);
        }
        assert!(safe_bounds(100.0, 0.0, 400.0, 1.0).is_none());
    }

    #[test]
    fn interruption_freezes_then_returns_and_disable_never_restarts() {
        let bounds = Some((-300.0, 300.0));
        let mut travel = Travel::default();
        travel.start_demo(0.0);
        run(&mut travel, 0.0, true, false, false, bounds);
        let moving = run(&mut travel, 2.5, true, false, false, bounds);
        assert!(moving.offset.abs() > 1.0);
        let paused = run(&mut travel, 2.6, true, true, false, bounds);
        assert_eq!(paused.offset, moving.offset);
        assert!(paused.pulling.is_none());
        assert_eq!(
            run(&mut travel, 30.0, false, true, false, bounds).offset,
            paused.offset
        );
        assert_eq!(
            run(&mut travel, 30.1, false, false, false, bounds).offset,
            paused.offset
        );
        let return_time = travel.active_config.duration(paused.offset, RETURN);
        let halfway = run(
            &mut travel,
            30.1 + return_time / 2.0,
            false,
            false,
            false,
            bounds,
        )
        .offset;
        assert!((halfway - paused.offset / 2.0).abs() < 0.01);
        assert_eq!(
            run(
                &mut travel,
                30.1 + return_time + 0.01,
                false,
                false,
                false,
                bounds
            )
            .offset,
            0.0
        );
        assert_eq!(
            run(&mut travel, 1000.0, false, false, false, bounds).offset,
            0.0
        );
        assert!(matches!(travel.phase, Phase::Unscheduled));
    }

    #[test]
    fn hidden_resets_and_changed_or_lost_bounds_return_without_jumping() {
        let mut travel = Travel::default();
        travel.start_demo(0.0);
        run(&mut travel, 0.0, true, false, false, Some((-300.0, 300.0)));
        let before = run(&mut travel, 3.0, true, false, false, Some((-300.0, 300.0))).offset;
        assert!(before.abs() > 0.0);
        let reduced = run(&mut travel, 3.1, true, false, false, Some((-40.0, 40.0)));
        assert_eq!(reduced.offset, before);
        assert_eq!(
            run(&mut travel, 3.2, true, false, true, Some((-40.0, 40.0))).offset,
            0.0
        );
        travel.start_demo(4.0);
        run(&mut travel, 4.0, true, false, false, Some((-300.0, 300.0)));
        let before = run(&mut travel, 7.0, true, false, false, Some((-300.0, 300.0))).offset;
        assert_eq!(
            run(&mut travel, 7.1, true, true, false, None).offset,
            before
        );
        assert_eq!(
            run(&mut travel, 7.2, true, false, false, None).offset,
            before
        );
        assert_eq!(
            run(&mut travel, 7.3, true, false, false, Some((-300.0, 300.0))).offset,
            before
        );
        let return_time = travel.active_config.duration(before, RETURN);
        assert_eq!(
            run(
                &mut travel,
                7.3 + return_time + 0.01,
                true,
                false,
                false,
                Some((-300.0, 300.0))
            )
            .offset,
            0.0
        );
    }
}
