use std::fmt::Write;
use std::collections::HashMap;
use std::sync::Arc;

use colorful::{Colorful, Color, Style as TermStyle};
use colorful::core::color_string::CString;


#[derive(Hash, PartialEq, Eq, Debug, Clone, Copy)]
pub enum Style {
    Decorator,
    Comment,
    String,
    Number,
    Boolean,
    UUID,
    Enum,
    Cast,
    SetLiteral,
    ArrayLiteral,
    TupleLiteral,
    TupleField,
    ObjectLiteral,
    ObjectLinkProperty,
    ObjectPointer,
    Punctuation,
    Keyword,
    Operator,
    BackslashCommand,
    Error,
}

#[derive(Debug)]
pub struct Styled<T>(T, Style);

#[derive(Debug)]
pub struct Item(Option<Color>, Option<TermStyle>);

#[derive(Debug)]
pub struct Theme {
    items: HashMap<Style, Item>,
}

#[derive(Debug, Clone)]
pub struct Styler(Arc<Theme>);


impl Styler {
    pub fn dark_256() -> Styler {
        use self::Style::*;
        use colorful::Style::*;

        let mut t = HashMap::new();
        t.insert(String,            Item(Some(Color::DarkOliveGreen3a), None));
        t.insert(SetLiteral,        Item(Some(Color::SteelBlue), None));
        t.insert(ObjectLiteral,     Item(Some(Color::Grey63), None));
        t.insert(ObjectLinkProperty,Item(Some(Color::IndianRed1b), None));
        t.insert(Number,            Item(Some(Color::CadetBlue1), None));
        t.insert(Boolean,           Item(Some(Color::LightSalmon3b), None));
        t.insert(Enum,              Item(Some(Color::DarkGoldenrod), None));
        t.insert(UUID,              Item(Some(Color::LightGoldenrod3), None));
        t.insert(Keyword,           Item(Some(Color::IndianRed1b), None));
        t.insert(Operator,          Item(Some(Color::IndianRed1b), None));
        t.insert(Comment,           Item(Some(Color::Grey66), None));
        t.insert(Cast,              Item(Some(Color::IndianRed1b), None));
        t.insert(Error,             Item(Some(Color::IndianRed1c), None));
        t.insert(BackslashCommand,  Item(Some(Color::MediumPurple2a), Some(Bold)));

        return Styler(Arc::new(Theme {
            items: t,
        }));
    }
    pub fn write(&self, style: Style, data: &str, buf: &mut String) {
        write!(buf, "{}", self.apply(style, data)).unwrap();
    }
    pub fn apply(&self, style: Style, data: &str) -> CString {
        if let Some(Item(col, style)) = self.0.items.get(&style) {
            return match (col, style) {
                (Some(c), Some(s)) => data.color(*c).style(*s),
                (Some(c), None) => data.color(*c),
                (None, Some(s)) => data.style(*s),
                (None, None) => CString::new(data)
            }
        } else {
            return CString::new(data);
        }
    }
}
