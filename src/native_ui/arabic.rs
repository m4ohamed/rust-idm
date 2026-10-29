<<<<<<< HEAD
//! محرك معالجة وتشكيل اللغة العربية وخوارزمية ثنائية الاتجاه (Pure Rust Arabic Reshaping & UAX #9 BiDi Engine v2.3)
//!
//! يوفر هذا المحرك حلاً جذرياً ومتكاملاً لعرض اللغة العربية في `egui` (الذي يعتمد على رسم LTR):
//! 1. منع التشكيل المزدوج (`is_already_shaped`) لضمان عدم انقلاب النصوص التي سبق تشكيلها في أي مكان.
//! 2. دعم كامل لحروف العربية القياسية والممتدة والفارسية والأردية (پ، چ، ژ، گ، ک، ی، ه‍، ڤ).
//! 3. تشكيل الحروف العربية (Contextual Shaping) وربطها (Initial, Medial, Final, Isolated) ودمج تركيبات اللام-ألف (لا، لأ، لإ، لآ).
//! 4. خوارزمية BiDi ذكية تدمج العبارات اللاتينية المتجاورة (`LTR Runs` مثل `(Status Bar)`, `(Bounded BFS Site Grabber)`, `(SHA-256 / MD5 / CRC32)`, `part_[01-10].zip`)
//!    وتحافظ على ترتيب كلماتها وأقواسها من اليسار إلى اليمين دون عكس أو تشويه.
=======
//! محرك معالجة وتشكيل اللغة العربية الصافي (Pure Rust Arabic Reshaping & BiDi Engine)
//!
//! يوفر هذا المحرك:
//! 1. تشكيل الحروف العربية (Contextual Shaping) وربطها (Initial, Medial, Final, Isolated).
//! 2. دمج تركيبات "اللام ألف" (Lam-Alef Ligatures: لا, لأ, لإ, لآ).
//! 3. عكس الترتيب ليتوافق مع محركات الرسم (BiDi Layout for Left-to-Right Renderers like egui).
//! 4. الحفاظ على اتجاه الأرقام والكلمات اللاتينية (مثل MB/s, GB, IDM) كما هي دون تشويه.
//! 5. عكس الأقواس في السياق العربي: `(` <-> `)`.
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct ArabicForms {
    isolated: char,
    final_form: char,
    initial: char,
    medial: char,
    dual: bool, // true if connects to the next (left) letter
}

impl ArabicForms {
    const fn new(
        isolated: char,
        final_form: char,
        initial: char,
        medial: char,
        dual: bool,
    ) -> Self {
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
        // همزة (ء)
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
        // حروف عربية ممتدة وفارسية وأردية (پ، چ، ژ، گ، ک، ی، ڤ)
        '\u{067E}' => Some(ArabicForms::new('\u{FB56}', '\u{FB57}', '\u{FB58}', '\u{FB59}', true)),
        '\u{0686}' => Some(ArabicForms::new('\u{FB7A}', '\u{FB7B}', '\u{FB7C}', '\u{FB7D}', true)),
        '\u{0698}' => Some(ArabicForms::new('\u{FB8A}', '\u{FB8B}', '\u{FB8A}', '\u{FB8B}', false)),
        '\u{06AF}' => Some(ArabicForms::new('\u{FB92}', '\u{FB93}', '\u{FB94}', '\u{FB95}', true)),
        '\u{06A9}' => Some(ArabicForms::new('\u{FB8E}', '\u{FB8F}', '\u{FB90}', '\u{FB91}', true)),
        '\u{06CC}' => Some(ArabicForms::new('\u{FBFD}', '\u{FBFE}', '\u{FBFF}', '\u{FBFD}', true)),
        '\u{06A4}' => Some(ArabicForms::new('\u{FB6A}', '\u{FB6B}', '\u{FB6C}', '\u{FB6D}', true)),
        _ => None,
    }
}

/// التحقق مما إذا كان الحرف من حركات التشكيل أو التطويل (التي تُدمج أو تُتجاوز أثناء الربط)
#[inline]
fn is_tashkeel(c: char) -> bool {
    matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}')
}

/// التحقق مما إذا كان النص يحتوي على أي حرف عربي
#[inline]
pub fn has_arabic(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(
            c,
            '\u{0600}'..='\u{06FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'
        )
    })
}

/// التحقق مما إذا كان النص قد سبق تشكيله إلى محارف العرض (`Presentation Forms-B`) لمنع العكس المزدوج
#[inline]
pub fn is_already_shaped(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'))
}

/// دمج اللام مع الألف (لا، لأ، لإ، لآ) في محرف عرض واحد
fn get_lam_alef(next: char, prev_connected: bool) -> Option<char> {
    match next {
        '\u{0622}' => Some(if prev_connected { '\u{FEF6}' } else { '\u{FEF5}' }),
        '\u{0623}' => Some(if prev_connected { '\u{FEF8}' } else { '\u{FEF7}' }),
        '\u{0625}' => Some(if prev_connected { '\u{FEFA}' } else { '\u{FEF9}' }),
        '\u{0627}' => Some(if prev_connected { '\u{FEFC}' } else { '\u{FEFB}' }),
        _ => None,
    }
}

/// تشكيل كلمة عربية واحدة وربط حروفها وعكسها للعرض في محرك LTR
fn shape_arabic_word_visual(word: &str) -> String {
    let raw_chars: Vec<char> = word.chars().filter(|c| !is_tashkeel(*c)).collect();
    if raw_chars.is_empty() {
        return String::new();
    }

    let mut shaped = Vec::with_capacity(raw_chars.len());
    let mut i = 0;
    let mut prev_connected = false;

    while i < raw_chars.len() {
        let cur = raw_chars[i];

        if cur == '\u{0644}' && i + 1 < raw_chars.len() {
            let next = raw_chars[i + 1];
            if let Some(ligature) = get_lam_alef(next, prev_connected) {
                shaped.push(ligature);
                prev_connected = false;
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

    shaped.into_iter().rev().collect()
}

/// عكس الأقواس في السياق العربي فقط
fn mirror_bracket(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        _ => c,
    }
}

<<<<<<< HEAD
#[inline]
fn is_arabic_char(c: char) -> bool {
    matches!(
        c,
        '\u{0600}'..='\u{06FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}'
    )
=======
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    Arabic,
    NonArabic,
    Whitespace,
>>>>>>> efc40c730b5c3e9a4001ccff0b896d7f2f3ef344
}

#[inline]
fn is_ltr_alnum(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

/// تشكيل رمز عربي واحد (قد يتضمن علامات ترقيم ملتصقة مثل `:النشطة` أو `(يدوي)`)
fn shape_rtl_token(tok: &str) -> String {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum SubKind {
        Arabic,
        Latin,
        Punct,
    }

    let mut subs: Vec<(SubKind, String)> = Vec::new();
    let mut cur_kind: Option<SubKind> = None;
    let mut buf = String::new();

    for c in tok.chars() {
        let k = if is_arabic_char(c) {
            SubKind::Arabic
        } else if is_ltr_alnum(c) {
            SubKind::Latin
        } else {
            SubKind::Punct
        };

        if cur_kind == Some(k) {
            buf.push(c);
        } else {
            if let Some(prev) = cur_kind {
                subs.push((prev, std::mem::take(&mut buf)));
            }
            cur_kind = Some(k);
            buf.push(c);
        }
    }
    if let Some(prev) = cur_kind {
        subs.push((prev, buf));
    }

    let mut out = String::with_capacity(tok.len() * 2);
    for (k, s) in subs.into_iter().rev() {
        match k {
            SubKind::Arabic => out.push_str(&shape_arabic_word_visual(&s)),
            SubKind::Latin => out.push_str(&s),
            SubKind::Punct => {
                for c in s.chars() {
                    out.push(mirror_bracket(c));
                }
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BidiClass {
    Ws,
    Rtl,
    Ltr,
    Neutral,
}

fn is_ltr_connector(class: &BidiClass, val: &str) -> bool {
    match class {
        BidiClass::Ws => true,
        BidiClass::Neutral => val.chars().all(|c| matches!(c, '/' | '-' | '&' | '+' | ':' | ',')),
        _ => false,
    }
}

/// تشكيل سطر واحد باستخدام خوارزمية دمج المقاطع اللاتينية (`LTR Runs`) وترتيب السياق العربي (`RTL`)
fn shape_bidi_line(line: &str) -> String {
    if !has_arabic(line) || is_already_shaped(line) {
        return line.to_string();
    }

    // 1. تقسيم السطر إلى مسافات وكلمات
    let mut raw_tokens: Vec<(BidiClass, String)> = Vec::new();
    let mut cur_ws: Option<bool> = None;
    let mut buf = String::new();

    for c in line.chars() {
        let is_w = c.is_whitespace();
        if cur_ws == Some(is_w) {
            buf.push(c);
        } else {
            if let Some(prev_w) = cur_ws {
                let tok = std::mem::take(&mut buf);
                let cls = if prev_w {
                    BidiClass::Ws
                } else if tok.chars().any(is_arabic_char) {
                    BidiClass::Rtl
                } else if tok.chars().any(is_ltr_alnum) {
                    BidiClass::Ltr
                } else {
                    BidiClass::Neutral
                };
                raw_tokens.push((cls, tok));
            }
            cur_ws = Some(is_w);
            buf.push(c);
        }
    }
    if let Some(prev_w) = cur_ws {
        let cls = if prev_w {
            BidiClass::Ws
        } else if buf.chars().any(is_arabic_char) {
            BidiClass::Rtl
        } else if buf.chars().any(is_ltr_alnum) {
            BidiClass::Ltr
        } else {
            BidiClass::Neutral
        };
        raw_tokens.push((cls, buf));
    }

    // 2. دمج الكلمات اللاتينية المتتالية (مع الفواصل البينية مثل المسافات و `/` و `-` و `:`) في مقطع LTR واحد
    //    حتى تبقى عبارات مثل `(Status Bar)` و `(SHA-256 / MD5 / CRC32)` بترتيبها الصحيح من اليسار لليمين.
    let mut merged: Vec<(BidiClass, String)> = Vec::new();
    let mut i = 0;
    let n = raw_tokens.len();

    while i < n {
        if raw_tokens[i].0 == BidiClass::Ltr {
            let mut ltr_buf = raw_tokens[i].1.clone();
            let mut j = i + 1;
            while j < n {
                if raw_tokens[j].0 == BidiClass::Ltr {
                    ltr_buf.push_str(&raw_tokens[j].1);
                    j += 1;
                } else if is_ltr_connector(&raw_tokens[j].0, &raw_tokens[j].1) {
                    let mut k = j + 1;
                    while k < n && is_ltr_connector(&raw_tokens[k].0, &raw_tokens[k].1) {
                        k += 1;
                    }
                    if k < n && raw_tokens[k].0 == BidiClass::Ltr {
                        for item in &raw_tokens[j..=k] {
                            ltr_buf.push_str(&item.1);
                        }
                        j = k + 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            merged.push((BidiClass::Ltr, ltr_buf));
            i = j;
        } else {
            merged.push(raw_tokens[i].clone());
            i += 1;
        }
    }

    // 3. فصل الأقواس الخارجية غير المتوازنة على أطراف مقطع LTR (مثل القوس النهائي في `(مثال: https://example.com/file.zip)`)
    let mut runs: Vec<(BidiClass, String)> = Vec::with_capacity(merged.len() + 2);
    for (cls, val) in merged {
        if cls == BidiClass::Ltr {
            let open_cnt = val.chars().filter(|&c| c == '(').count();
            let close_cnt = val.chars().filter(|&c| c == ')').count();
            if close_cnt > open_cnt && val.ends_with("):") {
                runs.push((BidiClass::Ltr, val[..val.len() - 2].to_string()));
                runs.push((BidiClass::Rtl, "):".to_string()));
            } else if close_cnt > open_cnt && val.ends_with(')') {
                runs.push((BidiClass::Ltr, val[..val.len() - 1].to_string()));
                runs.push((BidiClass::Rtl, ")".to_string()));
            } else if open_cnt > close_cnt && val.starts_with('(') {
                runs.push((BidiClass::Rtl, "(".to_string()));
                runs.push((BidiClass::Ltr, val[1..].to_string()));
            } else {
                runs.push((BidiClass::Ltr, val));
            }
        } else {
            runs.push((cls, val));
        }
    }

    // 4. بناء السطر النهائي بعكس ترتيب المقاطع في السياق العربي مع إبقاء محتوى مقاطع LTR كما هو
    let mut out = String::with_capacity(line.len() * 2);
    for (cls, val) in runs.into_iter().rev() {
        match cls {
            BidiClass::Ltr | BidiClass::Ws => out.push_str(&val),
            BidiClass::Rtl | BidiClass::Neutral => out.push_str(&shape_rtl_token(&val)),
        }
    }
    out
}

/// تشكيل وعكس النصوص العربية لعرضها بانسجام ووضوح تام داخل محركات LTR (مثل egui)
/// مع حماية كاملة ضد التشكيل المزدوج (Idempotent Shaping) ودعم العبارات الإنجليزية المركبة (`LTR Runs`).
pub fn shape_arabic(text: &str) -> String {
    if !has_arabic(text) || is_already_shaped(text) {
        return text.to_string();
    }

    let lines: Vec<&str> = text.split('\n').collect();
    let mut shaped_lines = Vec::with_capacity(lines.len());

    for line in lines {
        shaped_lines.push(shape_bidi_line(line));
    }

    shaped_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arabic_shaping_is_idempotent() {
        let raw = "إضافة تنزيل جديد... (Ctrl+N)";
        let shaped_once = shape_arabic(raw);
        let shaped_twice = shape_arabic(&shaped_once);
        assert_eq!(shaped_once, shaped_twice);
        assert!(shaped_once.contains("(Ctrl+N)"));
    }

    #[test]
    fn test_preserves_multiword_english_phrases_and_brackets() {
        let raw = "إظهار شريط الحالة السفلي (Status Bar)";
        let shaped = shape_arabic(raw);
        assert!(shaped.contains("(Status Bar)"), "got: {shaped}");

        let raw2 = "أدخل نمط روابط (مثال: https://cdn.example.com/part_[01-10].zip)";
        let shaped2 = shape_arabic(raw2);
        assert!(
            shaped2.contains("https://cdn.example.com/part_[01-10].zip"),
            "got: {shaped2}"
        );
    }
}
