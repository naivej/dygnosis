//! Byte spans internally; line/character at the CLI/LSP/MCP edge.
//!
//! Python's oracle uses 0-based line and Unicode scalar `character` (a Python
//! `str` index). Convert at the edge so internals can stay on byte offsets.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: start as u32,
            end: end as u32,
        }
    }

    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug)]
pub struct LineIndex {
    /// Byte offset of the first character of each line.
    starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        for (i, b) in src.bytes().enumerate() {
            if b == b'\n' {
                starts.push((i + 1) as u32);
            }
        }
        Self { starts }
    }

    pub fn position(&self, src: &str, byte: u32) -> Position {
        let idx = self
            .starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1);
        let line_start = self.starts[idx] as usize;
        let byte = (byte as usize).min(src.len());
        let character = src.get(line_start..byte).unwrap_or("").chars().count() as u32;
        Position {
            line: idx as u32,
            character,
        }
    }

    /// Inverse of [`Self::position`]: LSP line / Unicode-scalar character → byte.
    pub fn offset(&self, src: &str, pos: Position) -> u32 {
        let line = pos.line as usize;
        let Some(&start) = self.starts.get(line) else {
            return src.len() as u32;
        };
        let start = start as usize;
        let next = self
            .starts
            .get(line + 1)
            .map(|&s| s as usize)
            .unwrap_or(src.len());
        let line_bytes = src.get(start..next).unwrap_or("");
        let line_text = line_bytes.strip_suffix('\n').unwrap_or(line_bytes);
        let line_text = line_text.strip_suffix('\r').unwrap_or(line_text);
        for (chars, (i, _)) in line_text.char_indices().enumerate() {
            if chars as u32 == pos.character {
                return (start + i) as u32;
            }
        }
        (start + line_text.len()) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_comment_does_not_shift_code_column() {
        let src = "// 产出\nvar y;\n";
        let index = LineIndex::new(src);
        let y = src.find("y").unwrap() as u32;
        let pos = index.position(src, y);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 4);
        assert_eq!(index.offset(src, pos), y);
    }
}
