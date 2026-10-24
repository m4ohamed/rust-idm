//! محرك معالجة وتشكيل اللغة العربية الصافي (Pure Rust Arabic Reshaping & BiDi Engine)
//! 
//! يوفر هذا المحرك:
//! 1. تشكيل الحروف العربية (Contextual Shaping) وربطها (Initial, Medial, Final, Isolated).
//! 2. دمج تركيبات "اللام ألف" (Lam-Alef Ligatures: لا, لأ, لإ, لآ).
//! 3. عكس الترتيب ليتوافق مع محركات الرسم (BiDi Layout for Left-to-Right Renderers like egui).
//! 4. الحفاظ على اتجاه الأرقام والكلمات اللاتينية (مثل MB/s, GB, IDM) كما هي دون تشويه.
//! 5. عكس الأقواس في السياق العربي: `(` <-> `)`.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct ArabicForms {
    isolated: char,
    final_form: char,
    initial: char,
    medial: char,
    dual: bool, // true if connects to the next (left) letter
}

impl ArabicForms {
    const fn new(isolated: char, final_form: char, initial: char, medial: char, dual: bool) -> Self {
        Self {
            isolated,
            final_form,
            initial,
            medial,
            dual,
        }
    }
}

fn get_arabic_forms(c: char) -> Option<ArabicForms> {
    match c {
        // همزة
        '\u{0621}' => Some(ArabicForms::new('\u{FE80}', '\u{FE80}', '\u{FE80}', '\u{FE80}', false)),
        // ألف ممدودة (آ)
        '\u{0622}' => Some(ArabicForms::new('\u{FE81}', '\u{FE82}', '\u{FE81}', '\u{FE82}', false)),
        // ألف مهموزة فوق (أ)
        '\u{0623}' => Some(ArabicForms::new('\u{FE83}', '\u{FE84}', '\u{FE83}', '\u{FE84}', false)),
        // واو مهموزة (ؤ)
        '\u{0624}' => Some(ArabicForms::new('\u{FE85}', '\u{FE86}', '\u{FE85}', '\u{FE86}', false)),
        // ألف مهموزة تحت (إ)
        '\u{0625}' => Some(ArabicForms::new('\u{FE87}', '\u{FE88}', '\u{FE87}', '\u{FE88}', false)),
        // ياء مهموزة (ئ)
        '\u{0626}' => Some(ArabicForms::new('\u{FE89}', '\u{FE8A}', '\u{FE8B}', '\u{FE8C}', true)),
        // ألف (ا)
        '\u{0627}' => Some(ArabicForms::new('\u{FE8D}', '\u{FE8E}', '\u{FE8D}', '\u{FE8E}', false)),
        // باء (ب)
        '\u{0628}' => Some(ArabicForms::new('\u{FE8F}', '\u{FE90}', '\u{FE91}', '\u{FE92}', true)),
        // تاء مربوطة (ة)
        '\u{0629}' => Some(ArabicForms::new('\u{FE93}', '\u{FE94}', '\u{FE93}', '\u{FE94}', false)),
        // تاء (ت)
        '\u{062A}' => Some(ArabicForms::new('\u{FE95}', '\u{FE96}', '\u{FE97}', '\u{FE98}', true)),
        // ثاء (ث)
        '\u{062B}' => Some(ArabicForms::new('\u{FE99}', '\u{FE9A}', '\u{FE9B}', '\u{FE9C}', true)),
        // جيم (ج)
        '\u{062C}' => Some(ArabicForms::new('\u{FE9D}', '\u{FE9E}', '\u{FE9F}', '\u{FEA0}', true)),
        // حاء (ح)
        '\u{062D}' => Some(ArabicForms::new('\u{FEA1}', '\u{FEA2}', '\u{FEA3}', '\u{FEA4}', true)),
        // خاء (خ)
        '\u{062E}' => Some(ArabicForms::new('\u{FEA5}', '\u{FEA6}', '\u{FEA7}', '\u{FEA8}', true)),
        // دال (د)
        '\u{062F}' => Some(ArabicForms::new('\u{FEA9}', '\u{FEAA}', '\u{FEA9}', '\u{FEAA}', false)),
        // ذال (ذ)
        '\u{0630}' => Some(ArabicForms::new('\u{FEAB}', '\u{FEAC}', '\u{FEAB}', '\u{FEAC}', false)),
        // راء (ر)
        '\u{0631}' => Some(ArabicForms::new('\u{FEAD}', '\u{FEAE}', '\u{FEAD}', '\u{FEAE}', false)),
        // زاي (ز)
        '\u{0632}' => Some(ArabicForms::new('\u{FEAF}', '\u{FEB0}', '\u{FEAF}', '\u{FEB0}', false)),
        // سين (س)
        '\u{0633}' => Some(ArabicForms::new('\u{FEB1}', '\u{FEB2}', '\u{FEB3}', '\u{FEB4}', true)),
        // شين (ش)
        '\u{0634}' => Some(ArabicForms::new('\u{FEB5}', '\u{FEB6}', '\u{FEB7}', '\u{FEB8}', true)),
        // صاد (ص)
        '\u{0635}' => Some(ArabicForms::new('\u{FEB9}', '\u{FEBA}', '\u{FEBB}', '\u{FEBC}', true)),
        // ضاد (ض)
        '\u{0636}' => Some(ArabicForms::new('\u{FEBD}', '\u{FEBE}', '\u{FEBF}', '\u{FEC0}', true)),
        // طاء (ط)
        '\u{0637}' => Some(ArabicForms::new('\u{FEC1}', '\u{FEC2}', '\u{FEC3}', '\u{FEC4}', true)),
        // ظاء (ظ)
        '\u{0638}' => Some(ArabicForms::new('\u{FEC5}', '\u{FEC6}', '\u{FEC7}', '\u{FEC8}', true)),
        // عين (ع)
        '\u{0639}' => Some(ArabicForms::new('\u{FEC9}', '\u{FECA}', '\u{FECB}', '\u{FECC}', true)),
        // غين (غ)
        '\u{063A}' => Some(ArabicForms::new('\u{FECD}', '\u{FECE}', '\u{FECF}', '\u{FED0}', true)),
        // تطويل (ـ)
        '\u{0640}' => Some(ArabicForms::new('\u{0640}', '\u{0640}', '\u{0640}', '\u{0640}', true)),
        // فاء (ف)
        '\u{0641}' => Some(ArabicForms::new('\u{FED1}', '\u{FED2}', '\u{FED3}', '\u{FED4}', true)),
        // قاف (ق)
        '\u{0642}' => Some(ArabicForms::new('\u{FED5}', '\u{FED6}', '\u{FED7}', '\u{FED8}', true)),
        // كاف (ك)
        '\u{0643}' => Some(ArabicForms::new('\u{FED9}', '\u{FEDA}', '\u{FEDB}', '\u{FEDC}', true)),
        // لام (ل)
        '\u{0644}' => Some(ArabicForms::new('\u{FEDD}', '\u{FEDE}', '\u{FEDF}', '\u{FEE0}', true)),
        // ميم (م)
        '\u{0645}' => Some(ArabicForms::new('\u{FEE1}', '\u{FEE2}', '\u{FEE3}', '\u{FEE4}', true)),
        // نون (ن)
        '\u{0646}' => Some(ArabicForms::new('\u{FEE5}', '\u{FEE6}', '\u{FEE7}', '\u{FEE8}', true)),
        // هاء (ه)
        '\u{0647}' => Some(ArabicForms::new('\u{FEE9}', '\u{FEEA}', '\u{FEEB}', '\u{FEEC}', true)),
        // واو (و)
        '\u{0648}' => Some(ArabicForms::new('\u{FEED}', '\u{FEEE}', '\u{FEED}', '\u{FEEE}', false)),
        // ألف مقصورة (ى)
        '\u{0649}' => Some(ArabicForms::new('\u{FEEF}', '\u{FEF0}', '\u{FEEF}', '\u{FEF0}', false)),
        // ياء (ي)
        '\u{064A}' => Some(ArabicForms::new('\u{FEF1}', '\u{FEF2}', '\u{FEF3}', '\u{FEF4}', true)),
        _ => None,
    }
}

fn is_tashkeel(c: char) -> bool {
    matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}')
}

pub fn has_arabic(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '\u{0600}'..='\u{06FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'))
}

/// دمج اللام ألف (Lam-Alef Ligature)
fn get_lam_alef(alef: char, prev_connects: bool) -> Option<char> {
    match alef {
        '\u{0622}' => Some(if prev_connects { '\u{FEF6}' } else { '\u{FEF5}' }), // لآ
        '\u{0623}' => Some(if prev_connects { '\u{FEF8}' } else { '\u{FEF7}' }), // لأ
        '\u{0625}' => Some(if prev_connects { '\u{FEFA}' } else { '\u{FEF9}' }), // لإ
        '\u{0627}' => Some(if prev_connects { '\u{FEFC}' } else { '\u{FEFB}' }), // لا
        _ => None,
    }
}

/// تشكيل كلمة عربية واحدة وربط حروفها بدقة
fn shape_arabic_word(word: &str) -> String {
    let raw_chars: Vec<char> = word.chars().filter(|c| !is_tashkeel(*c)).collect();
    if raw_chars.is_empty() {
        return String::new();
    }

    let mut shaped = Vec::with_capacity(raw_chars.len());
    let mut i = 0;
    let mut prev_connected = false;

    while i < raw_chars.len() {
        let cur = raw_chars[i];

        // التحقق من حالة "اللام ألف" المركبة
        if cur == '\u{0644}' && i + 1 < raw_chars.len() {
            let next = raw_chars[i + 1];
            if let Some(ligature) = get_lam_alef(next, prev_connected) {
                shaped.push(ligature);
                prev_connected = false; // اللام ألف لا تتصل بما بعدها أبداً
                i += 2;
                continue;
            }
        }

        if let Some(forms) = get_arabic_forms(cur) {
            let next_is_joining = if i + 1 < raw_chars.len() {
                get_arabic_forms(raw_chars[i + 1]).is_some()
            } else {
                false
            };

            let ch = match (prev_connected, forms.dual && next_is_joining) {
                (false, false) => forms.isolated,
                (true, false) => forms.final_form,
                (false, true) => forms.initial,
                (true, true) => forms.medial,
            };

            shaped.push(ch);
            prev_connected = forms.dual;
        } else {
            shaped.push(cur);
            prev_connected = false;
        }

        i += 1;
    }

    shaped.into_iter().collect()
}

/// عكس الأقواس لتتناسب مع السياق العربي من اليمين لليسار
fn mirror_bracket(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        _ => c,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum TokenKind {
    Arabic,
    NonArabic,
    Whitespace,
}

fn char_kind(c: char) -> TokenKind {
    if c.is_whitespace() {
        TokenKind::Whitespace
    } else if matches!(c, '\u{0600}'..='\u{06FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}') {
        TokenKind::Arabic
    } else {
        TokenKind::NonArabic
    }
}

/// تشكيل وعكس النصوص العربية لعرضها بانسجام ووضوح تام داخل محركات LTR (مثل egui)
pub fn shape_arabic(text: &str) -> String {
    if !has_arabic(text) {
        return text.to_string();
    }

    // تقسيم النص إلى أسطر للحفاظ على سلامة فواصل الأسطر
    let lines: Vec<&str> = text.split('\n').collect();
    let mut shaped_lines = Vec::with_capacity(lines.len());

    for line in lines {
        if !has_arabic(line) {
            shaped_lines.push(line.to_string());
            continue;
        }

        // تقسيم السطر إلى مقاطع (Tokens) عربية وغير عربية ومسافات
        let mut tokens: Vec<(TokenKind, String)> = Vec::new();
        let mut current_kind = None;
        let mut current_buf = String::new();

        for c in line.chars() {
            let k = char_kind(c);
            if current_kind == Some(k) {
                current_buf.push(c);
            } else {
                if let Some(kind) = current_kind {
                    tokens.push((kind, current_buf));
                }
                current_kind = Some(k);
                current_buf = String::from(c);
            }
        }
        if let Some(kind) = current_kind {
            tokens.push((kind, current_buf));
        }

        // معالجة كل مقطع: المقاطع العربية تُشكّل وتُعكس حروفها
        let mut processed_tokens: Vec<String> = Vec::with_capacity(tokens.len());
        for (kind, tok) in tokens {
            match kind {
                TokenKind::Arabic => {
                    let shaped = shape_arabic_word(&tok);
                    // عكس حروف الكلمة العربية لتُرسم سليمة RTL في محرك LTR
                    let reversed: String = shaped.chars().rev().collect();
                    processed_tokens.push(reversed);
                }
                TokenKind::NonArabic => {
                    // عكس الأقواس فقط في السياق العربي
                    let mirrored: String = tok.chars().map(mirror_bracket).collect();
                    processed_tokens.push(mirrored);
                }
                TokenKind::Whitespace => {
                    processed_tokens.push(tok);
                }
            }
        }

        // عكس ترتيب المقاطع نفسها في السطر العربي لتظهر الجملة من اليمين لليسار
        processed_tokens.reverse();
        shaped_lines.push(processed_tokens.join(""));
    }

    shaped_lines.join("\n")
}

/// اختصار سريع ومباشر لتشكيل النصوص العربية
#[inline]
pub fn ar(text: &str) -> String {
    shape_arabic(text)
}
