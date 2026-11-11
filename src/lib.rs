//! 呱：Windows 顶部「灵动岛」。
//!
//! - `app` / `pages`：与平台无关的界面与动效（外壳与录制器共用）
//! - `anim` / `paint` / `shape` / `theme`：弹簧、矢量绘制、面板轮廓、配色字体
//! - `model` / `lyrics` / `smtc` / `icons`：数据、歌词、系统媒体、文件图标
//! - `native` / `tray`：仅 Windows 的窗口区域、命中穿透与托盘

pub mod anim;
pub mod app;
#[cfg(windows)]
pub mod backdrop;
mod bar_layout;
pub mod brand;
pub mod bt;
pub mod calendar;
mod calendar_editor;
mod calendar_ui;
#[cfg(windows)]
pub mod composition;
mod egui_input;
pub mod desktop_pet;
mod extras;
mod fluid;
mod focus_bar;
pub mod glass;
#[cfg(any(test, feature = "record"))]
pub mod headless;
pub mod icons;
pub mod input;
mod local_calendar;
pub mod lyrics;
pub mod mail;
mod mail_reader;
pub mod model;
pub mod otp;
mod pages;
pub mod paint;
mod pet;
mod pet_expressions;
mod pet_travel;
pub mod poll_clock;
pub mod quota;
mod quota_ui;
mod resident_settings;
pub mod settings_theme;
pub mod shape;
pub mod skin_paint;
pub mod smtc;
#[cfg(windows)]
pub mod system;
mod task_picker;
pub mod theme;
mod updates;
mod weather;

#[cfg(windows)]
pub mod native;
#[cfg(windows)]
pub mod tray;

#[cfg(test)]
#[path = "bin/gua-record/raster.rs"]
mod preview_raster;
#[cfg(test)]
mod ui_tests;
