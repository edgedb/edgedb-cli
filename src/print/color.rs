use std::fmt;

use colorful::Color;
use colorful::core::ColorInterface;

static THEME: once_cell::sync::Lazy<Theme> = once_cell::sync::Lazy::new(|| {
    if clicolors_control::colors_enabled() {
        Theme {
            fade: Some(Style { color: Color::Grey37, bold: false }),
            err_marker: Some(Style { color: Color::LightRed, bold: true }),
            emphasize: Some(Style { color: Color::White, bold: true }),
        }
    } else {
        Theme {
            fade: None,
            err_marker: None,
            emphasize: None,
        }
    }
});

#[derive(Clone, Copy)]
struct Style {
    color: Color,
    bold: bool,
}

struct Theme {
    fade: Option<Style>,
    err_marker: Option<Style>,
    emphasize: Option<Style>,
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
}

fn theme() -> &'static Theme {
    &*THEME
}

impl<T: fmt::Display> Highlight for T {
}

impl<T: fmt::Display> fmt::Display for Colored<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(style) = self.style {
            write!(f, "\x1B[38;5;{}{}m{}\x1B[0m",
                style.color.to_color_str(),
                if style.bold { ";1" } else { "" },
                self.value)
        } else {
            self.value.fmt(f)
        }
    }
}

#[macro_export]
macro_rules! echo {
    ($word1:expr $(,$word:expr )* $(,)?) => {
        // Buffer the whole output so mutliple processes do not interfere
        // each other
        {
            use ::std::fmt::Write;
            let mut buf = ::std::string::String::with_capacity(4096);
            write!(&mut buf, "{}", $word1)
                .expect("buffering of echo succeeds");
            $(
                buf.push(' ');
                write!(&mut buf, "{}", $word)
                    .expect("buffering of echo succeeds");
            )*
            buf.push('\n');
            print!("{}", buf);
        };
    }
}

#[macro_export]
macro_rules! eecho {
    ($word1:expr $(,$word:expr )* $(,)?) => {
        // Buffer the whole output so mutliple processes do not interfere
        // each other
        {
            use ::std::fmt::Write;
            let mut buf = ::std::string::String::with_capacity(4096);
            write!(&mut buf, "{}", $word1)
                .expect("buffering of echo succeeds");
            $(
                buf.push(' ');
                write!(&mut buf, "{}", $word)
                    .expect("buffering of echo succeeds");
            )*
            buf.push('\n');
            eprint!("{}", buf);
        };
    }
}
