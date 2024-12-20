use std::fmt;

use colorful::core::ColorInterface;
use colorful::Color;

static THEME: once_cell::sync::Lazy<Theme> = once_cell::sync::Lazy::new(|| {
    if concolor::get(concolor::Stream::Stdout).color() {
        Theme {
            fade: Some(Style {
                color: Color::Grey37,
                bold: true,
                underline: false,
            }),
            err_marker: Some(Style {
                color: Color::LightRed,
                bold: true,
                underline: false,
            }),
            emphasize: Some(Style {
                color: Color::White,
                bold: true,
                underline: false,
            }),
            command_hint: Some(Style {
                color: Color::White,
                bold: true,
                underline: false,
            }),
            title: Some(Style {
                color: Color::White,
                bold: true,
                underline: true,
            }),
            deleted: Some(Style {
                color: Color::Red,
                bold: false,
                underline: false,
            }),
            added: Some(Style {
                color: Color::Green,
                bold: false,
                underline: false,
            }),
        }
    } else {
        Theme {
            fade: None,
            err_marker: None,
            emphasize: None,
            command_hint: None,
            title: None,
            deleted: None,
            added: None,
        }
    }
});

#[derive(Clone, Copy)]
struct Style {
    color: Color,
    bold: bool,
    underline: bool,
}

struct Theme {
    fade: Option<Style>,
    err_marker: Option<Style>,
    emphasize: Option<Style>,
    command_hint: Option<Style>,
    #[allow(dead_code)]
    title: Option<Style>,
    added: Option<Style>,
    deleted: Option<Style>,
}

pub struct Colored<T> {
    style: Option<Style>,
    value: T,
}

pub trait Highlight: fmt::Display + Sized {
    fn fade(self) -> Colored<Self> {
        Colored {
            style: theme().fade,
            value: self,
        }
    }
    #[allow(dead_code)]
    fn title(self) -> Colored<Self> {
        Colored {
            style: theme().title,
            value: self,
        }
    }
    fn err_marker(self) -> Colored<Self> {
        Colored {
            style: theme().err_marker,
            value: self,
        }
    }
    fn emphasize(self) -> Colored<Self> {
        Colored {
            style: theme().emphasize,
            value: self,
        }
    }
    fn command_hint(self) -> Colored<Self> {
        Colored {
            style: theme().command_hint,
            value: self,
        }
    }
    fn deleted(self) -> Colored<Self> {
        Colored {
            style: theme().deleted,
            value: self,
        }
    }
    fn added(self) -> Colored<Self> {
        Colored {
            style: theme().added,
            value: self,
        }
    }
}

fn theme() -> &'static Theme {
    &THEME
}

impl<T: fmt::Display> Highlight for &T {}

impl<T: fmt::Display> fmt::Display for Colored<&T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(style) = self.style {
            write!(
                f,
                "\x1B[38;5;{}{}{}m{}\x1B[0m",
                style.color.to_color_str(),
                if style.bold { ";1" } else { "" },
                if style.underline { ";4" } else { "" },
                self.value
            )
        } else {
            self.value.fmt(f)
        }
    }
}

#[macro_export]
macro_rules! msg {
    ($($tt:tt)*) => {
        eprintln!($($tt)*);
    }
}
