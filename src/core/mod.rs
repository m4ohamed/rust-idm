//! الوحدة الأساسية لمحرك التحميل (Core Engine Module)
//! تضم محدد السرعة القياسي (`limiter`)، إدارة التخزين الذري والبصمات الرقمية (`storage`)،
//! ومحرك التقسيم الديناميكي النصفي الخالي من التنافس على الأقفال (`segmenter`).

// إعادة تصدير وحدات ::core القياسية لضمان التوافق التام مع أي ماكرو (derive macro) يشير إلى core::*
#[doc(hidden)]
pub use ::core::{
    clone, cmp, convert, default, fmt, future, hash, hint, iter, marker, mem, ops, option, panic,
    pin, primitive, ptr, result, slice, str, task,
};

pub mod limiter;
pub mod segmenter;
pub mod storage;

pub use limiter::*;
pub use segmenter::*;
pub use storage::*;
