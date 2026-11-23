//! Read-only snapshot of items displayed by the desktop Shell view.
use super::placement::{Rect, Screen};
use windows::{core::{Interface, VARIANT, w}, Win32::{
    Foundation::POINT,
    Graphics::Gdi::ClientToScreen,
    System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IServiceProvider, CLSCTX_ALL, COINIT_APARTMENTTHREADED},
    UI::{Shell::{IFolderView2, IShellBrowser, IShellWindows, ShellWindows, SID_STopLevelBrowser, SVGIO_ALLVIEW, SWC_DESKTOP, SWFO_NEEDDISPATCH},
        WindowsAndMessaging::{FindWindowExW, IsWindowVisible}},
}};

pub(super) fn snapshot(screen: Screen) -> Vec<Rect> {
    unsafe {
        let initialized = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let result = read(screen).unwrap_or_default();
        if initialized { CoUninitialize(); }
        result
    }
}

unsafe fn read(screen: Screen) -> windows::core::Result<Vec<Rect>> {
    let shell: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
    let location = VARIANT::from(0i32);
    let empty = VARIANT::default();
    let mut desktop_hwnd=0;
    let dispatch=shell.FindWindowSW(&location,&empty,SWC_DESKTOP,&mut desktop_hwnd,SWFO_NEEDDISPATCH)?;
    let service: IServiceProvider=dispatch.cast()?;
    let browser: IShellBrowser=service.QueryService(&SID_STopLevelBrowser)?;
    let view=browser.QueryActiveShellView()?;
    let folder: IFolderView2=view.cast()?;
    let view_hwnd=view.GetWindow()?;
    // This child is used only to detect hidden icons and translate view coordinates;
    // item identities and positions come from the supported Shell interfaces.
    let list=FindWindowExW(view_hwnd,None,w!("SysListView32"),None)?;
    if !IsWindowVisible(list).as_bool() { return Ok(Vec::new()); }
    let mut origin=POINT::default();
    if !ClientToScreen(list,&mut origin).as_bool() { return Ok(Vec::new()); }
    let mut spacing=POINT::default();
    folder.GetSpacing(&mut spacing)?;
    let mut mode=Default::default();
    let mut icon_size=0;
    folder.GetViewModeAndIconSize(&mut mode,&mut icon_size)?;
    if icon_size<=0 || spacing.x<=0 || spacing.y<=0 { return Ok(Vec::new()); }
    let mut result=Vec::new();
    for index in 0..folder.ItemCount(SVGIO_ALLVIEW)? {
        let Ok(pidl)=folder.Item(index) else { continue; };
        let pos=folder.GetItemPosition(pidl);
        CoTaskMemFree(Some(pidl.cast()));
        let Ok(pos)=pos else { continue; };
        let size=icon_size.min(spacing.x).min(spacing.y);
        let left=origin.x.saturating_add(pos.x).saturating_add((spacing.x-size)/2);
        let top=origin.y.saturating_add(pos.y);
        let rect=Rect {left,top,right:left.saturating_add(size),bottom:top.saturating_add(size)};
        if rect.left>=screen.bounds.left && rect.top>=screen.bounds.top && rect.right<=screen.bounds.right && rect.bottom<=screen.bounds.bottom { result.push(rect); }
    }
    Ok(result)
}

