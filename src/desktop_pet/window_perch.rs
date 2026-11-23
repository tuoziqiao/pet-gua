//! A desktop pet temporarily supported by the visible top edge of a window.
use super::{
    placement::{Rect, Screen},
    throw::Body,
};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{BOOL, HWND, LPARAM, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
    System::Threading::GetCurrentProcessId,
    UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
        IsIconic, IsWindow, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE, WS_CHILD, WS_EX_TOOLWINDOW,
    },
};

#[derive(Clone, Copy)]
struct Window {
    handle: isize,
    pid: u32,
    rect: Rect,
}

unsafe fn read_window(hwnd: HWND) -> Option<Window> {
    if !IsWindow(hwnd).as_bool() || !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        return None;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == GetCurrentProcessId() {
        return None;
    }
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if style & WS_CHILD.0 != 0 || exstyle & WS_EX_TOOLWINDOW.0 != 0 {
        return None;
    }
    let mut class = [0u16; 128];
    let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
    if matches!(
        String::from_utf16_lossy(&class[..n]).as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
    ) {
        return None;
    }
    let mut cloaked = 0u32;
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        (&mut cloaked as *mut u32).cast(),
        std::mem::size_of::<u32>() as u32,
    )
    .is_ok()
        && cloaked != 0
    {
        return None;
    }
    let mut rect = RECT::default();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS,
        (&mut rect as *mut RECT).cast(),
        std::mem::size_of::<RECT>() as u32,
    )
    .is_err()
    {
        GetWindowRect(hwnd, &mut rect).ok()?;
    }
    if rect.left >= rect.right || rect.top >= rect.bottom {
        return None;
    }
    Some(Window {
        handle: hwnd.0 as isize,
        pid,
        rect: Rect {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        },
    })
}

unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
    let windows = &mut *(data.0 as *mut Vec<Window>);
    if let Some(window) = read_window(hwnd) {
        windows.push(window);
    }
    BOOL(1)
}

/// Desktop contact points covered by ordinary windows must not collide.
pub(super) fn desktop_point_visible(point: (i32, i32)) -> bool {
    let mut windows: Vec<Window> = Vec::new();
    if unsafe { EnumWindows(Some(collect), LPARAM((&mut windows as *mut Vec<Window>) as isize)) }.is_err() { return false; }
    !windows.iter().any(|window| {
        let r=window.rect;
        point.0>=r.left && point.0<r.right && point.1>=r.top && point.1<r.bottom
    })
}

fn fits_screen(pos: (i32, i32), body: Body, screens: &[Screen]) -> bool {
    let l = pos.0 as f64 + body.left;
    let r = pos.0 as f64 + body.right;
    let t = pos.1 as f64 + body.top;
    let b = pos.1 as f64 + body.bottom;
    screens.iter().any(|s| {
        l >= s.work.left as f64
            && r <= s.work.right as f64
            && t >= s.work.top as f64
            && b <= s.work.bottom as f64
    })
}

fn position(rect: Rect, offset: f64, body: Body, screens: &[Screen]) -> Option<(i32, i32)> {
    let half = (body.right - body.left) / 2.0;
    if half <= 0.0 || ((rect.right - rect.left) as f64) < 2.0 * half {
        return None;
    }
    let center =
        (rect.left as f64 + offset).clamp(rect.left as f64 + half, rect.right as f64 - half);
    let pos = (
        (center - (body.left + body.right) / 2.0).round() as i32,
        (rect.top as f64 - body.bottom).round() as i32,
    );
    fits_screen(pos, body, screens).then_some(pos)
}

pub(super) struct Perch {
    target: Window,
    offset: f64,
    body: Body,
    size: (i32, i32),
    idle_body: bool,
    samples: VecDeque<(Instant, i32, i32)>,
}

pub(super) enum Update {
    Follow((i32, i32)),
    Drop,
}

impl Perch {
    /// The first suitable visible top edge in front-to-back Z order wins.
    pub fn find(
        pos: (i32, i32),
        size: (i32, i32),
        body: Body,
        screens: &[Screen],
        now: Instant,
    ) -> Option<(Self, (i32, i32))> {
        let mut windows: Vec<Window> = Vec::new();
        unsafe {
            EnumWindows(
                Some(collect),
                LPARAM((&mut windows as *mut Vec<Window>) as isize),
            )
            .ok()?;
        }
        let center = pos.0 as f64 + (body.left + body.right) / 2.0;
        let feet = pos.1 as f64 + body.bottom;
        let tolerance = (size.0 as f64 * 0.07).clamp(12.0, 32.0);
        for (index, window) in windows.iter().enumerate() {
            let rect = window.rect;
            if center < rect.left as f64
                || center >= rect.right as f64
                || (feet - rect.top as f64).abs() > tolerance
            {
                continue;
            }
            let covered = windows[..index].iter().any(|other| {
                let r = other.rect;
                center >= r.left as f64
                    && center < r.right as f64
                    && rect.top >= r.top
                    && rect.top < r.bottom
            });
            if covered {
                continue;
            }
            let offset = center - rect.left as f64;
            let Some(next) = position(rect, offset, body, screens) else {
                continue;
            };
            let mut samples = VecDeque::new();
            samples.push_back((now, rect.left, rect.top));
            return Some((
                Self {
                    target: *window,
                    offset,
                    body,
                    size,
                    idle_body: false,
                    samples,
                },
                next,
            ));
        }
        None
    }

    pub fn body(&self) -> Body {
        self.body
    }

    pub fn update_body(
        &mut self,
        size: (i32, i32),
        idle_mask: Option<(&[u8], [usize; 2])>,
        mirror: bool,
    ) {
        if size.0 <= 0 || size.1 <= 0 {
            return;
        }
        if !self.idle_body && idle_mask.is_some() {
            self.body = Body::from_mask(size, idle_mask, mirror);
            self.idle_body = true;
            self.size = size;
        } else if size != self.size && self.size.0 > 0 && self.size.1 > 0 {
            let sx = size.0 as f64 / self.size.0 as f64;
            let sy = size.1 as f64 / self.size.1 as f64;
            self.body.left *= sx;
            self.body.right *= sx;
            self.body.top *= sy;
            self.body.bottom *= sy;
            self.size = size;
        }
    }

    pub fn update(&mut self, now: Instant, screens: &[Screen]) -> Update {
        let hwnd = HWND(self.target.handle as *mut _);
        let Some(window) = (unsafe { read_window(hwnd) }) else {
            return Update::Drop;
        };
        if window.pid != self.target.pid {
            return Update::Drop;
        }
        self.samples
            .push_back((now, window.rect.left, window.rect.top));
        while self.samples.len() > 2
            && now.saturating_duration_since(self.samples[1].0) >= Duration::from_millis(100)
        {
            self.samples.pop_front();
        }
        if let Some(&(time, x, y)) = self.samples.front() {
            let dt = now.saturating_duration_since(time).as_secs_f64();
            if dt >= 0.06 && dt <= 0.5 {
                let dx = window.rect.left as f64 - x as f64;
                let dy = window.rect.top as f64 - y as f64;
                if dx.hypot(dy) / dt > 1200.0 {
                    return Update::Drop;
                }
            } else if dt > 0.5 {
                self.samples.clear();
                self.samples
                    .push_back((now, window.rect.left, window.rect.top));
            }
        }
        self.target = window;
        match position(window.rect, self.offset, self.body, screens) {
            Some(pos) => Update::Follow(pos),
            None => Update::Drop,
        }
    }
}
