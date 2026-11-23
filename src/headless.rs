//! Utilities for driving egui in headless recording and UI tests.
use eframe::egui::{self, Context, Ui};
use std::path::{Path, PathBuf};

pub trait PanelContextExt {
    fn show_ctx<R>(
        self,
        ctx: &Context,
        contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R>;
}
impl PanelContextExt for egui::CentralPanel {
    fn show_ctx<R>(
        self,
        ctx: &Context,
        contents: impl FnOnce(&mut Ui) -> R,
    ) -> egui::InnerResponse<R> {
        ctx.root_ui(|ui| self.show(ui, contents))
    }
}

#[derive(Debug)]
pub struct DroppedPath(pub PathBuf);
impl egui::DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Run headless interaction tests that do not render textures.
pub trait ContextDiscardExt {
    fn run_pass_discard(&self, input: egui::RawInput, ui: impl FnMut(&egui::Context));
    fn run_ui_discard(&self, input: egui::RawInput, ui: impl FnMut(&mut egui::Ui));
}
impl ContextDiscardExt for Context {
    fn run_pass_discard(&self, input: egui::RawInput, ui: impl FnMut(&egui::Context)) {
        self.run_pass(input, ui).drop_without_applying_deltas();
    }
    fn run_ui_discard(&self, input: egui::RawInput, ui: impl FnMut(&mut egui::Ui)) {
        self.run_ui(input, ui).drop_without_applying_deltas();
    }
}
