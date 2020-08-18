use std::mem;


struct Slice<N> {
    name: N,
    byte_offset: usize,
    size: usize,
}

pub struct SourceMap<N> {
    slices: Vec<Slice<N>>,
}

pub struct Builder<N> {
    buffer: String,
    source_map: SourceMap<N>,
}

impl<N> Builder<N> {
    pub fn new() -> Builder<N> {
        Builder {
            buffer: String::new(),
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
            size: data.len(),
        });
        self.buffer.push_str(data);
        if !data.ends_with('\n') {
            self.buffer.push('\n');
        }
        self
    }
}

impl<N> SourceMap<N> {
    pub fn translate_range(&self, start: usize, end: usize)
        -> Result<(&N, usize), ()>
    {
        // TODO(tailhook) use binary search instead
        for slice in self.slices.iter().rev() {
            if start >= slice.byte_offset {
                let local_end = end - slice.byte_offset;
                if local_end > slice.size {
                    return Err(())
                }
                return Ok((&slice.name, slice.byte_offset));
            }
        }
        return Err(())
    }
}


#[cfg(test)]
mod test {
    use super::Builder;

    #[test]
    fn simple() {
        let (_, map) = Builder::new()
            .add_lines("file1", "hello\nworld")
            .add_lines("file2", "another\nfile\n")
            .add_lines("file3", "file3")
            .done();
        assert_eq!(map.translate_range(0, 1).unwrap(), (&"file1", 0));
        assert_eq!(map.translate_range(12, 14).unwrap(), (&"file2", 12));
        assert_eq!(map.translate_range(25, 30).unwrap(), (&"file3", 25));
    }

    #[test]
    fn carriage_return() {
        let (_, map) = Builder::new()
            .add_lines("file1", "hello\r\nworld")
            .add_lines("file2", "another\rfile\r")
            .add_lines("file3", "line5\r\rline7")
            .done();
        assert_eq!(map.translate_range(0, 1).unwrap(), (&"file1", 0));
        assert_eq!(map.translate_range(13, 14).unwrap(), (&"file2", 13));
        assert_eq!(map.translate_range(27, 39).unwrap(), (&"file3", 27));
    }
}
