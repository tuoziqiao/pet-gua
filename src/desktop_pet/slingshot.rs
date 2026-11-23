//! Persistent slingshot interaction in physical desktop coordinates.
use super::{placement::Screen, throw::Body};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum Phase {
    Ready,
    Pulling((i32, i32)),
    Returning { from: (f64, f64), started: Instant },
}

pub(super) struct Session {
    pub original: (i32, i32),
    pub origin: (i32, i32),
    pub size: (i32, i32),
    pub body: Body,
    pub unit: f64,
    pub scale: f64,
    pub revision: u64,
    delta: (f64, f64),
    phase: Phase,
}

impl Session {
    pub fn new(original: (i32, i32), size: (i32, i32), body: Body, scale: f64, screens: &[Screen], revision: u64) -> Option<Self> {
        if size.0 <= 0 || size.1 <= 0 || !scale.is_finite() || scale <= 0.0 { return None; }
        let cx = original.0 as f64 + (body.left + body.right) * 0.5;
        let cy = original.1 as f64 + (body.top + body.bottom) * 0.5;
        let screen = screens.iter().min_by(|a,b| {
            let distance=|s: &Screen| {
                let x=(s.bounds.left as f64-cx).max(0.0).max(cx-s.bounds.right as f64);
                let y=(s.bounds.top as f64-cy).max(0.0).max(cy-s.bounds.bottom as f64);
                x*x+y*y
            };
            distance(a).total_cmp(&distance(b))
        })?;
        let unit=size.0 as f64/300.0;
        let foot=(cx, original.1 as f64+body.bottom);
        let base=(foot.0,foot.1+102.43*unit);
        let bounds=(
            (original.0 as f64+body.left).min(base.0-46.0*unit),
            (original.1 as f64+body.top).min(base.1-128.0*unit),
            (original.0 as f64+body.right).max(base.0+46.0*unit),
            (original.1 as f64+body.bottom).max(base.1),
        );
        let fit=|low:f64,high:f64,start:i32,end:i32| {
            let left=start as f64-low;
            let right=end as f64-high;
            if left > right { left } else { 0.0_f64.clamp(left,right) }
        };
        let shift=(fit(bounds.0,bounds.2,screen.work.left,screen.work.right),fit(bounds.1,bounds.3,screen.work.top,screen.work.bottom));
        let origin=((original.0 as f64+shift.0).round() as i32,(original.1 as f64+shift.1).round() as i32);
        Some(Self {original,origin,size,body,unit,scale,revision,delta:(0.0,0.0),phase:Phase::Ready})
    }

    pub fn is_ready(&self) -> bool { matches!(self.phase,Phase::Ready) }
    pub fn is_pulling(&self) -> bool { matches!(self.phase,Phase::Pulling(_)) }
    pub fn start_pull(&mut self, cursor: (i32,i32)) { self.phase=Phase::Pulling(cursor); }
    pub fn update_pull(&mut self, cursor: (i32,i32)) -> (i32,i32) {
        if let Phase::Pulling(start)=self.phase {
            let mut dx=cursor.0 as f64-start.0 as f64;
            let mut dy=cursor.1 as f64-start.1 as f64;
            let length=dx.hypot(dy);
            let max=self.size.0 as f64;
            if length>max { dx*=max/length; dy*=max/length; }
            self.delta=(dx,dy);
        }
        self.position()
    }
    pub fn position(&self) -> (i32,i32) {
        ((self.origin.0 as f64+self.delta.0).round() as i32,(self.origin.1 as f64+self.delta.1).round() as i32)
    }
    pub fn velocity(&self) -> Option<(f64,f64)> {
        let distance=self.delta.0.hypot(self.delta.1);
        let minimum=(self.size.0 as f64*20.0/300.0).max(12.0*self.scale);
        if distance < minimum { return None; }
        let factor=1800.0/(self.size.0 as f64*0.6);
        Some((-self.delta.0*factor,-self.delta.1*factor))
    }
    pub fn start_return(&mut self, now:Instant) {
        self.phase=Phase::Returning {from:self.delta,started:now};
    }
    pub fn update_return(&mut self, now:Instant) -> Option<(i32,i32)> {
        if let Phase::Returning {from,started}=self.phase {
            let progress=(now.saturating_duration_since(started).as_secs_f64()/Duration::from_millis(220).as_secs_f64()).clamp(0.0,1.0);
            let remaining=(1.0-progress).powi(3);
            self.delta=(from.0*remaining,from.1*remaining);
            if progress>=1.0 { self.delta=(0.0,0.0); self.phase=Phase::Ready; }
            Some(self.position())
        } else { None }
    }
    pub fn base(&self) -> (f64,f64) {
        (self.origin.0 as f64+(self.body.left+self.body.right)*0.5,self.origin.1 as f64+self.body.bottom+102.43*self.unit)
    }
    pub fn pouch(&self) -> (f64,f64) {
        let base=self.base();
        (base.0+self.delta.0,base.1-102.43*self.unit+self.delta.1)
    }
}
