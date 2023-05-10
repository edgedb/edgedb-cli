use std::fmt::{self, Write};
use std::cmp::max;
use unicode_width::UnicodeWidthChar;

use terminal_size::{terminal_size, Width};


pub trait Contents {
    fn width_bounds(&self) -> (usize, usize);
    fn height(&self, width: usize) -> usize;
    fn render(&self, width: usize, height: usize, f: &mut fmt::Formatter)
        -> fmt::Result;
}

struct BufRender<'a> {
    cell: &'a dyn Contents,
    width: usize,
    height: usize,
}

#[derive(Debug, PartialEq, Clone)]
enum TextState {
    Normal,
    Escape,
    Bracket,
}

struct Counter {
    width: usize,
    state: TextState,
}

pub struct Float(pub f64);
pub struct Right<T: fmt::Display>(pub T);

pub fn render(table: &Vec<Vec<Box<dyn Contents+'_>>>) {
    let width = terminal_size().map(|(Width(w), _h)| w.into()).unwrap_or(200);
    let cols = table.iter().map(|r| r.len()).max().unwrap_or(1);
    let width_bounds = (0..cols).map(|c| {
        table.iter().map(|r| {
            r.get(c).map(|c| c.width_bounds()).unwrap_or((0, 0))
        })
        .fold((0, 0, 0), |(fmin, fmax, sum), (cmin, cmax)| {
            (max(fmin, cmin), max(fmax, cmax), sum + cmax)
        })
    }).collect::<Vec<(usize, usize, usize)>>();
    let borders = cols-1;  // TODO (tailhook)
    let min_width = width_bounds.iter().map(|&(m, _, _)| m).sum::<usize>() + borders;
    let max_width = width_bounds.iter().map(|&(_, m, _)| m).sum::<usize>() + borders;
    let widths = if max_width <= width {
        width_bounds.iter().map(|(_min, max, _sum)| *max).collect()
    } else if min_width >= width {
        width_bounds.iter().map(|(min, _max, _sum)| *min).collect()
    } else {
        let sum: usize = width_bounds.iter().map(|(_, _, s)| s).sum();
        let to_divide: usize = width - min_width;
        let mut widths = Vec::with_capacity(cols);
        let mut total_width = 0;
        for &(cmin, _, csum) in &width_bounds {
            let cwidth = cmin + (to_divide as f64*csum as f64/sum as f64) as usize;
            widths.push(cwidth);
            total_width += cwidth;
        }
        let mut rem_width = width - total_width - borders;
        for idx in (0..cols).cycle() {
            if rem_width == 0 {
                break;
            }
            if widths[idx] < width_bounds[idx].1 {
                widths[idx] += 1;
                rem_width -= 1;
            }
        }
        widths
    };
    let heights = table.iter().map(|row| {
        row.iter().zip(&widths).map(|(cell, &width)| cell.height(width))
            .max().unwrap_or(0)
    }).collect::<Vec<_>>();

    let mut buffers = widths.iter()
        .map(|w| String::with_capacity(*w))
        .collect::<Vec<_>>();
    let mut line_buf = String::with_capacity(width + 1);
    for (row, height) in table.iter().zip(heights) {
        for (idx, (cell, &width)) in row.iter().zip(&widths).enumerate() {
            buffers[idx].truncate(0);
            write!(&mut buffers[idx], "{}",
                   BufRender { cell: &**cell, width, height }
            ).ok();
        }
        let mut lines = buffers.iter().map(|text| text.lines().peekable())
            .collect::<Vec<_>>();
        while lines.iter_mut().any(|l| l.peek().is_some()) {
            let mut next_col = 0;
            let mut col = 0;
            for (iter, width) in lines.iter_mut().zip(&widths) {
                if col < next_col {
                    for _ in 0..next_col - col {
                        line_buf.push(' ');
                    }
                }
                next_col = col + width + 1;
                iter.next().map(|line| {
                    col += str_width(line);
                    line_buf.push_str(line);
                });
            }
            println!("{}", line_buf);
            line_buf.truncate(0);
        }
    }
}

impl fmt::Display for BufRender<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.cell.render(self.width, self.height, f)
    }
}

impl<T: fmt::Display> Contents for T {
    fn width_bounds(&self) -> (usize, usize) {
        let mut cnt = Counter::new();
        write!(&mut cnt, "{}", self).expect("can write into counter");
        (cnt.width, cnt.width)
    }
    fn height(&self, _width: usize) -> usize {
        1
    }
    fn render(&self, _width: usize, _height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        write!(f, "{}", self)
    }
}

impl<T: fmt::Display> Contents for Right<T> {
    fn width_bounds(&self) -> (usize, usize) {
        let mut cnt = Counter::new();
        write!(&mut cnt, "{}", self.0).expect("can write into counter");
        (cnt.width, cnt.width)
    }
    fn height(&self, _width: usize) -> usize {
        1
    }
    fn render(&self, width: usize, _height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        let inner_width = self.width_bounds().0;
        write!(f, "{0:>1$}{2}", "", width - inner_width, self.0)
    }
}

impl Contents for Float {
    fn width_bounds(&self) -> (usize, usize) {
        let mut cnt = Counter::new();
        write!(&mut cnt, "{:.1}", self.0).expect("can write into counter");
        (cnt.width, cnt.width)
    }
    fn height(&self, _width: usize) -> usize {
        1
    }
    fn render(&self, width: usize, _height: usize, f: &mut fmt::Formatter)
        -> fmt::Result
    {
        write!(f, "{1:>0$.1}", width, self.0)
    }
}

impl Counter {
    fn new() -> Counter {
        Counter {
            width: 0,
            state: TextState::Normal,
        }
    }
    fn add_char(&mut self, c: char) {
        use TextState::*;
        match self.state {
            Escape => {
                if c == '[' {
                    self.state = Bracket;
                } else {
                    self.state = Normal;
                }
            }
            Bracket => {
                match c {
                    ';' | '?' | '0'..='9' => {},
                    _ => self.state = Normal,
                }
            }
            Normal if c == '\x1b' => {
                self.state = Escape;
            }
            Normal => {
                self.width += c.width().unwrap_or(0);
            }
        }
    }
}

impl fmt::Write for Counter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.add_char(c);
        }
        Ok(())
    }
}

fn str_width(s: &str) -> usize {
    let mut cnt = Counter::new();
    for c in s.chars() {
        cnt.add_char(c);
    }
    cnt.width
}
