//! End of a Dynare `NATIVE` line.
//!
//! The pinned lexer copies native MATLAB through the newline. `...` before
//! that newline continues the line. A block comment returns to `NATIVE`.
//! `... /*` stays native until `*/` followed by whitespace and a newline.

/// Byte index of the newline that ends the native region, or `src.len()` at EOF.
///
/// `start` is the first byte of the native head. The newline itself is not
/// part of the region. A token that starts before this index is still native.
pub(crate) fn native_region_end(src: &str, start: usize) -> usize {
    let bytes = src.as_bytes();
    let mut i = start.min(bytes.len());
    let mut state = State::Native;
    while i < bytes.len() {
        match state {
            State::Native => match native_step(bytes, i) {
                Step::Stay(next) => i = next,
                Step::End(at) => return at,
                Step::Comment(next) => {
                    i = next;
                    state = State::Comment;
                }
                Step::NativeComment(next) => {
                    i = next;
                    state = State::NativeComment;
                }
            },
            State::Comment => {
                let next = skip_flex_space(bytes, i);
                if next > i {
                    i = next;
                    continue;
                }
                if bytes[i..].starts_with(b"*/") {
                    i += 2;
                    state = State::Native;
                    continue;
                }
                i += 1;
            }
            State::NativeComment => {
                if let Some(next) = native_comment_close(bytes, i) {
                    i = next;
                    state = State::Native;
                    continue;
                }
                i += 1;
            }
        }
    }
    bytes.len()
}

#[derive(Clone, Copy)]
enum State {
    Native,
    Comment,
    NativeComment,
}

enum Step {
    Stay(usize),
    End(usize),
    Comment(usize),
    NativeComment(usize),
}

fn prefer(best_len: &mut usize, best: &mut Step, len: usize, step: Step) {
    if len > *best_len {
        *best_len = len;
        *best = step;
    }
}

fn native_step(bytes: &[u8], i: usize) -> Step {
    let mut best_len = 0usize;
    let mut best = Step::Stay(i + 1);

    if let Some(len) = ordinary_run(bytes, i) {
        prefer(&mut best_len, &mut best, len, Step::Stay(i + len));
    }
    if bytes[i] == b'\'' {
        prefer(&mut best_len, &mut best, 1, Step::Stay(i + 1));
        if let Some(len) = quoted(bytes, i, b'\'') {
            prefer(&mut best_len, &mut best, len, Step::Stay(i + len));
        }
    }
    if let Some(len) = quoted(bytes, i, b'"') {
        prefer(&mut best_len, &mut best, len, Step::Stay(i + len));
    }
    if let Some(len) = one_or_two_dots(bytes, i) {
        prefer(&mut best_len, &mut best, len, Step::Stay(i + len));
    }
    if bytes[i] == b'*' {
        prefer(&mut best_len, &mut best, 1, Step::Stay(i + 1));
    }
    if bytes[i] == b'/' {
        prefer(&mut best_len, &mut best, 1, Step::Stay(i + 1));
    }
    if let Some(end) = dot_continuation(bytes, i, Continuation::Newline) {
        prefer(&mut best_len, &mut best, end - i, Step::Stay(end));
    }
    if bytes[i] == b'\n' {
        prefer(&mut best_len, &mut best, 1, Step::End(i));
    }
    if let Some(end) = dot_continuation(bytes, i, Continuation::Percent) {
        prefer(&mut best_len, &mut best, end - i, Step::Stay(end));
    }
    if bytes[i] == b'%' {
        let len = line_comment_body(bytes, i + 1);
        prefer(&mut best_len, &mut best, 1 + len, Step::Stay(i + 1 + len));
    }
    if let Some(end) = dot_continuation(bytes, i, Continuation::SlashSlash) {
        prefer(&mut best_len, &mut best, end - i, Step::Stay(end));
    }
    if bytes[i..].starts_with(b"//") {
        let len = line_comment_body(bytes, i + 2);
        prefer(&mut best_len, &mut best, 2 + len, Step::Stay(i + 2 + len));
    }
    if let Some(end) = dot_continuation(bytes, i, Continuation::Block) {
        prefer(&mut best_len, &mut best, end - i, Step::NativeComment(end));
    }
    if bytes[i..].starts_with(b"/*") {
        prefer(&mut best_len, &mut best, 2, Step::Comment(i + 2));
    }
    if best_len == 0 {
        Step::Stay(i + 1)
    } else {
        best
    }
}

#[derive(Clone, Copy)]
enum Continuation {
    Newline,
    Percent,
    SlashSlash,
    Block,
}

fn is_flex_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

fn ordinary(byte: u8) -> bool {
    !matches!(byte, b'/' | b'%' | b'*' | b'\n' | b'.' | b'\'' | b'"')
}

fn ordinary_run(bytes: &[u8], i: usize) -> Option<usize> {
    let mut k = i;
    while k < bytes.len() && ordinary(bytes[k]) {
        k += 1;
    }
    (k > i).then_some(k - i)
}

fn quoted(bytes: &[u8], i: usize, quote: u8) -> Option<usize> {
    if bytes.get(i) != Some(&quote) {
        return None;
    }
    let mut k = i + 1;
    while k < bytes.len() && bytes[k] != b'\n' && bytes[k] != quote {
        k += 1;
    }
    if bytes.get(k) == Some(&quote) {
        Some(k + 1 - i)
    } else {
        None
    }
}

fn one_or_two_dots(bytes: &[u8], i: usize) -> Option<usize> {
    let dots = count_dots(bytes, i);
    if dots == 0 {
        None
    } else {
        Some(dots.min(2))
    }
}

fn count_dots(bytes: &[u8], i: usize) -> usize {
    bytes[i..].iter().take_while(|byte| **byte == b'.').count()
}

fn skip_flex_space(bytes: &[u8], i: usize) -> usize {
    let mut k = i;
    while k < bytes.len() && is_flex_space(bytes[k]) {
        k += 1;
    }
    k
}

fn line_comment_body(bytes: &[u8], i: usize) -> usize {
    bytes[i..].iter().take_while(|byte| **byte != b'\n').count()
}

/// Longest `\.{3,}[[:space:]]*` tail. `Newline`, `Percent`, and `SlashSlash`
/// include the terminating newline. `Block` stops after `/*`.
fn dot_continuation(bytes: &[u8], i: usize, kind: Continuation) -> Option<usize> {
    let dots = count_dots(bytes, i);
    if dots < 3 {
        return None;
    }
    let mut k = skip_flex_space(bytes, i + dots);
    match kind {
        Continuation::Newline => last_newline_end(bytes, i + dots, k),
        Continuation::Percent => {
            if bytes.get(k) != Some(&b'%') {
                return None;
            }
            k += 1;
            through_newline(bytes, k)
        }
        Continuation::SlashSlash => {
            if !bytes.get(k..)?.starts_with(b"//") {
                return None;
            }
            k += 2;
            through_newline(bytes, k)
        }
        Continuation::Block => bytes.get(k..)?.starts_with(b"/*").then_some(k + 2),
    }
}

fn last_newline_end(bytes: &[u8], start: usize, end: usize) -> Option<usize> {
    let rel = bytes[start..end].iter().rposition(|byte| *byte == b'\n')?;
    Some(start + rel + 1)
}

fn through_newline(bytes: &[u8], i: usize) -> Option<usize> {
    let rel = bytes[i..].iter().position(|byte| *byte == b'\n')?;
    Some(i + rel + 1)
}

/// `*/` plus the longest whitespace run that ends on a newline.
fn native_comment_close(bytes: &[u8], i: usize) -> Option<usize> {
    if !bytes[i..].starts_with(b"*/") {
        return None;
    }
    let start = i + 2;
    let end = skip_flex_space(bytes, start);
    last_newline_end(bytes, start, end)
}

#[cfg(test)]
mod tests {
    use super::native_region_end;

    fn end_at(src: &str) -> usize {
        native_region_end(src, 0)
    }

    #[test]
    fn newline_ends_the_line() {
        let src = "proof = 1; rho = pp.rho;\nrho = pp.rho;\n";
        let end = end_at(src);
        assert_eq!(&src[..end], "proof = 1; rho = pp.rho;");
        assert_eq!(src.as_bytes()[end], b'\n');
    }

    #[test]
    fn dots_continue_across_a_blank_line() {
        let src = "proof = 1 ...\n\nrho = pp.rho;\n";
        let end = end_at(src);
        assert!(src[..end].ends_with("rho = pp.rho;"));
        assert_eq!(src.as_bytes()[end], b'\n');
    }

    #[test]
    fn dots_percent_and_slash_keep_the_next_line() {
        let percent = "proof = 1 ... % stay\nrho = pp.rho;\n";
        assert!(percent[..end_at(percent)].ends_with("rho = pp.rho;"));
        let slashes = "proof = 1 ... // stay\nrho = pp.rho;\n";
        assert!(slashes[..end_at(slashes)].ends_with("rho = pp.rho;"));
    }

    #[test]
    fn a_percent_comment_does_not_pull_the_next_line() {
        let src = "proof = 1; % rho = pp.rho;\nrho = pp.rho;\n";
        let end = end_at(src);
        assert_eq!(&src[..end], "proof = 1; % rho = pp.rho;");
    }

    #[test]
    fn a_block_comment_returns_before_the_following_newline() {
        let src = "proof = 1; /* keep\ngoing */ rho = pp.rho;\n";
        assert!(src[..end_at(src)].ends_with("rho = pp.rho;"));
        let closed = "proof = 1; /*\n*/\nrho = pp.rho;\n";
        let end = end_at(closed);
        assert_eq!(&closed[..end], "proof = 1; /*\n*/");
    }

    #[test]
    fn dots_block_comment_keeps_the_line_after_the_closer() {
        let src = "proof = 1 ... /*\n*/\nrho = pp.rho;\nnext;\n";
        let end = end_at(src);
        assert!(src[..end].ends_with("rho = pp.rho;"));
        assert!(src[end..].starts_with("\nnext;"));
    }

    #[test]
    fn an_unclosed_native_comment_runs_to_eof() {
        let src = "proof = 1 ... /* c */ rho = pp.rho;\nrho = 1;\n";
        assert_eq!(end_at(src), src.len());
    }

    #[test]
    fn a_quoted_comment_marker_stays_on_its_line() {
        let src = "proof = '/* not';\nrho = pp.rho;\n";
        assert_eq!(&src[..end_at(src)], "proof = '/* not';");
        let double = "proof = \"/* not\";\nrho = pp.rho;\n";
        assert_eq!(&double[..end_at(double)], "proof = \"/* not\";");
    }

    #[test]
    fn carriage_return_does_not_end_the_line() {
        let src = "proof = 1;\r\nrho = pp.rho;\r\n";
        let end = end_at(src);
        assert_eq!(src.as_bytes()[end], b'\n');
        assert_eq!(src[..end].trim_end_matches('\r'), "proof = 1;");
    }
}
