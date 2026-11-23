//! 身体动作与表情独立组合。坐标基于 160×140 角色画布。
use super::expressions::Context;
use crate::pet_expressions::Expression;
use eframe::egui::{self, Pos2, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MotionKind { Breathe, Bounce, Drag, Stretch, Approach, Music, SleepWake }

pub(crate) const MOTIONS: [(MotionKind, &str); 7] = [
    (MotionKind::Breathe, "待机呼吸"), (MotionKind::Bounce, "点击弹跳"),
    (MotionKind::Drag, "拖拽变形"), (MotionKind::Stretch, "伸懒腰"),
    (MotionKind::Approach, "鼠标靠近"), (MotionKind::Music, "听歌摇摆"),
    (MotionKind::SleepWake, "打盹与醒来"),
];

#[derive(Clone, Copy)]
pub(super) struct BodyPose { pub scale_x: f32, pub scale_y: f32, pub skew: f32, pub offset: Vec2 }
impl Default for BodyPose {
    fn default() -> Self { Self { scale_x: 1.0, scale_y: 1.0, skew: 0.0, offset: Vec2::ZERO } }
}
impl BodyPose {
    pub fn map(self, point: Pos2) -> Pos2 {
        let x=point.x-80.0;
        let y=point.y-70.0;
        egui::pos2(80.0+x*self.scale_x+y*self.skew+self.offset.x, 70.0+y*self.scale_y+self.offset.y)
    }
    fn bounded(mut self) -> Self {
        self.scale_x=self.scale_x.clamp(0.73,1.27);
        self.scale_y=self.scale_y.clamp(0.73,1.27);
        self.skew=self.skew.clamp(-0.216,0.216);
        self.offset.x=self.offset.x.clamp(-18.9,18.9);
        self.offset.y=self.offset.y.clamp(-16.2,16.2);
        // 按真实轮廓限制整体变形，给窗口边缘留出一单位的余量。
        let mut amount: f32=1.0;
        for point in super::outline() {
            let mapped=self.map(point);
            for (base, delta, max) in [(point.x,mapped.x-point.x,super::SIZE.x-1.0), (point.y,mapped.y-point.y,super::SIZE.y-1.0)] {
                if delta>0.0 { amount=amount.min((max-base)/delta); }
                else if delta<0.0 { amount=amount.min((1.0-base)/delta); }
            }
        }
        let amount=amount.clamp(0.0,1.0);
        self.scale_x=1.0+(self.scale_x-1.0)*amount;
        self.scale_y=1.0+(self.scale_y-1.0)*amount;
        self.skew*=amount;
        self.offset*=amount;
        self
    }
}

pub(super) struct Motion {
    time: f32,
    pose: BodyPose,
    preview: Option<(MotionKind,f32)>,
    bounce: Option<f32>,
    release: Option<f32>,
    stretch: Option<f32>,
    stretch_wait: f32,
    stretch_index: u64,
    previous_idle: f64,
    wake: Option<f32>,
    drag_direction: f32,
}

impl Default for Motion {
    fn default() -> Self { Self { time: 0.0, pose: BodyPose::default(), preview: None, bounce: None, release: None, stretch: None, stretch_wait: 25.0, stretch_index: 0, previous_idle: 0.0, wake: None, drag_direction: 0.0 } }
}

impl Motion {
    pub fn preview(&mut self, kind: MotionKind) { self.preview=Some((kind,0.0)); }
    pub fn click(&mut self) { self.bounce=Some(0.0); }
    pub fn released(&mut self) { self.release=Some(0.0); }
    pub fn step(&mut self, dt: f32, enabled: bool, context: Context, cursor: Option<Pos2>, dragging: bool, delta_x: f32, expression_active: bool, expression_cue: Option<(Expression,f32,f32)>) -> BodyPose {
        if !enabled || !context.animations {
            self.pose=BodyPose::default(); self.preview=None; self.bounce=None; self.release=None; self.stretch=None; self.wake=None;
            self.previous_idle=context.idle;
            return self.pose;
        }
        let dt=dt.clamp(0.0,0.1);
        self.time+=dt;
        if self.previous_idle>=context.idle_threshold && context.idle<1.0 { self.wake=Some(0.0); }
        self.previous_idle=context.idle;
        self.drag_direction=crate::anim::approach(self.drag_direction,(delta_x/10.0).clamp(-1.0,1.0),9.0,dt);
        let near=cursor.filter(|p| (-16.0..=176.0).contains(&p.x) && (-16.0..=156.0).contains(&p.y));
        let idle=context.idle < context.idle_threshold && !context.focus && !context.worried && !context.music && near.is_none() && !expression_active;
        if idle && !dragging && self.preview.is_none() && self.bounce.is_none() && self.release.is_none() && self.wake.is_none() && self.stretch.is_none() {
            self.stretch_wait-=dt;
            if self.stretch_wait<=0.0 { self.stretch=Some(0.0); self.stretch_index+=1; self.stretch_wait=20.0+crate::anim::hash01(self.stretch_index)*15.0; }
        }
        let mut target=BodyPose::default();
        let (kind, age, previewing)=if dragging { (MotionKind::Drag,self.time,false) }
            else if let Some(age)=self.release.as_mut() { *age+=dt; let at=*age; if at>=0.75 { self.release=None; } (MotionKind::Bounce,at,false) }
            else if let Some((kind,age))=self.preview.as_mut() { *age+=dt; let result=(*kind,*age,true); if *age>=4.5 { self.preview=None; } result }
            else if let Some(age)=self.bounce.as_mut() { *age+=dt; let at=*age; if at>=0.85 { self.bounce=None; } (MotionKind::Bounce,at,false) }
            else if let Some(age)=self.wake.as_mut() { *age+=dt; let at=*age; if at>=1.8 { self.wake=None; } (MotionKind::SleepWake,2.5+at,false) }
            else if context.idle>=context.idle_threshold { (MotionKind::SleepWake,0.0,false) }
            else if context.music { (MotionKind::Music,self.time,false) }
            else if near.is_some() { (MotionKind::Approach,self.time,false) }
            else if let Some(age)=self.stretch.as_mut() { *age+=dt; let at=*age; if at>=2.2 { self.stretch=None; } (MotionKind::Stretch,at,false) }
            else { (MotionKind::Breathe,self.time,false) };
        let age=if previewing { match kind { MotionKind::Bounce => age%1.25, MotionKind::Stretch => age%2.7, _ => age } } else { age };
        match kind {
            MotionKind::Breathe => { let v=(age*1.35).sin(); target.scale_x=1.0+0.04*v; target.scale_y=1.0-0.04*v; }
            MotionKind::Bounce => { let v=(age*10.0).sin()*(-age*3.4).exp(); target.scale_x=1.0+0.12*v; target.scale_y=1.0-0.15*v; target.offset.y=-6.0*(age*5.0).sin().max(0.0)*(-age*2.0).exp(); }
            MotionKind::Drag => { let dir=if dragging { self.drag_direction } else {(age*2.0).sin()}; target.scale_x=1.06; target.scale_y=0.93; target.skew=0.06*dir; target.offset.x=-5.0*dir; }
            MotionKind::Stretch => { let v=(std::f32::consts::PI*(age/2.2).clamp(0.0,1.0)).sin(); target.scale_x=1.0-0.075*v; target.scale_y=1.0+0.09*v; target.skew=(age*4.0).sin()*0.025*v; }
            MotionKind::Approach => { let dir=if previewing { 0.85 } else { near.map_or(0.0,|p| ((p.x-80.0)/80.0).clamp(-1.0,1.0)) }; target.skew=-0.055*dir; target.offset.x=4.5*dir; target.scale_y=1.045; }
            MotionKind::Music => { let v=(age*2.3).sin(); target.skew=v*0.06; target.offset.x=v*4.0; target.scale_y=1.0+0.035*(age*4.6).sin(); }
            MotionKind::SleepWake => { if age<2.5 { target.scale_y=0.91+0.018*(self.time*0.8).sin(); target.scale_x=1.06; target.offset.y=4.0; } else { let v=(std::f32::consts::PI*((age-2.5)/1.8).clamp(0.0,1.0)).sin(); target.scale_y=1.0+0.09*v; target.scale_x=1.0-0.06*v; } }
        }
        // 七种身体动作相对静止姿态的幅度统一放大。
        target.scale_x=1.0+(target.scale_x-1.0)*2.7;
        target.scale_y=1.0+(target.scale_y-1.0)*2.7;
        target.skew*=2.7;
        target.offset*=2.7;
        // 低优先级的表情身体动作由动作层演绎；互动和显式预览不会被它覆盖。
        if kind==MotionKind::Breathe && !previewing {
            if let Some((expression,age,duration))=expression_cue {
                let amount=(age/0.2).min(1.0).min(((duration-age)/0.3).clamp(0.0,1.0));
                let wave=(age*5.0).sin()*amount;
                match expression {
                    Expression::Delighted | Expression::Heart => { target.scale_y-=0.02*wave; target.scale_x+=0.016*wave; }
                    Expression::Giggle => target.offset.x+=1.5*wave,
                    Expression::ShyPeek => target.offset.y+=1.5*amount,
                    Expression::Scan => target.offset.x+=(age*1.2).sin()*amount,
                    Expression::Peek => target.offset.y-=1.5*amount,
                    _ => {}
                }
            }
        }
        target=target.bounded();
        let rate=if matches!(kind,MotionKind::Bounce | MotionKind::Drag) {18.0} else {5.0};
        self.pose.scale_x=crate::anim::approach(self.pose.scale_x,target.scale_x,rate,dt);
        self.pose.scale_y=crate::anim::approach(self.pose.scale_y,target.scale_y,rate,dt);
        self.pose.skew=crate::anim::approach(self.pose.skew,target.skew,rate,dt);
        self.pose.offset.x=crate::anim::approach(self.pose.offset.x,target.offset.x,rate,dt);
        self.pose.offset.y=crate::anim::approach(self.pose.offset.y,target.offset.y,rate,dt);
        self.pose=self.pose.bounded();
        self.pose
    }
}
