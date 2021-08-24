use std::fmt;

use colorful::Color;
use colorful::core::ColorInterface;

static THEME: once_cell::sync::Lazy<Theme> = once_cell::sync::Lazy::new(|| {
    if clicolors_control::colors_enabled() {
        Theme {
            fade: Some(Color::Grey37),
        }
    } else {
        Theme {
            fade: None,
        }
    }
});

struct Theme {
    fade: Option<Color>,
}

pub struct Colored<T> {
    color: Option<Color>,
    value: T,
}


pub trait Highlight: fmt::Display + Sized {
    fn fade(self) -> Colored<Self> {
        Colored {
            color: theme().fade,
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
        if let Some(col) = self.color {
            write!(f, "\x1B[38;5;{}m{}\x1B[0m", col.to_color_str(), self.value)
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
