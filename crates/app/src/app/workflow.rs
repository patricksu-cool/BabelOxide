use super::*;
use babeloxide_config::Settings;

/// Per-file state is derived from the task manager's real completion/progress.
/// A batch stops on failure; its untouched tail is cancelled, never "complete".
fn file_status(snapshot: &TaskSnapshot, index: usize) -> TaskStatus {
    if index < snapshot.results.len() {
        return TaskStatus::Succeeded;
    }
    if snapshot.status.is_terminal() {
        return if snapshot.status == TaskStatus::Failed && index == snapshot.completed_files {
            TaskStatus::Failed
        } else {
            TaskStatus::Cancelled
        };
    }
    if snapshot.current_file == Some(index) {
        snapshot.status
    } else {
        TaskStatus::Queued
    }
}

impl NativeApp {
    pub(super) fn workflow_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.small(self.text("Add PDFs, choose a preset, then translate"));
        ui.add_space(8.0);
        self.files(ui, frame);
        self.url_input(ui);
        ui.separator();
        self.preset_controls(ui);
        ui.separator();
        ui.strong(self.text("Translation service"));
        self.engine(ui, frame, false);
        if ui
            .link(self.text("Configure service and API key…"))
            .clicked()
        {
            self.show_settings = true;
            self.settings_section = 1;
        }
        ui.separator();
        ui.columns(2, |columns| {
            self.language(&mut columns[0], "translation.lang_in", true);
            self.language(&mut columns[1], "translation.lang_out", false);
        });
        if ui.button(self.text("Swap languages")).clicked()
            && let Err(error) = self.editor.swap_languages()
        {
            self.fail(error);
        }
        ui.horizontal_wrapped(|ui| {
            self.output_choice(ui, "pdf.no_mono", self.text("Translated PDF"));
            self.output_choice(ui, "pdf.no_dual", self.text("Bilingual PDF"));
        });
        egui::CollapsingHeader::new(self.text("Output and page range")).show(ui, |ui| {
            self.field_path(
                ui,
                frame,
                "translation.output",
                Some(self.text("Output folder")),
            );
            self.field_path(ui, frame, "pdf.pages", Some(self.text("Custom pages")));
            ui.small(self.text("Page ranges help"));
        });
        self.queue_list(ui);
    }

    pub(super) fn start_action(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        let can_start = self.can_submit();
        if ui
            .add_enabled(
                can_start,
                egui::Button::new(RichText::new(self.text("Translate")).color(Color32::WHITE))
                    .fill(Color32::from_rgb(23, 91, 115))
                    .min_size(Vec2::new(ui.available_width(), 42.0)),
            )
            .on_hover_text("Ctrl / ⌘ + Enter")
            .clicked()
        {
            self.submit();
        }
        if !can_start {
            ui.small(self.text(if self.editor.files.is_empty() {
                "Choose at least one PDF"
            } else if self.submitting {
                "Preparing translation…"
            } else {
                "Save or finish the current operation first"
            }));
        }
    }

    pub(super) fn can_submit(&self) -> bool {
        !self.submitting
            && !self.saving
            && self.download_cancel.is_none()
            && !self.editor.files.is_empty()
            && self
                .editor
                .engine_enabled(self.editor.settings.main_engine())
    }

    fn preset_controls(&mut self, ui: &mut egui::Ui) {
        ui.strong(self.text("Translation presets"));
        let mut presets = crate::presets::builtins();
        let saved = match crate::presets::saved(&self.editor.settings) {
            Ok(saved) => saved,
            Err(error) => {
                ui.label(error.to_string());
                return;
            }
        };
        presets.extend(saved.clone());
        let mut selected = self.selected_preset.clone();
        egui::ComboBox::from_id_salt("workflow-presets")
            .width(ui.available_width())
            .selected_text(&selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut selected, "自訂".into(), self.text("Custom"));
                for preset in &presets {
                    ui.selectable_value(&mut selected, preset.name.clone(), &preset.name);
                }
            });
        if selected != self.selected_preset {
            if let Some(preset) = presets.iter().find(|preset| preset.name == selected) {
                let changed = self
                    .editor
                    .snapshot(false)
                    .and_then(|settings| crate::presets::apply(&settings, preset));
                match changed {
                    Ok(settings) => {
                        self.editor.settings = settings;
                        self.editor.drafts.clear();
                        self.editor.mark_dirty();
                        self.selected_preset = selected.clone();
                        self.preset_name = if saved.iter().any(|preset| preset.name == selected) {
                            selected
                        } else {
                            String::new()
                        };
                    }
                    Err(error) => self.fail(error),
                }
            } else {
                self.selected_preset = selected;
            }
        }
        egui::CollapsingHeader::new(self.text("Save or rename preset")).show(ui, |ui| {
            ui.label(self.text("Preset name"));
            ui.add(egui::TextEdit::singleline(&mut self.preset_name).desired_width(f32::INFINITY));
            ui.small(self.text("Presets keep API keys and request limits unchanged"));
            ui.horizontal_wrapped(|ui| {
                if ui.button(self.text("Save preset")).clicked() {
                    let result = self.editor.snapshot(false).and_then(|settings| {
                        crate::presets::capture(&settings, self.preset_name.trim())
                    });
                    match result {
                        Ok(preset) => {
                            let mut updated = saved.clone();
                            updated.retain(|item| !item.name.eq_ignore_ascii_case(&preset.name));
                            self.selected_preset = preset.name.clone();
                            updated.push(preset);
                            if let Err(error) =
                                crate::presets::store(&mut self.editor.settings, &updated)
                            {
                                self.fail(error);
                            } else {
                                self.editor.mark_dirty();
                                self.save();
                            }
                        }
                        Err(error) => self.fail(error),
                    }
                }
                let custom = saved
                    .iter()
                    .position(|preset| preset.name == self.selected_preset);
                if ui
                    .add_enabled(custom.is_some(), egui::Button::new(self.text("Rename")))
                    .clicked()
                    && let Some(index) = custom
                {
                    let mut updated = saved.clone();
                    updated[index].name = self.preset_name.trim().into();
                    match crate::presets::store(&mut self.editor.settings, &updated) {
                        Ok(()) => {
                            self.selected_preset = self.preset_name.trim().into();
                            self.editor.mark_dirty();
                            self.save();
                        }
                        Err(error) => self.fail(error),
                    }
                }
            });
        });
    }

    fn queue_list(&mut self, ui: &mut egui::Ui) {
        if self.batches.is_empty() {
            return;
        }
        ui.separator();
        ui.strong(self.text("Task queue"));
        ui.small(self.text("PDFs run one at a time; request concurrency is unchanged"));
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("queue-overview")
            .max_height(160.0)
            .show(ui, |ui| {
                for (batch_index, batch) in self.batches.iter().enumerate() {
                    for (index, input) in batch.inputs.iter().enumerate() {
                        let status = file_status(&batch.snapshot, index);
                        let name = input.file_name().unwrap_or_default().to_string_lossy();
                        let label = format!("{} · {}", self.status_text(status), name);
                        if ui
                            .selectable_label(
                                self.selected_batch == Some(batch_index)
                                    && self.selected_file == index,
                                label,
                            )
                            .on_hover_text(input.display().to_string())
                            .clicked()
                        {
                            selected = Some((batch_index, index));
                        }
                    }
                }
            });
        if let Some((batch, file)) = selected {
            self.selected_batch = Some(batch);
            self.selected_file = file;
            self.compact_preview = true;
            self.kind = if file < self.batches[batch].snapshot.results.len() {
                DocumentKind::Mono
            } else {
                DocumentKind::Original
            };
        }
    }

    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.heading(self.text("Settings"));
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["Basic", "Provider", "PDF", "Performance", "Advanced"]
                .into_iter()
                .enumerate()
            {
                ui.selectable_value(
                    &mut self.settings_section,
                    index,
                    crate::strings::ui(label, self.chinese),
                );
            }
        });
        ui.separator();
        match self.settings_section {
            0 => {
                self.language(ui, "translation.lang_in", true);
                self.language(ui, "translation.lang_out", false);
                for path in [
                    "translation.output",
                    "translation.glossaries",
                    "translation.no_auto_extract_glossary",
                    "translation.save_auto_extracted_glossary",
                ] {
                    self.field_path(ui, frame, path, None);
                }
            }
            1 => {
                self.engine(ui, frame, false);
                egui::CollapsingHeader::new(self.text("Terminology service"))
                    .show(ui, |ui| self.engine(ui, frame, true));
                egui::CollapsingHeader::new(self.text("Manage services")).show(ui, |ui| {
                    for engine in engine_specs() {
                        let mut enabled = self.editor.engine_enabled(engine);
                        if ui.checkbox(&mut enabled, &engine.name).changed()
                            && let Err(error) = self.editor.toggle_service(engine, enabled)
                        {
                            self.fail(error);
                        }
                    }
                });
            }
            2 => {
                for spec in field_specs()
                    .iter()
                    .filter(|spec| spec.path.starts_with("pdf."))
                {
                    self.field(ui, frame, spec, None);
                }
            }
            3 => {
                for path in [
                    "translation.qps",
                    "translation.pool_max_workers",
                    "translation.term_qps",
                    "translation.term_pool_max_workers",
                    "translation.ignore_cache",
                ] {
                    self.field_path(ui, frame, path, None);
                }
            }
            _ => self.settings_panel(ui, frame),
        }
    }

    pub(super) fn retry_unfinished(&mut self, index: usize) {
        let Some(batch) = self.batches.get(index) else {
            return;
        };
        if !batch.snapshot.status.is_terminal() || self.submitting {
            return;
        }
        let files: Vec<_> = batch
            .inputs
            .iter()
            .enumerate()
            .filter(|(index, _)| *index >= batch.snapshot.results.len())
            .map(|(_, path)| path.to_string_lossy().into_owned())
            .collect();
        if files.is_empty() {
            return;
        }
        let mut settings: Settings = batch.settings.clone();
        if let Err(error) = settings.set("basic.input_files", json!(files)) {
            self.fail(error);
            return;
        }
        self.pending_settings = Some(settings.clone());
        if self.command(Command::Translate(settings)) {
            self.submitting = true;
            self.error = None;
        } else {
            self.pending_settings = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_failed_batch_never_marks_untouched_tail_successful() {
        let mut snapshot = TaskSnapshot {
            id: 1,
            status: TaskStatus::Running,
            total_files: 3,
            completed_files: 1,
            current_file: Some(1),
            progress: None,
            results: vec![json!({})],
            failure: None,
        };
        assert_eq!(file_status(&snapshot, 0), TaskStatus::Succeeded);
        assert_eq!(file_status(&snapshot, 1), TaskStatus::Running);
        assert_eq!(file_status(&snapshot, 2), TaskStatus::Queued);
        snapshot.status = TaskStatus::Failed;
        snapshot.current_file = None;
        assert_eq!(file_status(&snapshot, 1), TaskStatus::Failed);
        assert_eq!(file_status(&snapshot, 2), TaskStatus::Cancelled);
    }
}
