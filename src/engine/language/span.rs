//! Byte offsets inside the library. Callers convert at the edge.
//!
//! [`LineIndex::position`] and [`LineIndex::offset`] count Unicode scalars.
//! Formatted diagnostic lines and MCP use those. The language server uses
//! [`LineIndex::position_utf16`] and [`LineIndex::offset_utf16`], which count
//! UTF-16 code units.

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

    /// Inverse of [`Self::position`]: line / Unicode-scalar character → byte.
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

    /// Byte → line / UTF-16 code unit. A supplementary character such as
    /// U+1F600 counts as 2. A BMP character, including Chinese, counts as 1.
    pub fn position_utf16(&self, src: &str, byte: u32) -> Position {
        let idx = self
            .starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1);
        let line_start = self.starts[idx] as usize;
        let byte = (byte as usize).min(src.len());
        let character = src
            .get(line_start..byte)
            .unwrap_or("")
            .encode_utf16()
            .count() as u32;
        Position {
            line: idx as u32,
            character,
        }
    }

    /// Inverse of [`Self::position_utf16`]. A line ending is `\n` or `\r\n`;
    /// the offset is within the line text and does not count the CR.
    ///
    /// An offset that falls between the two code units of one character maps
    /// to that character's first byte. There is no byte between those units.
    pub fn offset_utf16(&self, src: &str, pos: Position) -> u32 {
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
        let mut units = 0u32;
        for (i, ch) in line_text.char_indices() {
            let width = ch.len_utf16() as u32;
            if pos.character < units + width {
                return (start + i) as u32;
            }
            units += width;
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

    #[test]
    fn utf16_emoji_column_differs_from_scalar_and_bmp_still_matches() {
        // `var 😀😀y`: scalar index of `y` is 6 (MCP column 7); UTF-16 is 8.
        let src = "var \u{1F600}\u{1F600}y";
        let index = LineIndex::new(src);
        let y = src.find('y').unwrap() as u32;
        let scalar = index.position(src, y);
        let utf16 = index.position_utf16(src, y);
        assert_eq!(scalar.line, 0);
        assert_eq!(scalar.character, 6);
        assert_eq!(scalar.character + 1, 7);
        assert_eq!(utf16.character, 8);
        assert_eq!(index.offset(src, scalar), y);
        assert_eq!(index.offset_utf16(src, utf16), y);
        // Second code unit of the first emoji has no byte of its own.
        let emoji = src.find('\u{1F600}').unwrap() as u32;
        assert_eq!(
            index.offset_utf16(
                src,
                Position {
                    line: 0,
                    character: 5
                }
            ),
            emoji
        );

        let chinese = "// 产出\nvar y;\n";
        let index = LineIndex::new(chinese);
        let y = chinese.find('y').unwrap() as u32;
        let scalar = index.position(chinese, y);
        let utf16 = index.position_utf16(chinese, y);
        assert_eq!(scalar, utf16);
        assert_eq!(
            utf16,
            Position {
                line: 1,
                character: 4
            }
        );
        assert_eq!(index.offset_utf16(chinese, utf16), y);
        let chan = chinese.find('产').unwrap() as u32;
        assert_eq!(
            index.position(chinese, chan),
            index.position_utf16(chinese, chan)
        );
    }

    #[test]
    fn utf16_round_trip_crlf_and_eof() {
        let crlf = "var y;\r\nx;\r\n";
        let index = LineIndex::new(crlf);
        let y = crlf.find('y').unwrap() as u32;
        let pos = index.position_utf16(crlf, y);
        assert_eq!(
            pos,
            Position {
                line: 0,
                character: 4
            }
        );
        assert_eq!(index.offset_utf16(crlf, pos), y);
        let end_of_line = Position {
            line: 0,
            character: 6,
        };
        let at_cr = index.offset_utf16(crlf, end_of_line);
        assert_eq!(crlf.as_bytes()[at_cr as usize], b'\r');
        assert_eq!(index.position_utf16(crlf, at_cr), end_of_line);
        assert_eq!(
            index.offset_utf16(
                crlf,
                Position {
                    line: 0,
                    character: 80,
                }
            ),
            at_cr
        );

        let no_nl = "var y;";
        let index = LineIndex::new(no_nl);
        let eof = Position {
            line: 0,
            character: 6,
        };
        assert_eq!(index.offset_utf16(no_nl, eof), no_nl.len() as u32);
        assert_eq!(index.position_utf16(no_nl, no_nl.len() as u32), eof);
        assert_eq!(
            index.offset_utf16(
                no_nl,
                Position {
                    line: 3,
                    character: 1,
                }
            ),
            no_nl.len() as u32
        );

        let with_nl = "var y;\n";
        let index = LineIndex::new(with_nl);
        let empty_last = Position {
            line: 1,
            character: 0,
        };
        assert_eq!(
            index.offset_utf16(with_nl, empty_last),
            with_nl.len() as u32
        );
        assert_eq!(
            index.position_utf16(with_nl, with_nl.len() as u32),
            empty_last
        );
        assert_eq!(
            index.offset_utf16(
                with_nl,
                Position {
                    line: 0,
                    character: 6,
                }
            ),
            with_nl.find('\n').unwrap() as u32
        );
    }
}
