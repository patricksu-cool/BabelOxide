//! User-facing text; technical identifiers are kept as hover text.
use pdf2zh_config::FieldSpec;

pub fn ui(key: &'static str, chinese: bool) -> &'static str {
    if !chinese {
        return key;
    }
    match key {
        "Documents and settings" => "文件與常用設定",
        "Add PDFs, choose a preset, then translate" => "加入 PDF → 選擇常用組合 → 開始翻譯",
        "Configure service and API key…" => "設定服務網址與 API 密鑰…",
        "Output and page range" => "輸出位置與頁碼範圍",
        "Save or finish the current operation first" => "請先完成目前操作，或確認翻譯服務已啟用。",
        "Translation presets" => "翻譯預設組合",
        "Custom" => "自訂",
        "Save or rename preset" => "保存／重新命名組合",
        "Preset name" => "組合名稱",
        "Presets keep API keys and request limits unchanged" => {
            "組合保留各服務密鑰、網址、限流與並行設定。"
        }
        "Save preset" => "保存組合",
        "Rename" => "重新命名",
        "Task queue" => "任務佇列",
        "PDFs run one at a time; request concurrency is unchanged" => {
            "PDF 依序處理；文字請求沿用原有並行設定。"
        }
        "Settings" => "設定",
        "Basic" => "基本",
        "Provider" => "服務",
        "Performance" => "效能",
        "Advanced" => "進階",
        "Translate workspace" => "翻譯工作區",
        "Preview and results" => "預覽與成果",
        "Unprocessed PDFs can be retried with the original settings" => {
            "未處理的文件可使用原任務設定重試；已完成成果會保留。"
        }
        "Retry unfinished PDFs" => "重試未完成文件",
        "Open folder" => "開啟資料夾",
        "Settings saved" => "設定已保存",
        "Saved snapshot; newer edits remain unsaved" => "已保存先前的設定；較新的修改尚未保存。",
        "Saved to" => "已另存為",
        "Translate PDFs, keep the layout" => "翻譯 PDF 並保留版面",
        "Save settings" => "保存設定",
        "Translate" => "開始翻譯",
        "Unsaved changes" => "尚未保存",
        "Dismiss" => "關閉",
        "PDF documents" => "PDF 文件",
        "Add…" => "加入…",
        "Drop PDFs here, or choose Add" => "將 PDF 拖放到這裡，或按「加入」",
        "Same as document" => "與正文相同",
        "Service options" => "服務選項",
        "Translation service" => "翻譯服務",
        "Output and terminology" => "輸出與術語",
        "Output folder" => "輸出資料夾",
        "Leave empty to use the source folder" => "留空使用原文件資料夾",
        "Translated PDF" => "譯文 PDF",
        "Bilingual PDF" => "雙語 PDF",
        "Original" => "原文",
        "Translated" => "譯文",
        "Bilingual" => "雙語",
        "Pages (e.g. 1-3,5; empty for all)" => "頁碼（例如 1-3,5；留空為全部）",
        "Glossary CSV" => "術語表 CSV",
        "Separate multiple files with commas" => "多份以逗號分隔",
        "Extract terminology automatically" => "自動擷取術語",
        "Save extracted glossary" => "保存擷取的術語表",
        "Terminology service" => "術語翻譯服務",
        "Terminology requests per second" => "術語每秒請求數",
        "Concurrent terminology requests" => "術語同時請求數",
        "Speed and layout" => "速度與版面",
        "Requests per second" => "每秒請求數",
        "Concurrent requests (empty for automatic)" => "同時請求數（留空為自動）",
        "Ignore translation cache" => "略過翻譯快取",
        "Alternate original and translated pages" => "雙語 PDF 交錯頁面",
        "Translation first" => "譯文在前",
        "Translate table text" => "翻譯表格文字",
        "Use OCR" => "使用 OCR",
        "Detect when OCR is needed" => "自動偵測 OCR",
        "All settings" => "所有設定",
        "Settings are written only when you choose Save settings. Unknown settings are preserved." => {
            "按「保存設定」才會寫入；未識別的設定會保留。"
        }
        "Find a setting" => "搜尋設定",
        "Queued" => "排隊中",
        "Preparing" => "準備中",
        "Translating" => "翻譯中",
        "Cancelling and cleaning up" => "正在取消並清理",
        "Complete" => "已完成",
        "Failed" => "失敗",
        "Cancelled" => "已取消",
        "Preparing translation…" => "正在建立翻譯任務…",
        "Choose a translation" => "選擇翻譯任務",
        "Cancel" => "取消",
        "Open" => "開啟",
        "Save as…" => "另存…",
        "Save glossary…" => "保存術語表…",
        "Page" => "頁",
        "Fit page" => "整頁",
        "Reload" => "重新載入",
        "You can still open or save result files." => "仍可開啟或保存輸出文件。",
        "Loading preview…" => "正在載入預覽…",
        "Preview your PDF here" => "在這裡預覽 PDF",
        "Add a document to preview the original. Switch to translated or bilingual output when it is ready." => {
            "加入文件後可查看原文，完成翻譯後可切換譯文與雙語版。"
        }
        "Cancelling tasks and closing safely…" => "正在取消任務並安全關閉…",
        "Input language" => "來源語言",
        "Output language" => "目標語言",
        "Custom language code" => "自訂語言代碼",
        "Swap languages" => "交換語言",
        "Enabled services" => "已啟用服務",
        "Manage services" => "管理服務",
        "All services" => "所有服務",
        "Advanced options" => "進階選項",
        "Performance and rate limits" => "效能與速率限制",
        "PDF processing" => "PDF 處理",
        "Page selection" => "頁面選擇",
        "All pages" => "全部頁面",
        "First page" => "第一頁",
        "First five pages" => "前五頁",
        "Custom pages" => "自訂頁碼",
        "Download PDF URL" => "從網址下載 PDF",
        "Download" => "下載",
        "Cancel download" => "取消下載",
        "Export batch ZIP" => "匯出批次 ZIP",
        "All PDF results" => "全部 PDF 結果",
        "Mono PDF results" => "譯文 PDF 結果",
        "Dual PDF results" => "雙語 PDF 結果",
        "Reload settings" => "重新載入設定",
        "Retry" => "重試",
        "Disabled services keep their saved settings" => "停用的服務仍會保留已保存的設定。",
        "Glossaries require an LLM service" => "使用術語表需要大型語言模型服務。",
        "Invalid page selection" => "頁碼格式無效，請檢查輸入。",
        "Page ranges help" => "可輸入 1,2,1-,-3,3-5，留空表示全部頁面。",
        "PDF download complete" => "PDF 下載完成。",
        "Pending edits requiring attention" => "需要修正的待保存設定",
        "Public PDF URL limits" => "僅接受公開的 HTTP 或 HTTPS 網址；下載大小與時間有限制。",
        "Remove" => "移除",
        "Save edits before reloading" => "請先保存修改，再重新載入設定。",
        "Settings reloaded" => "設定已重新載入。",
        "Source documents" => "原始文件",
        "Terminology limits help" => {
            "術語翻譯可使用獨立的速率與並行限制；未設定時沿用主要翻譯設定。"
        }
        "Terminology requires an LLM service" => "術語擷取需要大型語言模型服務。",
        "The selected service does not support this language" => "所選服務不支援此語言代碼。",
        "The selected service is disabled" => "所選服務目前已停用，請啟用或選擇其他服務。",
        "Zoom" => "縮放",
        "Not set" => "未設定",
        "Enabled" => "啟用",
        "Disabled" => "停用",
        "enabled" => "啟用",
        "disabled" => "停用",
        "high" => "高",
        "max" => "最高",
        "serif" => "襯線字型",
        "sans-serif" => "無襯線字型",
        "script" => "手寫字型",
        "no_watermark" => "無浮水印",
        "watermarked" => "含浮水印",
        "both" => "兩種都輸出",
        "Choose…" => "選擇…",
        "Invalid value" => "值無效，請檢查輸入。",
        "Enter a valid HTTP or HTTPS URL" => "請輸入有效的 HTTP 或 HTTPS 網址。",
        "Sensitive fields are disabled by settings" => "設定已禁止在介面中編輯敏感資料。",
        "JSON array of strings" => "請輸入字串的 JSON 陣列，例如 [\"a\", \"b\"]。",
        "Leave empty for automatic" => "留空表示未設定。",
        "Options depend on the selected service" => "可用語言依所選服務而異，請確認服務支援。",
        "Basic translation settings" => "基本翻譯設定",
        "Service and API" => "服務與 API",
        "Other advanced settings" => "其他進階設定",
        "Choose a PDF file" => "請選擇 PDF 檔案。",
        "Choose at least one PDF" => "請至少選擇一個 PDF。",
        "Choose at least one PDF output" => "請至少選擇一種 PDF 輸出。",
        "This filename cannot be represented in the settings file" => "此檔名無法寫入設定檔。",
        "configuration is not valid TOML" => "設定檔不是有效的 TOML。",
        "configuration file operation failed" => "無法讀取或寫入設定檔。",
        "The result is no longer available" => "結果檔案已不存在。",
        "Cannot open result" => "無法開啟結果。",
        _ => key,
    }
}

pub fn field_label(spec: &FieldSpec, chinese: bool) -> String {
    if !chinese {
        return spec.help.split('.').next().unwrap_or(&spec.flag).to_owned();
    }
    let p = spec.path.as_str();
    let label = match p {
        "basic.input_files" => "輸入 PDF 檔案",
        "basic.debug" => "偵錯模式",
        "basic.gui" => "啟用圖形介面",
        "basic.warmup" => "僅下載並驗證資源",
        "basic.generate_offline_assets" => "產生離線資源包的資料夾",
        "basic.restore_offline_assets" => "還原離線資源包",
        "basic.version" => "顯示版本",
        "gui_settings.share" => "啟用分享",
        "gui_settings.auth_file" => "驗證檔案",
        "gui_settings.welcome_page" => "歡迎頁面檔案",
        "gui_settings.enabled_services" => "啟用的服務",
        "gui_settings.disable_gui_sensitive_input" => "禁止介面編輯敏感資料",
        "gui_settings.disable_config_auto_save" => "停用自動保存設定",
        "gui_settings.server_port" => "網頁介面連接埠",
        "gui_settings.server_name" => "網頁介面監聽位址",
        "gui_settings.ui_lang" => "介面語言",
        "translation.min_text_length" => "翻譯文字最短長度",
        "translation.rpc_doclayout" => "版面分析 RPC 位址",
        "translation.lang_in" => "來源語言",
        "translation.lang_out" => "目標語言",
        "translation.output" => "輸出資料夾",
        "translation.qps" => "每秒請求數",
        "translation.ignore_cache" => "略過翻譯快取",
        "translation.custom_system_prompt" => "自訂系統提示詞",
        "translation.glossaries" => "術語表檔案",
        "translation.save_auto_extracted_glossary" => "保存自動擷取的術語表",
        "translation.pool_max_workers" => "同時請求數",
        "translation.term_qps" => "術語每秒請求數",
        "translation.term_pool_max_workers" => "術語同時請求數",
        "translation.no_auto_extract_glossary" => "停用自動擷取術語",
        "translation.primary_font_family" => "譯文字型類型",
        "pdf.pages" => "翻譯頁碼",
        "pdf.no_dual" => "不輸出雙語 PDF",
        "pdf.no_mono" => "不輸出譯文 PDF",
        "pdf.formular_font_pattern" => "公式字型比對規則",
        "pdf.formular_char_pattern" => "公式字元比對規則",
        "pdf.split_short_lines" => "拆分短行段落",
        "pdf.short_line_split_factor" => "短行拆分係數",
        "pdf.skip_clean" => "略過 PDF 清理",
        "pdf.dual_translate_first" => "雙語文件中譯文在前",
        "pdf.disable_rich_text_translate" => "停用富文字翻譯",
        "pdf.enhance_compatibility" => "增強相容性",
        "pdf.use_alternating_pages_dual" => "雙語文件交錯排列頁面",
        "pdf.watermark_output_mode" => "浮水印輸出模式",
        "pdf.max_pages_per_part" => "分割翻譯每部分頁數上限",
        "pdf.translate_table_text" => "翻譯表格文字（實驗性）",
        "pdf.skip_scanned_detection" => "略過掃描文件偵測",
        "pdf.ocr_workaround" => "使用 OCR 文字相容處理",
        "pdf.auto_enable_ocr_workaround" => "自動啟用 OCR 相容處理",
        "pdf.only_include_translated_page" => "只輸出指定翻譯頁面",
        "pdf.no_merge_alternating_line_numbers" => "處理交錯行號",
        "pdf.no_remove_non_formula_lines" => "保留段落中的非公式線條",
        "pdf.non_formula_line_iou_threshold" => "非公式線條 IoU 閾值",
        "pdf.figure_table_protection_threshold" => "圖表保護閾值",
        "pdf.skip_formula_offset_calculation" => "略過公式偏移計算",
        "config_file" => "設定檔路徑",
        "report_interval" => "進度回報間隔（秒）",
        _ => "",
    };
    if !label.is_empty() {
        return label.into();
    }
    if !p.contains('.') {
        return if let Some(name) = p.strip_prefix("term_") {
            format!("術語擷取使用 {name} 服務")
        } else {
            format!("使用 {p} 翻譯服務")
        };
    }
    let (section, key) = p.split_once('.').unwrap_or(("", p));
    let service = section
        .trim_start_matches("term_")
        .trim_end_matches("_detail");
    let term = if section.starts_with("term_") {
        "術語"
    } else {
        ""
    };
    let key = key.trim_start_matches("term_");
    let suffix = [
        "send_enable_thinking_param",
        "postprocess_command",
        "send_reasoning_effort",
        "send_temperature",
        "send_temprature",
        "enable_json_mode",
        "enable_thinking",
        "reasoning_effort",
        "thinking_mode",
        "api_version",
        "secret_id",
        "secret_key",
        "auth_key",
        "api_key",
        "apikey",
        "base_url",
        "ali_domains",
        "num_predict",
        "temperature",
        "endpoint",
        "command",
        "timeout",
        "model",
        "host",
        "path",
        "url",
    ]
    .iter()
    .find(|s| key == **s || key.ends_with(&format!("_{s}")))
    .copied()
    .unwrap_or("");
    let name = match suffix {
        "model" => "模型",
        "api_key" | "apikey" | "auth_key" | "secret_key" => "API 密鑰",
        "secret_id" => "密鑰 ID",
        "api_version" => "API 版本",
        "base_url" | "url" | "endpoint" => "API 網址",
        "host" => "主機位址",
        "timeout" => "逾時秒數",
        "temperature" => "溫度參數",
        "reasoning_effort" => "推理強度",
        "enable_json_mode" => "啟用 JSON 模式",
        "enable_thinking" => "啟用思考模式",
        "thinking_mode" => "思考模式",
        "send_enable_thinking_param" => "傳送思考模式參數",
        "send_temperature" | "send_temprature" => "傳送溫度參數",
        "send_reasoning_effort" => "傳送推理強度",
        "num_predict" => "預測 Token 上限",
        "ali_domains" => "翻譯領域",
        "command" => "翻譯指令",
        "postprocess_command" => "後處理指令",
        "path" => "程式路徑",
        _ => "設定",
    };
    format!("{term}{service} {name}")
}

pub fn field_help(spec: &FieldSpec, chinese: bool) -> String {
    if spec.path.ends_with("deepseek_model")
        || spec.path.ends_with("deepseek_thinking_mode")
        || spec.path.ends_with("deepseek_reasoning_effort")
    {
        let hint = if chinese {
            "思考模式與推理強度只適用於 deepseek-v4-* 模型；推理強度還需要啟用思考模式。"
        } else {
            "Thinking mode and reasoning effort apply only to deepseek-v4-* models; reasoning effort also requires thinking mode to be enabled."
        };
        return format!("{hint}\n--{}", spec.flag);
    }
    if !chinese {
        return format!("{}\n--{}", spec.help, spec.flag);
    }
    let hint = match spec.path.as_str() {
        "pdf.pages" => "可輸入 1,2,1-,-3,3-5；留空表示全部頁面。",
        "translation.pool_max_workers" => "留空時使用每秒請求數；設定時須至少為 1。",
        "translation.term_pool_max_workers" => "留空或輸入 0 時沿用主要翻譯的同時請求數。",
        "translation.term_qps" => "留空時沿用主要翻譯的每秒請求數。",
        "pdf.max_pages_per_part" => "留空或輸入 0 表示不分割；其他值須至少為 50。",
        "pdf.short_line_split_factor" => "啟用短行拆分時須至少為 0.1。",
        "pdf.non_formula_line_iou_threshold" | "pdf.figure_table_protection_threshold" => {
            "範圍為 0 到 1。"
        }
        "translation.glossaries" => "多個術語表檔案以逗號分隔。",
        "translation.custom_system_prompt" => "自訂傳送給翻譯模型的系統提示詞。",
        "gui_settings.enabled_services" => "以逗號分隔啟用的服務；留空表示全部。",
        p if p.ends_with("_api_key")
            || p.ends_with("_apikey")
            || p.ends_with("_auth_key")
            || p.ends_with("_secret_key") =>
        {
            "填入服務提供的密鑰；內容會遮罩顯示。"
        }
        p if p.ends_with("_model") => "填入服務支援的模型名稱。",
        p if p.ends_with("_base_url")
            || p.ends_with("_endpoint")
            || p.ends_with("_url")
            || p.ends_with("_host") =>
        {
            "填入服務的 HTTP 或 HTTPS 網址；本機服務可使用 localhost。"
        }
        p if p.ends_with("_timeout") => "設定等待服務回應的秒數。",
        p if p.ends_with("_temperature") => "設定模型輸出隨機性；是否支援由模型決定。",
        p if p.ends_with("_reasoning_effort") => "設定模型的推理強度；是否支援由模型決定。",
        p if p.ends_with("_enable_json_mode") => "要求服務以 JSON 模式回應；需由模型支援。",
        _ => "",
    };
    if hint.is_empty() {
        format!("{}。\n--{}", field_label(spec, true), spec.flag)
    } else {
        format!("{hint}\n--{}", spec.flag)
    }
}

pub fn friendly_error(message: &str, chinese: bool) -> String {
    if !chinese {
        return message.to_owned();
    }
    if let Some(path) = message.strip_prefix("invalid configuration field: ") {
        if let Some(spec) = pdf2zh_config::field_specs()
            .iter()
            .find(|s| s.path == path || s.flag == path)
        {
            return format!("設定無效：{}", field_label(spec, true));
        }
        return "設定欄位無效，請檢查輸入。".into();
    }
    if let Some(flag) = message.strip_prefix("Check the value for ") {
        if let Some(spec) = pdf2zh_config::field_specs().iter().find(|s| s.flag == flag) {
            return format!("請檢查：{}", field_label(spec, true));
        }
        return "請檢查輸入值。".into();
    }
    // Known errors have static keys; unknown errors remain useful to the user.
    match message {
        "Choose a PDF file" => ui("Choose a PDF file", true).into(),
        "Choose at least one PDF" => ui("Choose at least one PDF", true).into(),
        "Choose at least one PDF output" => ui("Choose at least one PDF output", true).into(),
        "This filename cannot be represented in the settings file" => ui(
            "This filename cannot be represented in the settings file",
            true,
        )
        .into(),
        "configuration is not valid TOML" => ui("configuration is not valid TOML", true).into(),
        "configuration file operation failed" => {
            ui("configuration file operation failed", true).into()
        }
        "The result is no longer available" => ui("The result is no longer available", true).into(),
        "Cannot open result" => ui("Cannot open result", true).into(),
        "The selected service is disabled" => "所選翻譯服務已停用，請啟用或選擇其他服務。".into(),
        "The selected terminology service is disabled" => {
            "所選術語服務已停用，請啟用或選擇其他服務。".into()
        }
        "The selected service does not support this language" => {
            "所選服務不支援此語言代碼，請更換語言或服務。".into()
        }
        "Keep at least one service enabled" => "請至少保留一個啟用的翻譯服務。".into(),
        "The batch has no complete PDF results" => "這個批次尚無完整的 PDF 結果可匯出。".into(),
        "The application is closing" => "程式正在關閉，請稍後再試。".into(),
        "The application is busy; try again shortly" => "程式忙碌中，請稍後再試。".into(),
        "Cannot start the application background thread" => {
            "無法啟動背景處理，請重新開啟程式。".into()
        }
        "Cannot start the application task runtime" => {
            "無法啟動背景執行環境，請重新開啟程式。".into()
        }
        "Cannot create the task manager" => "無法建立翻譯任務管理員，請重新開啟程式。".into(),
        "Translation cache is unavailable; continuing without it" => {
            "翻譯快取無法使用，將繼續執行但不使用快取。".into()
        }
        "Cannot prepare batch output folders" => "無法準備批次輸出資料夾。".into(),
        "PDFs with matching names use separate output subfolders" => {
            "同名 PDF 已分別儲存至獨立輸出子資料夾，避免互相覆寫。".into()
        }
        "Cannot save this result to the selected destination" => {
            "無法將結果保存至所選位置。".into()
        }
        "Cannot open this result with the default application" => {
            "無法使用預設應用程式開啟結果。".into()
        }
        "Choose an absolute destination" => "請選擇完整的儲存路徑。".into(),
        "Keep the result file type" => "請使用與結果相同的檔案類型。".into(),
        "Cannot resolve result" => "找不到結果檔案。".into(),
        "Choose a destination folder" => "請選擇儲存資料夾。".into(),
        "Cannot read result" => "無法讀取結果檔案。".into(),
        "Cannot create result temporary file" => "無法建立結果暫存檔。".into(),
        "Cannot copy result" => "無法複製結果檔案。".into(),
        "Cannot flush result" | "Cannot sync result" => "無法完成結果檔案寫入。".into(),
        "Cannot publish result atomically" => "無法完成儲存，原有檔案已保留。".into(),
        "The result path is invalid" => "結果檔案路徑無效。".into(),
        "Choose a PDF or glossary result" => "請選擇 PDF 或術語表結果。".into(),
        "Preview returned an invalid bitmap" => "無法顯示預覽影像。".into(),
        _ => message.to_owned(),
    }
}

/// Names come from the BabelDOC runtime bundled with the baseline executable.
/// Unknown future stages remain visible rather than being guessed.
pub fn stage(name: &str, chinese: bool) -> String {
    if !chinese {
        return name.to_owned();
    }
    match name {
        "Loading PDF engine" => "載入 PDF 引擎",
        "Starting PDF engine" => "啟動 PDF 引擎",
        "Save PDF" => "儲存 PDF",
        "Subset font" => "整理字型子集",
        "Parse PDF and Create Intermediate Representation" => "解析 PDF 文件",
        "DetectScannedFile" => "偵測掃描文件",
        "Parse Page Layout" => "分析頁面版面",
        "Parse Table" => "分析表格",
        "Parse Paragraphs" => "分析段落",
        "Parse Formulas and Styles" => "分析公式與樣式",
        "Automatic Term Extraction" => "自動擷取術語",
        "Translate Paragraphs" => "翻譯段落",
        "Typesetting" => "排版",
        "Add Fonts" => "加入字型",
        "Generate drawing instructions" => "產生 PDF 繪製指令",
        "Add Debug Information" => "加入偵錯資訊",
        "Remove Char Descent" => "處理字元下降部",
        _ => name,
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn dynamically_selected_ui_labels_are_translated() {
        for (key, expected) in [
            ("Original", "原文"),
            ("Translated", "譯文"),
            ("Bilingual", "雙語"),
            ("All pages", "全部頁面"),
            ("First page", "第一頁"),
            ("First five pages", "前五頁"),
            ("Basic translation settings", "基本翻譯設定"),
            ("PDF processing", "PDF 處理"),
            ("Other advanced settings", "其他進階設定"),
            ("All PDF results", "全部 PDF 結果"),
            ("Mono PDF results", "譯文 PDF 結果"),
            ("Dual PDF results", "雙語 PDF 結果"),
            (
                "Saved snapshot; newer edits remain unsaved",
                "已保存先前的設定；較新的修改尚未保存。",
            ),
        ] {
            assert_eq!(super::ui(key, true), expected);
            assert_eq!(super::ui(key, false), key);
        }
    }
    #[test]
    fn deepseek_model_help_explains_thinking_requirements() {
        for path in [
            "deepseek_detail.deepseek_model",
            "term_deepseek_detail.term_deepseek_model",
        ] {
            let spec = pdf2zh_config::field_specs()
                .iter()
                .find(|spec| spec.path == path)
                .unwrap();
            assert!(super::field_help(spec, true).contains("deepseek-v4-*"));
            assert!(super::field_help(spec, false).contains("deepseek-v4-*"));
        }
    }
    #[test]
    fn all_schema_fields_have_chinese_labels() {
        for spec in pdf2zh_config::field_specs() {
            let label = super::field_label(spec, true);
            assert!(
                label
                    .chars()
                    .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "{}",
                spec.path
            );
        }
    }
    #[test]
    fn stage_translates_known_names_and_preserves_unknown_names() {
        assert_eq!(super::stage("Loading PDF engine", true), "載入 PDF 引擎");
        assert_eq!(super::stage("Starting PDF engine", true), "啟動 PDF 引擎");
        assert_eq!(super::stage("Save PDF", true), "儲存 PDF");
        assert_eq!(super::stage("Subset font", true), "整理字型子集");
        assert_eq!(super::stage("Parse Page Layout", true), "分析頁面版面");
        assert_eq!(super::stage("custom stage", true), "custom stage");
        assert_eq!(super::stage("Typesetting", false), "Typesetting");
    }
}
