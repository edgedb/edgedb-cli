use std::fmt;

use colorful::Color;
use colorful::core::ColorInterface;

static THEME: once_cell::sync::Lazy<Theme> = once_cell::sync::Lazy::new(|| {
    if clicolors_control::colors_enabled() {
        Theme {
            fade: Some(Style { color: Color::Grey37,
                               bold: true, underline: false }),
            err_marker: Some(Style { color: Color::LightRed,
                                     bold: true, underline: false }),
            emphasize: Some(Style { color: Color::White,
                                    bold: true, underline: false }),
            command_hint: Some(Style { color: Color::White,
                                       bold: true, underline: false }),
            title: Some(Style { color: Color::White,
                                bold: true, underline: true }),
        }
    } else {
        Theme {
            fade: None,
            err_marker: None,
            emphasize: None,
            command_hint: None,
            title: None,
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
}

fn theme() -> &'static Theme {
    &*THEME
}

impl<T: fmt::Display> Highlight for &T {
}

impl<T: fmt::Display> fmt::Display for Colored<&T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(style) = self.style {
            write!(f, "\x1B[38;5;{}{}{}m{}\x1B[0m",
                style.color.to_color_str(),
                if style.bold { ";1" } else { "" },
                if style.underline { ";4" } else { "" },
                self.value)
        } else {
            self.value.fmt(f)
        }
    }
}

#[macro_export]
macro_rules! echo {
    ($word1:expr $(; $semi_word1:expr)*
     $(,$word:expr $(; $semi_word:expr )*)* $(,)?) => {
        // Buffer the whole output so mutliple processes do not interfere
        // each other
        {
            use ::std::fmt::Write;
            let mut buf = ::std::string::String::with_capacity(4096);
            write!(&mut buf, "{}", $word1)
                .expect("buffering of echo succeeds");
            $(
                write!(&mut buf, "{}", $semi_word1)
                    .expect("buffering of echo succeeds");
            )*
            $(
                buf.push(' ');
                write!(&mut buf, "{}", $word)
                    .expect("buffering of echo succeeds");
                $(
                    write!(&mut buf, "{}", $semi_word)
                        .expect("buffering of echo succeeds");
                )*
            )*
            if cfg!(windows) {
                buf.push('\r');
            }
            buf.push('\n');
            eprint!("{}", buf);
        };
    }
}
