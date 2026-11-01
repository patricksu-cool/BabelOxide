use super::*;

impl NativeApp {
    pub(super) fn about_window(&mut self, context: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = self.show_about;
        egui::Window::new(self.text("About BabelOxide"))
            .open(&mut open).default_size([580.0, 460.0]).resizable(true)
            .show(context, |ui| {
                ui.heading(format!("{} v{}", babeloxide_core::PRODUCT_NAME, babeloxide_core::PRODUCT_VERSION));
                ui.label("BabelDOC 0.6.4 · Rust egui / PDFium");
                ui.label(self.text("Free software under AGPL-3.0-only; no warranty"));
                ui.label(self.text("Modified derivative; upstream authors and sources are retained"));
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    for (index, name) in ["Licenses and components", "GNU AGPL", "Source and modifications"].into_iter().enumerate() {
                        ui.selectable_value(&mut self.about_section, index, crate::strings::ui(name,self.chinese));
                    }
                });
                ui.separator();
                egui::ScrollArea::vertical().id_salt("legal-notices").max_height(360.0).show(ui,|ui| {
                    match self.about_section {
                        1 => { ui.label(include_str!("../../../../LICENSE")); }
                        2 => { ui.label(include_str!("../../../../THIRD_PARTY_NOTICES.md")); }
                        _ => {
                            ui.label(self.text("License files are included in licenses/project and licenses"));
                            ui.label("PDFMathTranslate-next v2.9.0 / BabelDOC v0.6.4 — AGPL-3.0；上游作者 awwaawwa 與其他貢獻者。修改與品牌重整：2026-11-01。");
                            ui.hyperlink_to("PDFMathTranslate-next source", "https://github.com/PDFMathTranslate-next/PDFMathTranslate-next/tree/v2.9.0");
                            ui.hyperlink_to("BabelDOC source", "https://github.com/funstory-ai/BabelDOC/tree/v0.6.4");
                            ui.label("Rust／Python／PDFium／ONNX／模型與字型：查看 THIRD_PARTY_COMPONENTS.json、provenance/ 與 licenses/。執行包保留各自的授權與來源資料。");
                            ui.label(self.text("Corresponding source must be provided when required by the license"));
                        }
                    }
                });
            });
        self.show_about = open;
    }
}
