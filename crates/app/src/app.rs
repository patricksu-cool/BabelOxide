use std::{path::PathBuf, sync::Arc, time::Duration};

use eframe::egui::{self, Align, Color32, Layout, RichText, Vec2};
use pdf2zh_config::{EngineSpec, FieldSpec, engine_specs, field_specs};
use pdf2zh_pdf_preview::{
    PreviewErrorKind, PreviewRenderer, PreviewRequest, PreviewResult, PreviewTicket,
};
use pdf2zh_tasks::{TaskHandle, TaskSnapshot, TaskStatus};
use serde_json::{Value, json};

use crate::{
    LaunchOptions,
    background::{Background, Command, Event},
    dialogs,
    model::{DocumentKind, Editor, progress_fraction, result_path},
};

struct Batch {
    handle: TaskHandle,
    inputs: Vec<PathBuf>,
    snapshot: TaskSnapshot,
}

pub(crate) struct NativeApp {
    editor: Editor,
    background: Background,
    batches: Vec<Batch>,
    selected_batch: Option<usize>,
    selected_file: usize,
    original_file: usize,
    kind: DocumentKind,
    submitting: bool,
    save_revision: u64,
    url: String,
    download_cancel: Option<tokio_util::sync::CancellationToken>,
    exporting: bool,
    saving: bool,
    notice: Option<String>,
    error: Option<String>,
    search: String,
    chinese: bool,
    preview: Option<PreviewRenderer>,
    preview_pending: Option<PreviewRequest>,
    preview_ticket: Option<PreviewTicket>,
    preview_key: Option<String>,
    preview_generation: u64,
    preview_error: Option<String>,
    preview_unavailable: bool,
    texture: Option<egui::TextureHandle>,
    page: u32,
    page_count: u32,
    zoom: f32,
    fit_to_page: bool,
}

impl NativeApp {
    pub fn new(
        context: &eframe::CreationContext<'_>,
        options: LaunchOptions,
        background: Background,
    ) -> Self {
        let ctx = context.egui_ctx.clone();
        background.attach_repaint(ctx.clone());
        let preview = PreviewRenderer::new_with_wakeup(
            options.bundle_root,
            Arc::new(move || ctx.request_repaint()),
        );
        let (preview, preview_error) = match preview {
            Ok(renderer) => (Some(renderer), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let chinese = options
            .settings
            .gui()
            .ok()
            .and_then(|gui| gui.ui_lang)
            .is_none_or(|language| !language.starts_with("en"));
        context.egui_ctx.set_visuals(egui::Visuals::light());
        let mut style = (*context.egui_ctx.style_of(egui::Theme::Light)).clone();
        style.spacing.item_spacing = Vec2::new(9.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        context.egui_ctx.set_style_of(egui::Theme::Light, style);
        let data_notice = options.store.fallback_from().map(|_| {
            format!(
                "Application folder is read-only; settings and cache: {}",
                options.store.app_root().display()
            )
        });
        let mut app = Self::with_renderer(
            options.settings,
            background,
            chinese,
            preview,
            preview_error,
        );
        app.notice = data_notice;
        app
    }
    fn with_renderer(
        settings: pdf2zh_config::Settings,
        background: Background,
        chinese: bool,
        preview: Option<PreviewRenderer>,
        preview_error: Option<String>,
    ) -> Self {
        let preview_unavailable = preview.is_none();
        Self {
            editor: Editor::new(settings),
            background,
            batches: Vec::new(),
            selected_batch: None,
            selected_file: 0,
            original_file: 0,
            kind: DocumentKind::Original,
            submitting: false,
            save_revision: 0,
            url: String::new(),
            download_cancel: None,
            exporting: false,
            saving: false,
            notice: None,
            error: None,
            search: String::new(),
            chinese,
            preview,
            preview_pending: None,
            preview_ticket: None,
            preview_key: None,
            preview_generation: 0,
            preview_error,
            preview_unavailable,
            texture: None,
            page: 1,
            page_count: 0,
            zoom: 1.25,
            fit_to_page: true,
        }
    }
    fn text(&self, key: &'static str) -> &'static str {
        crate::strings::ui(key, self.chinese)
    }
    fn fail(&mut self, error: impl std::fmt::Display) {
        self.error = Some(crate::strings::friendly_error(
            &error.to_string(),
            self.chinese,
        ));
        self.notice = None;
    }
    fn command(&mut self, command: Command) -> bool {
        match self.background.send(command) {
            Ok(()) => true,
            Err(error) => {
                self.fail(error);
                false
            }
        }
    }
    fn edit(&mut self, path: &str, value: Value) {
        if let Err(error) = self.editor.set(path, value) {
            self.fail(error);
        }
    }
    fn begin_close(&mut self) {
        if !self.background.control.is_closing() {
            self.background.control.close(self.preview.take());
        }
    }
    fn poll(&mut self, context: &egui::Context) {
        while let Ok(event) = self.background.events.try_recv() {
            match event {
                Event::Submitted { handle, inputs } => {
                    self.submitting = false;
                    self.error = None;
                    self.kind = DocumentKind::Original;
                    self.selected_batch = Some(self.batches.len());
                    self.selected_file = 0;
                    self.batches.push(Batch {
                        snapshot: handle.snapshot(),
                        handle,
                        inputs,
                    });
                }
                Event::Saved(settings) => {
                    let unchanged = self.editor.saved_at(settings, self.save_revision);
                    self.saving = false;
                    self.notice = Some(
                        self.text(if unchanged {
                            "Settings saved"
                        } else {
                            "Saved snapshot; newer edits remain unsaved"
                        })
                        .into(),
                    );
                    self.error = None;
                }
                Event::Copied(path) => {
                    self.notice = Some(format!("{} {}", self.text("Saved to"), path.display()))
                }
                Event::Warning(message) => {
                    self.notice = Some(crate::strings::friendly_error(message, self.chinese))
                }
                Event::SaveFailed => {
                    self.saving = false;
                    self.fail("Cannot save settings; check that the application settings folder is writable");
                }
                Event::SubmissionFailed(error) => {
                    self.submitting = false;
                    self.fail(error);
                }
                Event::Failed(error) => self.fail(error),
                Event::Reloaded(settings) => {
                    self.saving = false;
                    if self.editor.revision == self.save_revision {
                        let files = self.editor.files.clone();
                        self.editor = Editor::new(settings);
                        self.editor.files = files;
                        self.notice = Some(self.text("Settings reloaded").into());
                        self.error = None;
                    }
                }
                Event::ReloadFailed => {
                    self.saving = false;
                    self.fail("Cannot reload settings");
                }
                Event::Downloaded(path) => {
                    self.download_cancel = None;
                    if let Err(error) = self.editor.add_files([path]) {
                        self.fail(error);
                    } else {
                        self.notice = Some(self.text("PDF download complete").into());
                        self.url.clear();
                    }
                }
                Event::DownloadFailed(error) => {
                    self.download_cancel = None;
                    self.fail(error);
                }
                Event::Exported(path) => {
                    self.exporting = false;
                    self.notice = Some(format!("{} {}", self.text("Saved to"), path.display()));
                }
                Event::ExportFailed(error) => {
                    self.exporting = false;
                    self.fail(error);
                }
            }
        }
        for batch in &mut self.batches {
            batch.snapshot = batch.handle.snapshot();
        }
        if self
            .batches
            .iter()
            .any(|batch| !batch.snapshot.status.is_terminal())
            || self.submitting
            || self.saving
            || self.download_cancel.is_some()
            || self.exporting
        {
            context.request_repaint_after(Duration::from_millis(150));
        }
        if let Some(renderer) = &self.preview {
            while let Some(result) = renderer.try_take_result() {
                match result {
                    PreviewResult::Frame(frame)
                        if self.preview_ticket == Some(frame.ticket)
                            && self.preview_generation == frame.generation
                            && self.preview_key.as_deref() == Some(&frame.document_key) =>
                    {
                        let bytes = frame
                            .width
                            .checked_mul(frame.height)
                            .and_then(|count| count.checked_mul(4))
                            .and_then(|count| usize::try_from(count).ok());
                        if frame.width == 0
                            || frame.height == 0
                            || frame.width > 1600
                            || frame.height > 1600
                            || bytes != Some(frame.rgba.len())
                        {
                            self.preview_error = Some("Preview returned an invalid bitmap".into());
                            continue;
                        }
                        self.page = frame.page_one_based;
                        self.page_count = frame.page_count;
                        let bitmap = egui::ColorImage::from_rgba_unmultiplied(
                            [frame.width as usize, frame.height as usize],
                            &frame.rgba,
                        );
                        self.texture = Some(context.load_texture(
                            "pdf-page",
                            bitmap,
                            egui::TextureOptions::LINEAR,
                        ));
                        self.preview_error = None;
                    }
                    PreviewResult::Error(failure)
                        if failure.ticket.is_none() || failure.ticket == self.preview_ticket =>
                    {
                        if failure.ticket.is_none() {
                            self.preview_unavailable = true;
                        }
                        self.preview_error = Some(failure.error.to_string());
                        self.preview_pending = None;
                        self.preview_ticket = None;
                    }
                    _ => {}
                }
            }
        }
    }
    fn submit(&mut self) {
        match self.editor.snapshot(true) {
            Ok(settings) => {
                if self.command(Command::Translate(settings)) {
                    self.submitting = true;
                    self.notice = None;
                    self.error = None;
                }
            }
            Err(error) => self.fail(error),
        }
    }
    fn save(&mut self) {
        match self.editor.snapshot(false) {
            Ok(settings) => {
                self.save_revision = self.editor.revision;
                if self.command(Command::Save(settings)) {
                    self.saving = true;
                }
            }
            Err(error) => self.fail(error),
        }
    }
    fn heading(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading("PDFMathTranslate");
                ui.label(
                    RichText::new(self.text("Translate PDFs, keep the layout"))
                        .color(Color32::from_gray(95)),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let language = if self.chinese {
                    "English"
                } else {
                    "繁體中文"
                };
                if ui.small_button(language).clicked() {
                    self.chinese = !self.chinese;
                    self.edit("gui_settings.ui_lang", json!(self.text("en")));
                }
                if ui
                    .add_enabled(!self.saving, egui::Button::new(self.text("Save settings")))
                    .clicked()
                {
                    self.save();
                }
                if ui
                    .add_enabled(
                        !self.saving && !self.editor.dirty,
                        egui::Button::new(self.text("Reload settings")),
                    )
                    .on_hover_text(self.text("Save edits before reloading"))
                    .clicked()
                {
                    self.save_revision = self.editor.revision;
                    if self.command(Command::Reload) {
                        self.saving = true;
                    }
                }
                let can_submit = !self.submitting
                    && !self.saving
                    && self.download_cancel.is_none()
                    && !self.editor.files.is_empty()
                    && self
                        .editor
                        .engine_enabled(self.editor.settings.main_engine());
                if ui
                    .add_enabled(
                        can_submit,
                        egui::Button::new(
                            RichText::new(self.text("Translate")).color(Color32::WHITE),
                        )
                        .fill(Color32::from_rgb(33, 102, 183))
                        .stroke(egui::Stroke::NONE),
                    )
                    .clicked()
                {
                    self.submit();
                }
                if self.editor.dirty {
                    ui.label(
                        RichText::new(self.text("Unsaved changes"))
                            .small()
                            .color(Color32::from_gray(100)),
                    );
                }
            });
        });
    }
    fn notices(&mut self, ui: &mut egui::Ui) {
        if let Some(message) = self.error.clone() {
            egui::Frame::new()
                .fill(Color32::from_rgb(255, 237, 235))
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.colored_label(Color32::from_rgb(160, 38, 38), message);
                        if ui.small_button(self.text("Dismiss")).clicked() {
                            self.error = None;
                        }
                    });
                });
        }
        if let Some(message) = self.notice.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(message);
                if ui.small_button("×").clicked() {
                    self.notice = None;
                }
            });
        }
    }
    fn files(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.horizontal(|ui| {
            ui.strong(self.text("PDF documents"));
            ui.label(format!("({})", self.editor.files.len()));
            if ui.button(self.text("Add…")).clicked()
                && let Some(paths) = dialogs::pdfs(frame)
                && let Err(error) = self.editor.add_files(paths)
            {
                self.fail(error);
            }
        });
        if self.editor.files.is_empty() {
            egui::Frame::new()
                .stroke(egui::Stroke::new(1.0, Color32::from_gray(200)))
                .inner_margin(16.0)
                .show(ui, |ui| {
                    ui.label(self.text("Drop PDFs here, or choose Add"));
                });
        }
        let mut remove = None;
        let remove_label = self.text("Remove");
        egui::ScrollArea::vertical()
            .id_salt("input-documents")
            .max_height(145.0)
            .show(ui, |ui| {
                for (index, path) in self.editor.files.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        if ui
                            .selectable_label(
                                self.selected_batch.is_none() && self.original_file == index,
                                name,
                            )
                            .on_hover_text(path.display().to_string())
                            .clicked()
                        {
                            self.original_file = index;
                            self.selected_batch = None;
                            self.kind = DocumentKind::Original;
                        }
                        if ui.small_button("×").on_hover_text(remove_label).clicked() {
                            remove = Some(index);
                        }
                    });
                }
            });
        if let Some(index) = remove {
            self.editor.remove_file(index);
            self.original_file = self
                .original_file
                .min(self.editor.files.len().saturating_sub(1));
        }
    }
    fn engine(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, term: bool) {
        let current = if term {
            self.editor.explicit_term_engine()
        } else {
            Some(self.editor.settings.main_engine())
        };
        let selected = current.map(|engine| engine.flag.clone());
        let label = current.map_or_else(
            || self.text("Same as document").to_string(),
            |engine| engine.name.clone(),
        );
        let mut next = selected.clone();
        egui::ComboBox::from_id_salt(if term { "term-engine" } else { "main-engine" })
            .selected_text(label)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                if term {
                    ui.selectable_value(&mut next, None, self.text("Same as document"));
                }
                for engine in engine_specs().iter().filter(|engine| {
                    (!term || engine.supports_llm) && self.editor.engine_enabled(engine)
                }) {
                    ui.selectable_value(&mut next, Some(engine.flag.clone()), &engine.name);
                }
            });
        if next != selected
            && let Err(error) = self.editor.select_engine(next.as_deref(), term)
        {
            self.fail(error);
        }
        if current.is_some_and(|engine| !self.editor.engine_enabled(engine)) {
            ui.colored_label(
                Color32::from_rgb(160, 38, 38),
                self.text("The selected service is disabled"),
            );
        }
        let engine = if term {
            self.editor.explicit_term_engine()
        } else {
            Some(self.editor.settings.main_engine())
        };
        if let Some(engine) = engine {
            self.engine_fields(ui, frame, engine, term);
        }
    }
    fn engine_fields(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        engine: &EngineSpec,
        term: bool,
    ) {
        let Some(detail) = &engine.detail_key else {
            return;
        };
        let prefix = if term {
            format!("term_{detail}.")
        } else {
            format!("{detail}.")
        };
        let specs: Vec<_> = field_specs()
            .iter()
            .filter(|spec| spec.path.starts_with(&prefix))
            .collect();
        let mut advanced = Vec::new();
        for spec in specs {
            let leaf = spec.path.rsplit('.').next().unwrap_or_default();
            if !crate::settings_ui::advanced(spec)
                || spec.secret
                || leaf.ends_with("_base_url")
                || leaf.ends_with("_endpoint")
                || leaf.ends_with("_host")
                || leaf.ends_with("_api_url")
            {
                self.field(ui, frame, spec, None);
            } else {
                advanced.push(spec);
            }
        }
        if !advanced.is_empty() {
            egui::CollapsingHeader::new(self.text("Service options"))
                .id_salt(("service-options", term))
                .show(ui, |ui| {
                    for spec in advanced {
                        self.field(ui, frame, spec, None);
                    }
                });
        }
    }
    fn field(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        spec: &FieldSpec,
        label: Option<&str>,
    ) {
        if !crate::settings_ui::visible(&self.editor, spec) {
            return;
        }
        if let Err(error) =
            crate::settings_ui::field(ui, &mut self.editor, spec, label, self.chinese, frame)
        {
            self.fail(error);
        }
    }
    fn field_path(
        &mut self,
        ui: &mut egui::Ui,
        frame: &eframe::Frame,
        path: &str,
        label: Option<&str>,
    ) {
        if let Some(spec) = field_specs().iter().find(|spec| spec.path == path) {
            self.field(ui, frame, spec, label);
        }
    }
    fn settings_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        self.files(ui, frame);
        self.url_input(ui);
        ui.separator();
        ui.strong(self.text("Translation service"));
        self.engine(ui, frame, false);
        egui::CollapsingHeader::new(self.text("Manage services"))
            .id_salt("enabled-services")
            .show(ui, |ui| {
                ui.small(self.text("Disabled services keep their saved settings"));
                if ui.small_button(self.text("All services")).clicked() {
                    self.edit("gui_settings.enabled_services", Value::Null);
                }
                for engine in engine_specs() {
                    let mut enabled = self.editor.engine_enabled(engine);
                    if ui.checkbox(&mut enabled, &engine.name).changed()
                        && let Err(error) = self.editor.toggle_service(engine, enabled)
                    {
                        self.fail(error);
                    }
                }
            });
        ui.separator();
        self.language(ui, "translation.lang_in", true);
        ui.horizontal(|ui| {
            if ui
                .button("⇄")
                .on_hover_text(self.text("Swap languages"))
                .clicked()
                && let Err(error) = self.editor.swap_languages()
            {
                self.fail(error);
            }
            ui.small(self.text("Options depend on the selected service"));
        });
        self.language(ui, "translation.lang_out", false);
        egui::CollapsingHeader::new(self.text("Output and terminology"))
            .default_open(true)
            .show(ui, |ui| {
                self.field_path(
                    ui,
                    frame,
                    "translation.output",
                    Some(self.text("Output folder")),
                );
                self.output_choice(ui, "pdf.no_mono", self.text("Translated PDF"));
                self.output_choice(ui, "pdf.no_dual", self.text("Bilingual PDF"));
                ui.label(self.text("Page selection"));
                ui.horizontal_wrapped(|ui| {
                    for (name, pages) in [
                        ("All pages", None),
                        ("First page", Some("1")),
                        ("First five pages", Some("1-5")),
                    ] {
                        if ui.small_button(self.text(name)).clicked() {
                            self.edit("pdf.pages", json!(pages));
                        }
                    }
                });
                self.field_path(ui, frame, "pdf.pages", Some(self.text("Custom pages")));
                ui.small(self.text("Page ranges help"));
                if let Some(draft) = self.editor.drafts.get("pdf.pages") {
                    let mut probe = self.editor.settings.clone();
                    if probe.set("pdf.pages", json!(draft)).is_err() || probe.parse_pages().is_err()
                    {
                        ui.colored_label(
                            Color32::from_rgb(160, 38, 38),
                            self.text("Invalid page selection"),
                        );
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(self.text("Glossary CSV"));
                    if ui.small_button("…").clicked()
                        && let Some(path) = dialogs::glossary(frame)
                    {
                        self.edit("translation.glossaries", json!(path.to_string_lossy()));
                    }
                });
                let llm = self.editor.settings.main_engine().supports_llm;
                self.field_path(
                    ui,
                    frame,
                    "translation.glossaries",
                    Some(self.text("Separate multiple files with commas")),
                );
                if !llm {
                    ui.small(self.text("Glossaries require an LLM service"));
                }
                egui::CollapsingHeader::new(self.text("Terminology service")).show(ui, |ui| {
                    self.engine(ui, frame, true);
                    let available = self.editor.settings.term_engine().is_some();
                    ui.add_enabled_ui(available, |ui| {
                        self.output_choice(
                            ui,
                            "translation.no_auto_extract_glossary",
                            self.text("Extract terminology automatically"),
                        );
                        let active = self
                            .editor
                            .settings
                            .translation()
                            .is_ok_and(|translation| !translation.no_auto_extract_glossary);
                        ui.add_enabled_ui(active, |ui| {
                            self.field_path(
                                ui,
                                frame,
                                "translation.save_auto_extracted_glossary",
                                None,
                            );
                            self.field_path(ui, frame, "translation.term_qps", None);
                            self.field_path(ui, frame, "translation.term_pool_max_workers", None);
                        });
                    });
                    if !available {
                        ui.small(self.text("Terminology requires an LLM service"));
                    }
                    ui.small(self.text("Terminology limits help"));
                });
            });
        egui::CollapsingHeader::new(self.text("Performance and rate limits")).show(ui, |ui| {
            self.field_path(ui, frame, "translation.qps", None);
            self.field_path(ui, frame, "translation.pool_max_workers", None);
            self.field_path(ui, frame, "translation.ignore_cache", None);
        });
        let common = [
            "translation.lang_in",
            "translation.lang_out",
            "translation.output",
            "pdf.pages",
            "pdf.no_mono",
            "pdf.no_dual",
            "translation.glossaries",
            "translation.no_auto_extract_glossary",
            "translation.save_auto_extracted_glossary",
            "translation.term_qps",
            "translation.term_pool_max_workers",
            "translation.qps",
            "translation.pool_max_workers",
            "translation.ignore_cache",
            "gui_settings.enabled_services",
            "basic.input_files",
            "basic.gui",
            "basic.version",
            "basic.warmup",
            "basic.generate_offline_assets",
            "basic.restore_offline_assets",
            "gui_settings.share",
            "gui_settings.auth_file",
            "gui_settings.welcome_page",
            "gui_settings.server_port",
            "gui_settings.server_name",
        ];
        egui::CollapsingHeader::new(self.text("Advanced options")).show(ui, |ui| {
            ui.small(self.text("Settings are written only when you choose Save settings. Unknown settings are preserved."));
            let hint = self.text("Find a setting");
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(hint).desired_width(f32::INFINITY));
            let search = self.search.to_lowercase();
            let flags: Vec<_> = engine_specs().iter().flat_map(|engine| [engine.flag.clone(), format!("term_{}", engine.flag)]).collect();
            for group in ["Basic translation settings", "PDF processing", "Other advanced settings"] {
                egui::CollapsingHeader::new(self.text(group)).show(ui, |ui| {
                    for spec in field_specs().iter().filter(|spec| !common.contains(&spec.path.as_str()) && !flags.contains(&spec.path) && !spec.path.contains("_detail.") && crate::settings_ui::group(spec) == group) {
                        let label = crate::strings::field_label(spec, self.chinese);
                        if search.is_empty() || spec.path.to_lowercase().contains(&search) || label.to_lowercase().contains(&search) {
                            self.field(ui, frame, spec, None);
                        }
                    }
                });
            }
        });
        // Even a hidden/inactive service cannot trap an invalid pending edit.
        let invalid = self.editor.invalid_fields();
        if !invalid.is_empty() {
            egui::CollapsingHeader::new(self.text("Pending edits requiring attention"))
                .default_open(true)
                .show(ui, |ui| {
                    for spec in invalid {
                        ui.push_id(("invalid-field", &spec.path), |ui| {
                            if let Err(error) = crate::settings_ui::field(
                                ui,
                                &mut self.editor,
                                spec,
                                None,
                                self.chinese,
                                frame,
                            ) {
                                self.fail(error);
                            }
                        });
                    }
                });
        }
    }
    fn language(&mut self, ui: &mut egui::Ui, path: &str, source: bool) {
        ui.push_id(path, |ui| {
            ui.label(self.text(if source {
                "Input language"
            } else {
                "Output language"
            }));
            let current = self
                .editor
                .drafts
                .get(path)
                .cloned()
                .or_else(|| {
                    self.editor
                        .settings
                        .get(path)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            let mut selected = current.clone();
            let engine = self.editor.settings.main_engine().name.clone();
            egui::ComboBox::from_id_salt("language")
                .selected_text(crate::languages::display(&current, self.chinese))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for language in crate::languages::CATALOG.iter().filter(|language| {
                        crate::languages::supported(&engine, language.code, source) != Some(false)
                    }) {
                        ui.selectable_value(
                            &mut selected,
                            language.code.to_owned(),
                            crate::languages::display(language.code, self.chinese),
                        );
                    }
                });
            if selected != current {
                self.edit(path, json!(selected));
            }
            egui::CollapsingHeader::new(self.text("Custom language code"))
                .id_salt("custom-language")
                .show(ui, |ui| {
                    let spec = field_specs()
                        .iter()
                        .find(|spec| spec.path == path)
                        .expect("language field");
                    let changed = ui
                        .add(
                            egui::TextEdit::singleline(self.editor.draft(spec))
                                .desired_width(f32::INFINITY),
                        )
                        .changed();
                    if changed {
                        self.editor.mark_dirty();
                    }
                });
            if crate::languages::supported(&engine, &current, source) == Some(false) {
                ui.colored_label(
                    Color32::from_rgb(160, 38, 38),
                    self.text("The selected service does not support this language"),
                );
            }
        });
    }
    fn url_input(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new(self.text("Download PDF URL")).show(ui, |ui| {
            let hint = self.text("Enter a valid HTTP or HTTPS URL");
            ui.add_enabled(
                self.download_cancel.is_none(),
                egui::TextEdit::singleline(&mut self.url)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            );
            if let Some(cancel) = &self.download_cancel {
                ui.horizontal(|ui| {
                    ui.spinner();
                    if ui.button(self.text("Cancel download")).clicked() {
                        cancel.cancel();
                    }
                });
            } else if ui
                .add_enabled(
                    !self.url.trim().is_empty(),
                    egui::Button::new(self.text("Download")),
                )
                .clicked()
            {
                let cancel = tokio_util::sync::CancellationToken::new();
                if self.command(Command::Download {
                    url: self.url.clone(),
                    cancel: cancel.clone(),
                }) {
                    self.download_cancel = Some(cancel);
                    self.error = None;
                }
            }
            ui.small(self.text("Public PDF URL limits"));
        });
    }
    fn output_choice(&mut self, ui: &mut egui::Ui, path: &str, label: &str) {
        let mut enabled = !self
            .editor
            .settings
            .get(path)
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if ui.checkbox(&mut enabled, label).changed() {
            self.edit(path, json!(!enabled));
        }
    }
    fn status_text(&self, status: TaskStatus) -> &'static str {
        match status {
            TaskStatus::Queued => self.text("Queued"),
            TaskStatus::Starting => self.text("Preparing"),
            TaskStatus::Running => self.text("Translating"),
            TaskStatus::Cancelling => self.text("Cancelling and cleaning up"),
            TaskStatus::Succeeded => self.text("Complete"),
            TaskStatus::Failed => self.text("Failed"),
            TaskStatus::Cancelled => self.text("Cancelled"),
        }
    }
    fn task_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        if self.submitting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(self.text("Preparing translation…"));
            });
        }
        if self.batches.is_empty() {
            return;
        }
        let label = self
            .selected_batch
            .and_then(|index| self.batches.get(index))
            .map(|batch| {
                format!(
                    "#{} · {}",
                    batch.handle.id(),
                    self.status_text(batch.snapshot.status)
                )
            })
            .unwrap_or_else(|| self.text("Choose a translation").into());
        let source_documents = self.text("Source documents");
        egui::ComboBox::from_id_salt("batch")
            .selected_text(label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.selected_batch, None, source_documents);
                for (index, batch) in self.batches.iter().enumerate() {
                    let label = format!(
                        "#{} · {} · {} PDF",
                        batch.handle.id(),
                        self.status_text(batch.snapshot.status),
                        batch.snapshot.total_files
                    );
                    ui.selectable_value(&mut self.selected_batch, Some(index), label);
                }
            });
        let Some(batch) = self
            .selected_batch
            .and_then(|index| self.batches.get(index))
        else {
            return;
        };
        let snapshot = batch.snapshot.clone();
        let inputs = batch.inputs.clone();
        ui.horizontal(|ui| {
            ui.strong(self.status_text(snapshot.status));
            ui.label(format!(
                "{} / {} PDF",
                snapshot.completed_files, snapshot.total_files
            ));
            if !snapshot.status.is_terminal()
                && ui
                    .add_enabled(
                        snapshot.status != TaskStatus::Cancelling,
                        egui::Button::new(self.text("Cancel")),
                    )
                    .clicked()
            {
                batch.handle.cancel();
            }
        });
        self.export_batch(ui, frame, &snapshot);
        if let Some(fraction) = progress_fraction(&snapshot) {
            ui.add(
                egui::ProgressBar::new(fraction)
                    .show_percentage()
                    .animate(!snapshot.status.is_terminal()),
            );
        } else if !snapshot.status.is_terminal() {
            ui.spinner();
        }
        if let Some(stage) = snapshot
            .progress
            .as_ref()
            .and_then(|event| event.get("stage"))
            .and_then(Value::as_str)
        {
            ui.label(crate::strings::stage(stage, self.chinese));
        }
        if let Some(failure) = &snapshot.failure {
            ui.colored_label(
                Color32::from_rgb(160, 38, 38),
                crate::strings::friendly_error(&failure.message, self.chinese),
            );
        }
        self.selected_file = self.selected_file.min(inputs.len().saturating_sub(1));
        egui::ComboBox::from_id_salt("result-file")
            .selected_text(
                inputs
                    .get(self.selected_file)
                    .and_then(|path| path.file_name())
                    .unwrap_or_default()
                    .to_string_lossy(),
            )
            .show_ui(ui, |ui| {
                for (index, path) in inputs.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.selected_file,
                        index,
                        path.file_name().unwrap_or_default().to_string_lossy(),
                    );
                }
            });
    }
    fn selected_path(&self) -> Option<(PathBuf, String)> {
        if let Some(index) = self.selected_batch {
            let batch = self.batches.get(index)?;
            let path = result_path(
                &batch.snapshot,
                self.selected_file,
                self.kind,
                &batch.inputs,
            )?;
            Some((
                std::path::absolute(path).ok()?,
                format!(
                    "task:{}:file:{}:{:?}",
                    batch.handle.id(),
                    self.selected_file,
                    self.kind
                ),
            ))
        } else if self.kind == DocumentKind::Original {
            let path = std::path::absolute(self.editor.files.get(self.original_file)?).ok()?;
            Some((path, format!("input:{}", self.original_file)))
        } else {
            None
        }
    }
    fn export_batch(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, snapshot: &TaskSnapshot) {
        use crate::model::{ExportKind, batch_pdf_results};
        ui.add_enabled_ui(
            !self.exporting && snapshot.status == TaskStatus::Succeeded,
            |ui| {
                ui.menu_button(self.text("Export batch ZIP"), |ui| {
                    for (name, kind) in [
                        ("All PDF results", ExportKind::All),
                        ("Mono PDF results", ExportKind::Mono),
                        ("Dual PDF results", ExportKind::Dual),
                    ] {
                        let sources = batch_pdf_results(snapshot, kind);
                        if ui
                            .add_enabled(sources.is_ok(), egui::Button::new(self.text(name)))
                            .clicked()
                        {
                            ui.close();
                            if let Ok(sources) = sources
                                && let Some(destination) = dialogs::save(
                                    frame,
                                    std::path::Path::new("translation-results.zip"),
                                )
                                && self.command(Command::ExportZip {
                                    sources,
                                    destination,
                                })
                            {
                                self.exporting = true;
                            }
                        }
                    }
                });
            },
        );
        if self.exporting {
            ui.spinner();
        }
    }
    fn preview_panel(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        self.task_panel(ui, frame);
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            for (kind, _zh, en) in [
                (DocumentKind::Original, "原文", "Original"),
                (DocumentKind::Mono, "譯文", "Translated"),
                (DocumentKind::Dual, "雙語", "Bilingual"),
            ] {
                let label = self.text(en);
                ui.selectable_value(&mut self.kind, kind, label);
            }
            let path = self.selected_path().map(|(path, _)| path);
            if ui
                .add_enabled(path.is_some(), egui::Button::new(self.text("Open")))
                .clicked()
                && let Some(path) = path.clone()
            {
                self.command(Command::Open(path));
            }
            if ui
                .add_enabled(path.is_some(), egui::Button::new(self.text("Save as…")))
                .clicked()
                && let Some(source) = path
                && let Some(destination) = dialogs::save(frame, &source)
            {
                self.command(Command::Copy {
                    source,
                    destination,
                });
            }
            if let Some(batch) = self
                .selected_batch
                .and_then(|index| self.batches.get(index))
                && let Some(path) = batch
                    .snapshot
                    .results
                    .get(self.selected_file)
                    .and_then(|result| result.get("auto_extracted_glossary_path"))
                    .and_then(Value::as_str)
                    .filter(|path| !path.is_empty())
            {
                let source = PathBuf::from(path);
                if ui.button(self.text("Save glossary…")).clicked()
                    && let Some(destination) = dialogs::save(frame, &source)
                {
                    self.command(Command::Copy {
                        source,
                        destination,
                    });
                }
            }
        });
        let mut rerender = false;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.page > 1, egui::Button::new("‹"))
                .clicked()
            {
                self.page -= 1;
                rerender = true;
            }
            ui.label(self.text("Page"));
            rerender |= ui
                .add(
                    egui::DragValue::new(&mut self.page)
                        .range(1..=self.page_count.max(1))
                        .speed(0.1),
                )
                .changed();
            if self.page_count > 0 {
                ui.label(format!("/ {}", self.page_count));
            }
            if ui
                .add_enabled(self.page_count > self.page, egui::Button::new("›"))
                .clicked()
            {
                self.page += 1;
                rerender = true;
            }
            ui.separator();
            let zoom_label = self.text("Zoom");
            let zoom_changed = ui
                .add(
                    egui::Slider::new(&mut self.zoom, 0.5..=2.0)
                        .text(zoom_label)
                        .step_by(0.25),
                )
                .changed();
            if zoom_changed {
                rerender = true;
                self.fit_to_page = false;
            }
            if ui.small_button(self.text("Fit page")).clicked() {
                self.fit_to_page = true;
                self.zoom = 1.25;
                rerender = true;
            }
            if ui.small_button(self.text("Reload")).clicked() {
                rerender = true;
            }
        });
        let selected = self.selected_path();
        let desired = selected
            .as_ref()
            .map(|(path, key)| format!("{key}:{}", path.display()));
        if desired != self.preview_key {
            self.preview_key = desired.clone();
            self.page = 1;
            self.page_count = 0;
            self.fit_to_page = true;
            rerender = true;
        }
        if rerender {
            self.texture = None;
            self.preview_ticket = None;
            self.preview_generation = self.preview_generation.wrapping_add(1);
            if let Some((path, _)) = selected {
                let mut request = PreviewRequest::new(path, self.page);
                request.document_key = desired.unwrap_or_default();
                request.generation = self.preview_generation;
                request.zoom = self.zoom;
                if !self.preview_unavailable {
                    self.preview_pending = Some(request);
                }
                if !self.preview_unavailable {
                    self.preview_error = None;
                }
            } else {
                self.preview_pending = None;
            }
        }
        if let (Some(renderer), Some(request)) = (&self.preview, self.preview_pending.as_ref()) {
            match renderer.request(request.clone()) {
                Ok(ticket) => {
                    self.preview_ticket = Some(ticket);
                    self.preview_pending = None;
                }
                Err(error) if error.kind == PreviewErrorKind::Busy => {
                    ui.ctx().request_repaint_after(Duration::from_millis(16))
                }
                Err(error) => {
                    self.preview_error = Some(error.to_string());
                    self.preview_pending = None;
                }
            }
        }
        let available = ui.available_size().max(Vec2::new(100.0, 100.0));
        egui::Frame::new().fill(Color32::from_gray(231)).inner_margin(12.0).show(ui,|ui| {
            ui.set_min_size(available-Vec2::splat(24.0));
            egui::ScrollArea::both().id_salt("pdf-preview-scroll").auto_shrink([false,false]).show(ui,|ui| {
                if let Some(texture)=&self.texture {
                    let size=texture.size_vec2();
                    let scale=if self.fit_to_page {((available.x-24.0)/size.x).min((available.y-24.0)/size.y).clamp(0.01,1.0)}else{1.0};
                    ui.add(egui::Image::new(texture).fit_to_exact_size(size*scale));
                }
                else if let Some(error)=&self.preview_error {ui.vertical_centered(|ui| {ui.add_space(40.0);ui.label(error);ui.label(self.text("You can still open or save result files."));});}
                else if self.preview_ticket.is_some()||self.preview_pending.is_some() {ui.horizontal(|ui| {ui.spinner();ui.label(self.text("Loading preview…"));});}
                else {ui.vertical_centered(|ui| {ui.add_space(60.0);ui.heading(self.text("Preview your PDF here"));ui.label(self.text("Add a document to preview the original. Switch to translated or bilingual output when it is ready."));});}
            });
        });
    }
}

impl eframe::App for NativeApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(context);
        if context.input(|input| input.viewport().close_requested()) {
            self.begin_close();
        }
        if self.background.control.is_closing() {
            if self.background.control.is_done() {
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                context.request_repaint_after(Duration::from_millis(40));
            }
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let closing = self.background.control.is_closing();
        if !closing && !self.saving {
            let dropped = ui.ctx().input(|input| {
                input
                    .raw
                    .dropped_files
                    .iter()
                    .map(|file| file.path().to_path_buf())
                    .collect::<Vec<_>>()
            });
            if !dropped.is_empty()
                && let Err(error) = self.editor.add_files(dropped)
            {
                self.fail(error);
            }
        }
        egui::Frame::central_panel(ui.style())
            .inner_margin(18.0)
            .show(ui, |ui| {
                ui.add_enabled_ui(!closing && !self.saving, |ui| self.heading(ui));
                self.notices(ui);
                ui.separator();
                if closing {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(self.text("Cancelling tasks and closing safely…"));
                    });
                }
                let height = ui.available_height();
                let width = (ui.available_width() * 0.34).clamp(310.0, 420.0);
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_max_width(width);
                            ui.add_enabled_ui(!closing && !self.saving, |ui| {
                                egui::ScrollArea::vertical()
                                    .id_salt("settings-scroll")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| self.settings_panel(ui, frame));
                            });
                        },
                    );
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        Vec2::new(ui.available_width(), height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.add_enabled_ui(!closing, |ui| self.preview_panel(ui, frame));
                        },
                    );
                });
            });
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.begin_close();
    }
}
impl Drop for NativeApp {
    fn drop(&mut self) {
        self.begin_close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::background::BackgroundThread;
    use eframe::App;
    use pdf2zh_config::{ConfigStore, Settings};
    struct Harness {
        app: NativeApp,
        _thread: BackgroundThread,
        directory: tempfile::TempDir,
    }
    impl Harness {
        fn new(settings: Settings) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let store = ConfigStore::new(directory.path().into()).unwrap();
            let worker = babeldoc_bridge::WorkerOptions {
                python: directory.path().join("not-a-worker.exe"),
                script: directory.path().join("not-a-script.py"),
                ..Default::default()
            };
            let (background, thread) = Background::start(store, worker, None).unwrap();
            let app = NativeApp::with_renderer(settings, background, false, None, None);
            Self {
                app,
                _thread: thread,
                directory,
            }
        }
        fn paint(&mut self, input: egui::RawInput) -> egui::FullOutput {
            let context = egui::Context::default();
            let mut frame = eframe::Frame::_new_kittest();
            context.run_ui(input, |ui| self.app.ui(ui, &mut frame))
        }
    }
    fn input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                Vec2::new(1180.0, 820.0),
            )),
            ..Default::default()
        }
    }

    #[test]
    fn a_real_headless_frame_does_not_start_workers_or_auto_save() {
        let mut settings = Settings::default();
        settings
            .set("gui_settings.disable_config_auto_save", json!(true))
            .unwrap();
        let mut harness = Harness::new(settings);
        harness.app.edit("translation.lang_out", json!("ja"));
        let output = harness.paint(input());
        assert!(!output.shapes.is_empty());
        output.drop_without_applying_deltas();
        assert!(harness.app.editor.dirty);
        assert_eq!(
            std::fs::read_dir(harness.directory.path()).unwrap().count(),
            0
        );
        harness.app.begin_close();
        assert!(harness.app.background.control.is_closing());
    }
    #[test]
    fn secret_values_are_masked_in_actual_egui_paint_output() {
        let secret = "synthetic-unique-password-123456";
        let mut settings = Settings::default();
        settings
            .set("openai_detail.openai_api_key", json!(secret))
            .unwrap();
        let mut harness = Harness::new(settings);
        let context = egui::Context::default();
        let spec = field_specs()
            .iter()
            .find(|spec| spec.path == "openai_detail.openai_api_key")
            .unwrap();
        assert!(spec.secret);
        let output = context.run_ui(input(), |ui| {
            harness
                .app
                .field(ui, &eframe::Frame::_new_kittest(), spec, None)
        });
        let painted = format!("{:?}", output.shapes);
        output.drop_without_applying_deltas();
        assert!(!painted.contains(secret));
        assert_eq!(
            harness.app.editor.settings.get(&spec.path),
            Some(&json!(secret))
        );
    }
    #[derive(Debug)]
    struct Dropped(PathBuf);
    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            panic!("The UI must not synchronously read dropped PDFs")
        }
    }
    #[test]
    fn native_drop_adds_multiple_pdfs_and_removal_updates_selection() {
        let mut harness = Harness::new(Settings::default());
        let mut event = input();
        let first = harness.directory.path().join("first.pdf");
        let second = harness.directory.path().join("第二份.PDF");
        event.dropped_files = vec![
            Arc::new(Dropped(first.clone())),
            Arc::new(Dropped(second.clone())),
        ];
        harness.paint(event).drop_without_applying_deltas();
        assert_eq!(harness.app.editor.files, vec![first, second]);
        harness.app.editor.remove_file(0);
        assert_eq!(harness.app.editor.files.len(), 1);
        assert_eq!(
            std::fs::read_dir(harness.directory.path()).unwrap().count(),
            0
        );
    }
}
