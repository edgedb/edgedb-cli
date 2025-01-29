use std::fmt::Write;

use colorful::core::color_string::CString;

#[derive(Hash, PartialEq, Eq, Debug, Clone, Copy)]
#[allow(clippy::upper_case_acronyms)]
pub enum Style {
    Decorator,
    Comment,
    String,
    Number,
    Boolean,
    UUID,
    Enum,
    Cast,
    Set,
    Array,
    Tuple,
    TupleField,
    Object,
    LinkProperty,
    Pointer,
    Punctuation,
    Keyword,
    Operator,
    BackslashCommand,
    Error,
}

#[derive(Debug, Clone)]
pub struct Styler;

impl Styler {
    pub fn new() -> Styler {
        Styler
    }
    pub fn write(&self, style: Style, data: &str, buf: &mut String) {
        write!(buf, "{}", self.apply(style, data)).unwrap();
    }
    pub fn apply(&self, style: Style, data: &str) -> CString {
        super::color::apply_syntax_style(style, data)
    }
}

impl Default for Styler {
    fn default() -> Self {
        Self::new()
    }
}
