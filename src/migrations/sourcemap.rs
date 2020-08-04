use std::mem;

use edgeql_parser::position::Pos;


pub struct Span<N> {
    pub name: N,
    pub offset: Pos,
}

struct Slice<N> {
    name: N,
    byte_offset: usize,
    line_offset: usize,
}

pub struct SourceMap<N> {
    slices: Vec<Slice<N>>,
}

pub struct Builder<N> {
    buffer: String,
    lines: usize,
    source_map: SourceMap<N>,
}

impl<N> Builder<N> {
    pub fn new() -> Builder<N> {
        Builder {
            buffer: String::new(),
            lines: 0,
            source_map: SourceMap {
                slices: Vec::new(),
            }
        }
    }
    pub fn done(&mut self) -> (String, SourceMap<N>) {
        let data = mem::replace(self, Builder::new());
        (data.buffer, data.source_map)
    }
    pub fn add_lines(&mut self, name: N, data: &str) -> &mut Self {
        self.source_map.slices.push(Slice {
            name,
            byte_offset: self.buffer.len(),
            line_offset: self.lines,
        });
        self.buffer.push_str(data);
        if !data.ends_with('\n') {
            self.buffer.push('\n');
        }
        let mut carriage_return = true;
        for b in self.buffer.as_bytes() {
            if carriage_return {
                carriage_return = false;
                self.lines += 1;
            } else if *b == b'\n' {
                self.lines += 1;
            }
            if *b == b'\r' {
                carriage_return = true;
            }
        }
        self
    }
}

#[cfg(test)]
mod test {
    use super::Builder;

    #[test]
    fn simple() {
        let (text, map) = Builder::new()
            .add_lines("file1", "hello\nworld")
            .add_lines("file2", "another\nfile\n")
            .add_lines("file3", "file3")
            .done();
        // TODO(tailhook) test the source map
    }

    #[test]
    fn carriage_return() {
        let (text, map) = Builder::new()
            .add_lines("file1", "hello\r\nworld")
            .add_lines("file2", "another\rfile\r")
            .add_lines("file3", "line5\r\rline7")
            .done();
        // TODO(tailhook) test the source map
    }
}
