//! UI localization. `Strings` is the complete message catalog — every
//! language is one `Strings` literal and the compiler refuses a partial
//! table, so adding a language is: add the `Lang` variant, copy `EN`,
//! translate each field.
//!
//! Arabic-table entries are stored in **logical order** (normal Unicode
//! text). egui does no complex-text shaping, so [`view`] reshapes to
//! presentation forms and reorders RTL runs for display — apply it to any
//! *non-editable* string that may contain Arabic (UI chrome and user
//! content like file names). Editable buffers must NOT pass through it:
//! it reorders characters, which would desync caret math.

use std::borrow::Cow;

/// UI language. `code()` is the persisted prefs value; `native_name()` is
/// the picker's label (each language names itself).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ar,
}

pub const ALL: &[Lang] = &[Lang::En, Lang::Ar];

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ar => "ar",
        }
    }

    /// Unknown/missing codes fall back to English.
    pub fn from_code(code: &str) -> Self {
        match code {
            "ar" => Lang::Ar,
            _ => Lang::En,
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ar => "العربية",
        }
    }

    /// Whether this language's chrome should mirror right-to-left.
    pub fn rtl(self) -> bool {
        matches!(self, Lang::Ar)
    }
}

/// The UI message catalog. Keep fields in call-site order of appearance:
/// top bar, sidebar, status bar, preview, then each modal.
pub struct Strings {
    // top bar
    pub saved_locally: &'static str,
    pub unsaved_changes: &'static str,
    pub untitled: &'static str,
    pub recompile: &'static str,
    pub compile_options: &'static str,
    pub engine_auto: &'static str,
    pub auto_compile: &'static str,
    pub user_tools: &'static str,
    pub logs: &'static str,
    pub outline: &'static str,
    pub history: &'static str,
    pub stats: &'static str,
    pub stats_hint: &'static str,
    pub ai: &'static str,
    pub ai_hint: &'static str,
    pub cite: &'static str,
    pub help: &'static str,
    pub shortcuts_hint: &'static str,
    pub language: &'static str,
    // sidebar
    pub new_file: &'static str,
    pub new_folder: &'static str,
    pub refresh_tree: &'static str,
    pub open_file: &'static str,
    pub new_from_template: &'static str,
    pub empty_folder: &'static str,
    pub main_doc: &'static str,
    pub file_outline: &'static str,
    pub unfold_all: &'static str,
    pub fold_all: &'static str,
    pub no_headings: &'static str,
    // status bar
    pub tex_badge: &'static str,
    pub issues_none: &'static str,
    pub issues_many: &'static str, // "{n}" substituted
    pub folded_warn: &'static str,
    pub engine_label: &'static str, // "{name}" substituted
    pub engine_hint: &'static str,
    pub root_label: &'static str, // "{name}" substituted
    pub root_hint: &'static str,
    pub line_col: &'static str, // "{l}", "{c}" substituted
    // preview toolbar
    pub print_hint: &'static str,
    pub zoom_fit_width: &'static str,
    pub zoom_fit_page: &'static str,
    pub zoom_custom: &'static str,
    pub page_continuous: &'static str,
    pub page_single: &'static str,
    pub page_two_up: &'static str,
    pub page_two_up_cover: &'static str,
    // logs
    pub logs_title: &'static str,
    pub logs_empty: &'static str,
    pub engine_files: &'static str,
    // modals
    pub new_file_title: &'static str,
    pub new_folder_title: &'static str,
    pub in_dir: &'static str, // "{dir}" substituted
    pub folder_name_hint: &'static str,
    pub file_name_hint: &'static str,
    pub create: &'static str,
    pub cancel: &'static str,
    pub tpl_title: &'static str,
    pub tpl_none: &'static str,
    pub hist_title: &'static str,
    pub hist_label_hint: &'static str,
    pub snapshot_now: &'static str,
    pub no_snapshots: &'static str,
    pub restore: &'static str,
    pub restore_confirm: &'static str,
    pub close: &'static str,
    pub stats_title: &'static str,
    pub shortcuts_title: &'static str,
    pub ai_title: &'static str,
    // stats labels
    pub st_words: &'static str,
    pub st_chars: &'static str,
    pub st_chars_ns: &'static str,
    pub st_lines: &'static str,
    pub st_commands: &'static str,
    pub st_math_inline: &'static str,
    pub st_math_display: &'static str,
    pub st_envs: &'static str,
    pub st_sections: &'static str,
    pub st_labels: &'static str,
    pub st_cites: &'static str,
    // shortcut groups (SHORTCUTS action rows stay English — they
    // document physical keys; group headers localize)
    pub sc_file: &'static str,
    pub sc_editing: &'static str,
    pub sc_navigation: &'static str,
    pub sc_help: &'static str,
    // cite dialog
    pub cite_title: &'static str,
    pub cite_filter_hint: &'static str,
    pub cite_entries: &'static str, // "{n}" substituted
    pub cite_empty: &'static str,
    pub cite_no_bib: &'static str,
    pub cite_no_bib_wasm: &'static str,
    // misc hovers/menus
    pub theme_to_light: &'static str,
    pub theme_to_dark: &'static str,
    pub fold_open: &'static str,   // "{n}" substituted
    pub fold_hidden: &'static str, // "{n}" substituted
    pub spell_on: &'static str,
    pub spell_off: &'static str,
    pub spell_none: &'static str,
    // ai modal
    pub ai_endpoint: &'static str,
    pub ai_model: &'static str,
    pub ai_token: &'static str,
    pub ai_token_hint: &'static str,
    pub ai_test: &'static str,
    pub ai_question: &'static str,
    pub ai_ask: &'static str,
}

/// Substitute `{name}`-style placeholders in a catalog format string.
pub fn fmt(fmt: &'static str, pairs: &[(&str, &str)]) -> String {
    let mut out = fmt.to_string();
    for (k, v) in pairs {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

const EN: Strings = Strings {
    saved_locally: "saved locally",
    unsaved_changes: "unsaved changes",
    untitled: "untitled",
    recompile: "Recompile",
    compile_options: "compile options",
    engine_auto: "auto",
    auto_compile: "auto-compile after 5 s idle",
    user_tools: "user tools",
    logs: "Logs",
    outline: "Outline",
    history: "History",
    stats: "Stats",
    stats_hint: "word/character counts",
    ai: "AI",
    ai_hint: "AI assist — bring your own endpoint",
    cite: "cite",
    help: "help",
    shortcuts_hint: "keyboard shortcuts (F1)",
    language: "Language",
    new_file: "new file",
    new_folder: "new folder",
    refresh_tree: "refresh tree",
    open_file: "open file…",
    new_from_template: "new from template…",
    empty_folder: "(empty folder)",
    main_doc: "main document",
    file_outline: "FILE OUTLINE",
    unfold_all: "unfold",
    fold_all: "fold",
    no_headings: "(no headings)",
    tex_badge: "TeX",
    issues_none: "no issues",
    issues_many: "⚠ {n} issues",
    folded_warn: "folded — unfold to edit",
    engine_label: "engine: {name}",
    engine_hint: "% !TeX program — the file picks its own engine",
    root_label: "root→{name}",
    root_hint: "% !TeX root — compiles the master file",
    line_col: "Ln {l}, Col {c}",
    print_hint: "print the PDF",
    zoom_fit_width: "fit width",
    zoom_fit_page: "fit page",
    zoom_custom: "custom",
    page_continuous: "continuous",
    page_single: "single page",
    page_two_up: "two pages",
    page_two_up_cover: "two pages + cover",
    logs_title: "BUILD LOG",
    logs_empty: "(no output yet)",
    engine_files: "{n} files",
    new_file_title: "New file",
    new_folder_title: "New folder",
    in_dir: "in {dir}",
    folder_name_hint: "folder name",
    file_name_hint: "name.tex",
    create: "Create",
    cancel: "Cancel",
    tpl_title: "New from template",
    tpl_none: "no templates found",
    hist_title: "Local history",
    hist_label_hint: "label (optional)",
    snapshot_now: "snapshot now",
    no_snapshots: "no snapshots yet",
    restore: "restore",
    restore_confirm: "click again to overwrite the working tree",
    close: "Close",
    stats_title: "Statistics",
    shortcuts_title: "Keyboard shortcuts",
    ai_title: "AI assist",
    st_words: "words",
    st_chars: "characters",
    st_chars_ns: "chars (no spaces)",
    st_lines: "lines",
    st_commands: "commands",
    st_math_inline: "inline math",
    st_math_display: "display math",
    st_envs: "environments",
    st_sections: "sections",
    st_labels: "labels",
    st_cites: "citations",
    sc_file: "file",
    sc_editing: "editing",
    sc_navigation: "navigation",
    sc_help: "help",
    cite_title: "cite",
    cite_filter_hint: "filter by key, author, title…",
    cite_entries: "{n} entries",
    cite_empty: "(no matches)",
    cite_no_bib: "no .bib files found — save a refs.bib next to the document",
    cite_no_bib_wasm: "bibliography scanning needs a local file",
    theme_to_light: "switch to light theme",
    theme_to_dark: "switch to dark theme",
    fold_open: "fold {n} lines",
    fold_hidden: "unfold — {n} hidden lines",
    spell_on: "spell check: on",
    spell_off: "spell check: off",
    spell_none: "(no suggestions)",
    ai_endpoint: "endpoint",
    ai_model: "model",
    ai_token: "token",
    ai_token_hint: "optional — not saved",
    ai_test: "test connection",
    ai_question: "question (current document is sent as context)",
    ai_ask: "ask",
};

const AR: Strings = Strings {
    saved_locally: "محفوظ محلياً",
    unsaved_changes: "تغييرات غير محفوظة",
    untitled: "بدون عنوان",
    recompile: "إعادة التنضيد",
    compile_options: "خيارات التنضيد",
    engine_auto: "تلقائي",
    auto_compile: "تنضيد تلقائي بعد ٥ ثوانٍ من التوقف",
    user_tools: "أدوات المستخدم",
    logs: "السجل",
    outline: "المخطط",
    history: "المحفوظات",
    stats: "الإحصاءات",
    stats_hint: "عدّ الكلمات والأحرف",
    ai: "ذكاء",
    ai_hint: "مساعدة ذكية — استخدم خادمك الخاص",
    cite: "استشهاد",
    help: "مساعدة",
    shortcuts_hint: "اختصارات لوحة المفاتيح (F1)",
    language: "اللغة",
    new_file: "ملف جديد",
    new_folder: "مجلد جديد",
    refresh_tree: "تحديث الشجرة",
    open_file: "فتح ملف…",
    new_from_template: "جديد من قالب…",
    empty_folder: "(مجلد فارغ)",
    main_doc: "المستند الرئيسي",
    file_outline: "مخطط الملف",
    unfold_all: "توسيع",
    fold_all: "طي",
    no_headings: "(لا عناوين)",
    tex_badge: "TeX",
    issues_none: "لا مشكلات",
    issues_many: "⚠ {n} مشكلات",
    folded_warn: "مطوي — وسّع للتحرير",
    engine_label: "المحرك: {name}",
    engine_hint: "% !TeX program — يختار الملف محركه بنفسه",
    root_label: "{name}←الجذر",
    root_hint: "% !TeX root — ينضد الملف الرئيسي",
    line_col: "سطر {l}، عمود {c}",
    print_hint: "اطبع ملف PDF",
    zoom_fit_width: "ملاءمة العرض",
    zoom_fit_page: "ملاءمة الصفحة",
    zoom_custom: "مخصص",
    page_continuous: "مستمر",
    page_single: "صفحة واحدة",
    page_two_up: "صفحتان",
    page_two_up_cover: "صفحتان + غلاف",
    logs_title: "سجل البناء",
    logs_empty: "(لا مخرجات بعد)",
    engine_files: "{n} ملفات",
    new_file_title: "ملف جديد",
    new_folder_title: "مجلد جديد",
    in_dir: "في {dir}",
    folder_name_hint: "اسم المجلد",
    file_name_hint: "اسم.tex",
    create: "إنشاء",
    cancel: "إلغاء",
    tpl_title: "جديد من قالب",
    tpl_none: "لا قوالب",
    hist_title: "السجل المحلي",
    hist_label_hint: "وصف (اختياري)",
    snapshot_now: "احفظ لقطة الآن",
    no_snapshots: "لا لقطات بعد",
    restore: "استعادة",
    restore_confirm: "انقر مجدداً للكتابة فوق شجرة العمل",
    close: "إغلاق",
    stats_title: "الإحصاءات",
    shortcuts_title: "اختصارات لوحة المفاتيح",
    ai_title: "مساعدة ذكية",
    st_words: "الكلمات",
    st_chars: "الأحرف",
    st_chars_ns: "أحرف (بدون فراغات)",
    st_lines: "الأسطر",
    st_commands: "الأوامر",
    st_math_inline: "رياضيات سطرية",
    st_math_display: "رياضيات معروضة",
    st_envs: "البيئات",
    st_sections: "المقاطع",
    st_labels: "الوسوم",
    st_cites: "الاستشهادات",
    sc_file: "ملف",
    sc_editing: "تحرير",
    sc_navigation: "تنقل",
    sc_help: "مساعدة",
    cite_title: "استشهاد",
    cite_filter_hint: "رشّح بالمفتاح أو المؤلف أو العنوان…",
    cite_entries: "{n} مدخلاً",
    cite_empty: "(لا نتائج مطابقة)",
    cite_no_bib: "لا ملفات bib. — احفظ refs.bib بجانب المستند",
    cite_no_bib_wasm: "فحص المراجع يحتاج ملفاً محلياً",
    theme_to_light: "التبديل إلى السمة الفاتحة",
    theme_to_dark: "التبديل إلى السمة الداكنة",
    fold_open: "اطوِ {n} أسطر",
    fold_hidden: "وسّع — {n} أسطر مخفية",
    spell_on: "التدقيق الإملائي: مفعّل",
    spell_off: "التدقيق الإملائي: معطّل",
    spell_none: "(لا اقتراحات)",
    ai_endpoint: "الخادم",
    ai_model: "النموذج",
    ai_token: "الرمز",
    ai_token_hint: "اختياري — لا يُحفظ",
    ai_test: "اختبر الاتصال",
    ai_question: "سؤال (يُرسل المستند الحالي كسياق)",
    ai_ask: "اسأل",
};

/// The catalog for `lang`. English is the default for unknown languages.
pub fn strings(lang: Lang) -> &'static Strings {
    match lang {
        Lang::En => &EN,
        Lang::Ar => &AR,
    }
}

/// Does `s` contain characters that need Arabic-script shaping?
fn needs_shape(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF))
}

/// Prepare `s` for display in egui's LTR-only, unshaped text pipeline:
/// Arabic-script characters are mapped to their contextual presentation
/// forms and bidi runs are reordered into visual order. Zero-cost borrow
/// for strings without Arabic. Never apply to an editable buffer.
pub fn view(s: &str) -> Cow<'_, str> {
    if !needs_shape(s) {
        return Cow::Borrowed(s);
    }
    let reshaped = arabic_reshaper::arabic_reshape(s);
    let bidi = unicode_bidi::BidiInfo::new(&reshaped, None);
    let mut out = String::with_capacity(reshaped.len());
    for para in &bidi.paragraphs {
        out.push_str(&bidi.reorder_line(para, para.range.clone()));
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_code_falls_back_to_english() {
        assert_eq!(Lang::from_code("en"), Lang::En);
        assert_eq!(Lang::from_code("ar"), Lang::Ar);
        assert_eq!(Lang::from_code("xx"), Lang::En);
        assert_eq!(Lang::from_code(""), Lang::En);
    }

    #[test]
    fn every_language_has_code_and_name() {
        for l in ALL {
            assert!(!l.code().is_empty());
            assert!(!l.native_name().is_empty());
            // round-trip
            assert_eq!(Lang::from_code(l.code()), *l);
        }
    }

    #[test]
    fn view_passthrough_for_latin() {
        assert!(matches!(view("hello"), Cow::Borrowed(_)));
        assert!(matches!(view("Recompile 123"), Cow::Borrowed(_)));
    }

    #[test]
    fn view_shapes_arabic() {
        let shaped = view("مرحبا");
        assert!(matches!(shaped, Cow::Owned(_)));
        assert_ne!(shaped.as_ref(), "مرحبا"); // presentation forms differ
                                              // م ر ح ب ا — reshaping preserves the character count (1:1 forms).
        assert_eq!(shaped.chars().count(), 5);
    }

    #[test]
    fn view_reorders_rtl_runs() {
        // "a ب b" visually shows the RTL run reversed relative to logical.
        let shaped = view("a بب b");
        // Logical order is a, ب, ب, b; visual order puts the بب run's
        // mirrored forms between the Latin letters.
        assert!(shaped.contains('a'));
        assert!(shaped.contains('b'));
    }

    #[test]
    fn fmt_substitutes_placeholders() {
        assert_eq!(fmt("Ln {l}, Col {c}", &[("l", "3"), ("c", "7")]), "Ln 3, Col 7");
    }

    #[test]
    fn arabic_rtl_and_english_ltr() {
        assert!(Lang::Ar.rtl());
        assert!(!Lang::En.rtl());
    }
}
