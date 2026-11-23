//! Physical-pixel, click-through slingshot layers.

#[cfg(windows)]
#[derive(Clone, PartialEq)]
pub(super) struct Scene {
    pub base: (f64,f64),
    pub pouch: (f64,f64),
    pub unit: f64,
    pub scale: f64,
    pub dots: Vec<(f64,f64)>,
    pub screens: Vec<super::placement::Rect>,
}

#[cfg(windows)]
struct Assets {
    sling: image::RgbaImage,
    band: image::RgbaImage,
    pouch: image::RgbaImage,
    bubbles: [image::RgbaImage;3],
}

#[cfg(windows)]
fn assets() -> Option<&'static Assets> {
    use std::sync::OnceLock;
    static ASSETS: OnceLock<Option<Assets>>=OnceLock::new();
    ASSETS.get_or_init(|| {
        let decode=|bytes: &[u8]| image::load_from_memory_with_format(bytes,image::ImageFormat::Png).ok().map(|image|image.to_rgba8());
        Some(Assets {
            sling:decode(include_bytes!("../../assets/desktop-pet/slingshot/sling.png"))?,
            band:decode(include_bytes!("../../assets/desktop-pet/slingshot/band.png"))?,
            pouch:decode(include_bytes!("../../assets/desktop-pet/slingshot/pouch.png"))?,
            bubbles:[
                decode(include_bytes!("../../assets/desktop-pet/slingshot/bubble-s1.png"))?,
                decode(include_bytes!("../../assets/desktop-pet/slingshot/bubble-s2.png"))?,
                decode(include_bytes!("../../assets/desktop-pet/slingshot/bubble-s3.png"))?,
            ],
        })
    }).as_ref()
}

#[cfg(windows)]
pub(super) fn available() -> bool { assets().is_some() }

#[cfg(windows)]
fn scene_layout(scene:&Scene) -> Option<Layout> {
    if !scene.unit.is_finite() || scene.unit<=0.0 || !scene.scale.is_finite() || scene.scale<=0.0 { return None; }
    let margin=(20.0*scene.unit).max(12.0*scene.scale);
    let mut left=(scene.base.0-46.0*scene.unit).min(scene.pouch.0)-margin;
    let mut top=(scene.base.1-128.0*scene.unit).min(scene.pouch.1)-margin;
    let mut right=(scene.base.0+46.0*scene.unit).max(scene.pouch.0)+margin;
    let mut bottom=scene.base.1.max(scene.pouch.1)+margin;
    for &(x,y) in &scene.dots { left=left.min(x-margin); top=top.min(y-margin); right=right.max(x+margin); bottom=bottom.max(y+margin); }
    if ![left,top,right,bottom].iter().all(|v|v.is_finite() && *v>=i32::MIN as f64 && *v<=i32::MAX as f64) { return None; }
    let side=(((right-left).max(bottom-top)+2.0)/64.0).ceil().max(1.0)*64.0;
    if side>i32::MAX as f64 { return None; }
    Some(Layout {left:left.floor() as i32,top:top.floor() as i32,side:side as i32})
}

// Oversized scenes are drawn into disjoint screen-aligned tiles so no individual
// layered bitmap has to cover the whole extended rubber band.
#[cfg(windows)]
const TILE_SIDE:i32=1024;
#[cfg(windows)]
const MAX_TILES:usize=64;

#[cfg(windows)]
fn add_visible_tiles(tiles:&mut std::collections::BTreeSet<(i32,i32)>, screens:&[super::placement::Rect], bounds:(f64,f64,f64,f64)) -> Option<()> {
    if ![bounds.0,bounds.1,bounds.2,bounds.3].iter().all(|v|v.is_finite()) { return None; }
    for screen in screens {
        let left=bounds.0.max(screen.left as f64).floor() as i64;
        let top=bounds.1.max(screen.top as f64).floor() as i64;
        let right=bounds.2.min(screen.right as f64).ceil() as i64;
        let bottom=bounds.3.min(screen.bottom as f64).ceil() as i64;
        if left>=right || top>=bottom { continue; }
        let side=i64::from(TILE_SIDE);
        let (x0,x1)=(left.div_euclid(side),(right-1).div_euclid(side));
        let (y0,y1)=(top.div_euclid(side),(bottom-1).div_euclid(side));
        if (x1-x0+1).saturating_mul(y1-y0+1)>MAX_TILES as i64 { return None; }
        for y in y0..=y1 { for x in x0..=x1 {
            let left=i32::try_from(x*side).ok()?;
            let top=i32::try_from(y*side).ok()?;
            tiles.insert((left,top));
            if tiles.len()>MAX_TILES { return None; }
        } }
    }
    Some(())
}

#[cfg(windows)]
fn tiled_layouts(scene:&Scene, front:bool) -> Option<Vec<Layout>> {
    let mut tiles=std::collections::BTreeSet::new();
    let u=scene.unit;
    if front {
        let (x,y)=scene.pouch;
        add_visible_tiles(&mut tiles,&scene.screens,(x-13.0*u-1.0,y-6.0*u-1.0,x+13.0*u+1.0,y+6.0*u+1.0))?;
    } else {
        let (x,y)=scene.base;
        add_visible_tiles(&mut tiles,&scene.screens,(x-46.0*u-1.0,y-128.0*u-1.0,x+46.0*u+1.0,y+1.0))?;
        for (i,&(x,y)) in scene.dots.iter().enumerate() {
            let radius=((12.0-i as f64*0.65)*scene.scale).max(4.0*scene.scale)*0.5+1.0;
            add_visible_tiles(&mut tiles,&scene.screens,(x-radius,y-radius,x+radius,y+radius))?;
        }
    }
    let tip=if front {(scene.base.0+28.15*u,scene.base.1-105.14*u)} else {(scene.base.0-27.53*u,scene.base.1-106.43*u)};
    let delta=(scene.pouch.0-tip.0,scene.pouch.1-tip.1);
    let length=delta.0.hypot(delta.1);
    if !length.is_finite() { return None; }
    let segments=(length/(TILE_SIDE as f64*0.5)).ceil().max(1.0);
    if segments>65536.0 { return None; }
    let segments=segments as usize;
    let padding=u+(4.0*u).max(1.0)*0.5+2.0;
    for i in 0..segments {
        let a=i as f64/segments as f64;
        let b=(i+1) as f64/segments as f64;
        let p=(tip.0+delta.0*a,tip.1+delta.1*a);
        let q=(tip.0+delta.0*b,tip.1+delta.1*b);
        add_visible_tiles(&mut tiles,&scene.screens,(p.0.min(q.0)-padding,p.1.min(q.1)-padding,p.0.max(q.0)+padding,p.1.max(q.1)+padding))?;
    }
    Some(tiles.into_iter().map(|(left,top)|Layout {left,top,side:TILE_SIDE}).collect())
}

/// Alpha-composite a transformed image into the premultiplied BGRA DIB.
#[cfg(windows)]
fn sprite(pixels:&mut [u8], layout:&Layout, image:&image::RgbaImage, center:(f64,f64), size:(f64,f64), angle:f64, opacity:f64) {
    if size.0<=0.0 || size.1<=0.0 { return; }
    let (sin,cos)=angle.sin_cos();
    let extent=((size.0*cos.abs()+size.1*sin.abs())*0.5+1.0,(size.0*sin.abs()+size.1*cos.abs())*0.5+1.0);
    let center=(center.0-layout.left as f64,center.1-layout.top as f64);
    let minx=((center.0-extent.0).floor() as i32).clamp(0,layout.side);
    let maxx=((center.0+extent.0).ceil() as i32).clamp(0,layout.side);
    let miny=((center.1-extent.1).floor() as i32).clamp(0,layout.side);
    let maxy=((center.1+extent.1).ceil() as i32).clamp(0,layout.side);
    let (w,h)=image.dimensions();
    for y in miny..maxy { for x in minx..maxx {
        let dx=x as f64+0.5-center.0; let dy=y as f64+0.5-center.1;
        let u=(dx*cos+dy*sin)/size.0+0.5;
        let v=(-dx*sin+dy*cos)/size.1+0.5;
        if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) { continue; }
        // Interpolate premultiplied samples, so transparent artwork has no dark fringe.
        let sx=u*w as f64-0.5; let sy=v*h as f64-0.5;
        let x0=sx.floor() as i32; let y0=sy.floor() as i32;
        let fx=sx-x0 as f64; let fy=sy-y0 as f64;
        let mut sample=[0.0;4];
        for (ix,wx) in [(x0,1.0-fx),(x0+1,fx)] { for (iy,wy) in [(y0,1.0-fy),(y0+1,fy)] {
            if ix<0 || iy<0 || ix>=w as i32 || iy>=h as i32 { continue; }
            let p=image.get_pixel(ix as u32,iy as u32).0;
            let alpha=p[3] as f64/255.0*wx*wy*opacity;
            sample[0]+=p[2] as f64*alpha; sample[1]+=p[1] as f64*alpha; sample[2]+=p[0] as f64*alpha; sample[3]+=255.0*alpha;
        } }
        let at=(y as usize*layout.side as usize+x as usize)*4;
        let remaining=1.0-sample[3]/255.0;
        for channel in 0..4 { pixels[at+channel]=(sample[channel]+pixels[at+channel] as f64*remaining).round().clamp(0.0,255.0) as u8; }
    } }
}

#[cfg(windows)]
fn paint_scene(pixels:&mut [u8], layout:&Layout, scene:&Scene, front:bool) -> bool {
    let Some(art)=assets() else { return false; };
    pixels.fill(0);
    let u=scene.unit;
    let left=(scene.base.0-27.53*u,scene.base.1-106.43*u);
    let right=(scene.base.0+28.15*u,scene.base.1-105.14*u);
    let band=|pixels: &mut [u8], tip:(f64,f64)| {
        let delta=(scene.pouch.0-tip.0,scene.pouch.1-tip.1);
        sprite(pixels,layout,&art.band,((tip.0+scene.pouch.0)*0.5,(tip.1+scene.pouch.1)*0.5),(delta.0.hypot(delta.1)+2.0*u,(4.0*u).max(1.0)),delta.1.atan2(delta.0),1.0);
    };
    if front {
        band(pixels,right);
        sprite(pixels,layout,&art.pouch,scene.pouch,(26.0*u,12.0*u),0.0,1.0);
    } else {
        for (i,&point) in scene.dots.iter().enumerate() {
            let diameter=((12.0-i as f64*0.65)*scene.scale).max(4.0*scene.scale);
            sprite(pixels,layout,&art.bubbles[i%art.bubbles.len()],point,(diameter,diameter),0.0,(1.0-i as f64*0.075).max(0.4));
        }
        sprite(pixels,layout,&art.sling,(scene.base.0,scene.base.1-64.0*u),(92.0*u,128.0*u),0.0,1.0);
        band(pixels,left);
    }
    true
}

/// The hit burst uses the same artwork and premultiplied composition as the guide.
#[cfg(windows)]
fn paint_hit(pixels:&mut [u8], layout:&Layout, point:(i32,i32), scale:f64, progress:f64) -> bool {
    let Some(art)=assets() else { return false; };
    pixels.fill(0);
    let t=progress.clamp(0.0,1.0);
    let center=(point.0 as f64,point.1 as f64);
    // A quick squash, release, and disappearance makes the impact feel soft.
    let squash=(1.0-t/0.22).clamp(0.0,1.0);
    let fade=(1.0-((t-0.35)/0.65).clamp(0.0,1.0)).powi(2);
    let body=28.0*scale*(1.0+0.15*(t/0.2).min(1.0));
    sprite(pixels,layout,&art.bubbles[0],center,(body*(1.0+0.25*squash),body*(1.0-0.22*squash)),0.0,fade);
    let travel=1.0-(1.0-t).powi(2);
    let particles=[(-0.92,-0.40,13.0,1usize),(0.92,-0.50,14.0,2),(-0.45,0.72,11.0,2),(0.52,0.70,12.0,1),(-0.08,-1.0,11.0,0)];
    for (dx,dy,size,index) in particles {
        let p=(center.0+dx*34.0*scale*travel,center.1+(dy*29.0*travel-10.0*t*t)*scale);
        let diameter=size*scale*(0.65+0.35*(t/0.18).min(1.0));
        sprite(pixels,layout,&art.bubbles[index],p,(diameter,diameter),0.0,fade*0.95);
    }
    true
}

#[derive(Clone,Copy)]
struct Layout {
    left: i32,
    top: i32,
    side: i32,
}

#[cfg(test)]
fn layout(dots: &[(f64, f64)], scale: f32) -> Option<Layout> {
    if dots.is_empty()
        || !scale.is_finite()
        || !(0.1..=8.0).contains(&scale)
        || dots.iter().any(|(x, y)| !x.is_finite() || !y.is_finite())
    {
        return None;
    }
    let margin = 10.0 * scale as f64;
    let left = (dots.iter().map(|p| p.0).fold(f64::INFINITY, f64::min) - margin).floor();
    let top = (dots.iter().map(|p| p.1).fold(f64::INFINITY, f64::min) - margin).floor();
    if left < i32::MIN as f64
        || left > i32::MAX as f64
        || top < i32::MIN as f64
        || top > i32::MAX as f64
    {
        return None;
    }
    // The preview contains eight points at 14 logical pixels of path length.
    Some(Layout {
        left: left as i32,
        top: top as i32,
        side: (132.0 * scale).ceil() as i32,
    })
}

/// Draw premultiplied BGRA, as required by UpdateLayeredWindow's AC_SRC_ALPHA.
#[cfg(test)]
fn paint(pixels: &mut [u8], layout: &Layout, dots: &[(f64, f64)], scale: f32) {
    pixels.fill(0);
    let side = layout.side as usize;
    for (index, &(x, y)) in dots.iter().enumerate() {
        let cx = x - layout.left as f64;
        let cy = y - layout.top as f64;
        let radius = (3.5 - index as f64 * 0.15).max(1.0) * scale as f64;
        let inner = (radius - scale as f64 * 0.5).max(0.0);
        let outer = radius + scale as f64 * 0.5;
        let opacity = (230usize.saturating_sub(index * 16)) as f64 / 255.0;
        let min_x = ((cx - outer - 1.0).floor() as i32).clamp(0, layout.side);
        let max_x = ((cx + outer + 1.0).ceil() as i32).clamp(0, layout.side);
        let min_y = ((cy - outer - 1.0).floor() as i32).clamp(0, layout.side);
        let max_y = ((cy + outer + 1.0).ceil() as i32).clamp(0, layout.side);
        for py in min_y..max_y {
            for px in min_x..max_x {
                let distance = (px as f64 + 0.5 - cx).hypot(py as f64 + 0.5 - cy);
                let coverage = (outer + 0.5 - distance).clamp(0.0, 1.0);
                let fill = (inner + 0.5 - distance).clamp(0.0, coverage);
                let alpha = opacity * coverage;
                let ink = opacity * (255.0 * fill + 24.0 * (coverage - fill));
                let at = (py as usize * side + px as usize) * 4;
                for channel in &mut pixels[at..at + 3] {
                    *channel = (ink + *channel as f64 * (1.0 - alpha)).round().min(255.0) as u8;
                }
                pixels[at + 3] = (alpha * 255.0 + pixels[at + 3] as f64 * (1.0 - alpha))
                    .round()
                    .min(255.0) as u8;
            }
        }
    }
}

#[cfg(windows)]
mod native {
    use super::{paint_hit, paint_scene, scene_layout, tiled_layouts, Layout, Scene};
    use std::{ffi::c_void, ptr, sync::OnceLock};
    use windows::core::w;
    use windows::Win32::Foundation::{
        COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA,
        AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP,
        HDC, HGDIOBJ, HMONITOR, MonitorFromPoint, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    struct Surface {
        hwnd: HWND,
        dc: HDC,
        bitmap: HBITMAP,
        original: HGDIOBJ,
        bits: *mut u8,
        side: i32,
        visible: bool,
        hit_monitor: Option<HMONITOR>,
        hit_scale: f64,
    }

    pub(crate) struct Window {
        surfaces: Vec<Surface>,
        last_scene: Option<(Scene,bool)>,
    }

    impl Surface {
        fn new() -> Option<Self> {
            unsafe {
                let module = GetModuleHandleW(None).ok()?;
                let instance = HINSTANCE(module.0);
                static REGISTERED: OnceLock<bool> = OnceLock::new();
                if !*REGISTERED.get_or_init(|| {
                    RegisterClassW(&WNDCLASSW {
                        lpfnWndProc: Some(window_proc),
                        hInstance: instance,
                        lpszClassName: w!("GuaSlingshotGuideWindow"),
                        ..Default::default()
                    }) != 0
                }) {
                    return None;
                }
                let hwnd = CreateWindowExW(
                    WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE
                        | WS_EX_TOPMOST,
                    w!("GuaSlingshotGuideWindow"),
                    w!("呱呱桌宠 · 弹弓指引"),
                    WS_POPUP,
                    0,
                    0,
                    1,
                    1,
                    None,
                    None,
                    instance,
                    None,
                )
                .ok()?;
                let dc = CreateCompatibleDC(None);
                if dc.is_invalid() {
                    let _ = DestroyWindow(hwnd);
                    return None;
                }
                Some(Self {
                    hwnd,
                    dc,
                    bitmap: HBITMAP::default(),
                    original: HGDIOBJ::default(),
                    bits: ptr::null_mut(),
                    side: 0,
                    visible: false,
                    hit_monitor: None,
                    hit_scale: 1.0,
                })
            }
        }

        unsafe fn resize(&mut self, side: i32, exact: bool) -> bool {
            if self.side == side || (!exact && self.side > side) {
                return true;
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: side,
                    biHeight: -side,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                return false;
            };
            if bits.is_null() {
                let _ = DeleteObject(bitmap);
                return false;
            }
            let old = SelectObject(self.dc, bitmap);
            if old.is_invalid() {
                let _ = DeleteObject(bitmap);
                return false;
            }
            if self.bitmap.is_invalid() {
                self.original = old;
            } else {
                let _ = DeleteObject(self.bitmap);
            }
            self.bitmap = bitmap;
            self.bits = bits.cast();
            self.side = side;
            true
        }

        fn show(&mut self, scene: &Scene, front: bool, pet: HWND, mut layout: Layout, tiled: bool) -> bool {
            unsafe {
                if !self.resize(layout.side,tiled) {
                    return false;
                }
                layout.side=self.side;
                let pixels = std::slice::from_raw_parts_mut(
                    self.bits,
                    layout.side as usize * layout.side as usize * 4,
                );
                if !paint_scene(pixels, &layout, scene, front) { return false; }
                self.present(layout,front,pet)
            }
        }

        fn show_hit(&mut self, point:(i32,i32), progress:f64, pet:HWND) -> bool {
            unsafe {
                // Reposition before reading DPI only when the impact moved to another
                // monitor; moving on every frame would visibly shift the burst.
                let monitor=MonitorFromPoint(POINT {x:point.0,y:point.1},MONITOR_DEFAULTTONEAREST);
                if self.hit_monitor!=Some(monitor) {
                    self.hide();
                    if SetWindowPos(self.hwnd,HWND_TOPMOST,point.0,point.1,1,1,SWP_NOACTIVATE|SWP_NOZORDER).is_err() { return false; }
                    let dpi=GetDpiForWindow(self.hwnd);
                    self.hit_scale=if dpi==0 {1.0} else {(dpi as f64/96.0).clamp(0.5,8.0)};
                    self.hit_monitor=Some(monitor);
                }
                let scale=self.hit_scale;
                let side=(128.0*scale).ceil() as i32;
                if !self.resize(side,true) { return false; }
                let layout=Layout {left:point.0.saturating_sub(side/2),top:point.1.saturating_sub(side/2),side};
                let pixels=std::slice::from_raw_parts_mut(self.bits,side as usize*side as usize*4);
                if !paint_hit(pixels,&layout,point,scale,progress) { return false; }
                self.present(layout,true,pet)
            }
        }

        unsafe fn present(&mut self, layout:Layout, front:bool, pet:HWND) -> bool {
                let destination = POINT {
                    x: layout.left,
                    y: layout.top,
                };
                let source = POINT { x: 0, y: 0 };
                let size = SIZE {
                    cx: layout.side,
                    cy: layout.side,
                };
                let blend = BLENDFUNCTION {
                    BlendOp: AC_SRC_OVER as u8,
                    BlendFlags: 0,
                    SourceConstantAlpha: 255,
                    AlphaFormat: AC_SRC_ALPHA as u8,
                };
                if UpdateLayeredWindow(
                    self.hwnd,
                    None,
                    Some(&destination),
                    Some(&size),
                    self.dc,
                    Some(&source),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                )
                .is_err()
                {
                    return false;
                }
                if !self.visible {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    self.visible = true;
                }
                self.order(front,pet)
        }

        fn order(&self, front:bool, pet:HWND) -> bool {
            unsafe { SetWindowPos(self.hwnd,if front { HWND_TOPMOST } else { pet },0,0,0,0,SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).is_ok() }
        }

        fn hide(&mut self) {
            if self.visible { unsafe { let _=ShowWindow(self.hwnd,SW_HIDE); } self.visible=false; }
        }
    }

    impl Drop for Surface {
        fn drop(&mut self) {
            unsafe {
                if !self.bitmap.is_invalid() {
                    let _ = SelectObject(self.dc, self.original);
                    let _ = DeleteObject(self.bitmap);
                }
                let _ = DeleteDC(self.dc);
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }

    impl Window {
        pub(crate) fn new() -> Option<Self> {
            Some(Self {surfaces:vec![Surface::new()?],last_scene:None})
        }

        pub(crate) fn show_hit(&mut self, point:(i32,i32), progress:f64) -> bool {
            let Some(pet)=super::super::native::hwnd() else { return false; };
            self.last_scene=None;
            self.surfaces[0].show_hit(point,progress,pet)
        }

        pub(crate) fn show(&mut self, scene:&Scene, front:bool) -> bool {
            let Some(pet)=super::super::native::hwnd() else { return false; };
            if self.last_scene.as_ref().is_some_and(|(previous,was_front)|previous==scene && *was_front==front) {
                return self.surfaces.iter().filter(|surface|surface.visible).all(|surface|surface.order(front,pet));
            }
            let Some(layout)=scene_layout(scene) else { return false; };
            let tiled=layout.side>4096;
            let layouts=if tiled {
                let Some(layouts)=tiled_layouts(scene,front) else { return false; };
                layouts
            } else { vec![layout] };
            while self.surfaces.len()<layouts.len() {
                let Some(surface)=Surface::new() else { return false; };
                self.surfaces.push(surface);
            }
            for (surface,layout) in self.surfaces.iter_mut().zip(layouts.iter().copied()) {
                if !surface.show(scene,front,pet,layout,tiled) { return false; }
            }
            for surface in self.surfaces.iter_mut().skip(layouts.len()) { surface.hide(); }
            // Keep a few idle surfaces for subsequent drags, but release a large
            // burst of bitmaps as soon as the scene becomes small again.
            self.surfaces.truncate(layouts.len().saturating_add(4).max(1));
            self.last_scene=Some((scene.clone(),front));
            true
        }
    }
}

#[cfg(windows)]
pub(super) use native::Window;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn hit_bubbles_fade_without_clipping_or_alpha_fringes() {
        for scale in [1.0,1.5,2.0,3.0] {
            let side=(128.0_f64*scale).ceil() as i32;
            let point=(-800,500);
            let layout=Layout {left:point.0-side/2,top:point.1-side/2,side};
            let mut totals=Vec::new();
            for progress in [0.0,0.3,0.75,1.0] {
                let mut pixels=vec![0;side as usize*side as usize*4];
                assert!(paint_hit(&mut pixels,&layout,point,scale,progress));
                let alphas:Vec<_>=pixels.chunks_exact(4).map(|p|p[3]).collect();
                assert!(pixels.chunks_exact(4).all(|p|p[0]<=p[3] && p[1]<=p[3] && p[2]<=p[3]));
                assert!((0..side as usize).all(|n|alphas[n]==0 && alphas[(side as usize-1)*side as usize+n]==0 && alphas[n*side as usize]==0 && alphas[n*side as usize+side as usize-1]==0));
                totals.push(alphas.into_iter().map(u64::from).sum::<u64>());
            }
            assert!(totals[0]>0 && totals[1]>totals[2] && totals[2]>0 && totals[3]==0);
        }
    }

    #[test]
    fn guide_pixels_are_antialiased_premultiplied_and_bounded() {
        let dots = [(100.0, 100.0), (114.0, 100.0), (128.0, 102.0)];
        let bounds = layout(&dots, 1.0).unwrap();
        let mut pixels = vec![0; bounds.side as usize * bounds.side as usize * 4];
        paint(&mut pixels, &bounds, &dots, 1.0);
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert!(pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 100));
        assert!(pixels
            .chunks_exact(4)
            .all(|p| p[0] <= p[3] && p[1] <= p[3] && p[2] <= p[3]));
        assert!(layout(&dots, 0.0).is_none());
    }
}
