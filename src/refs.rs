//! Lexer-based identifier occurrences. Shared by LSP and MCP.

use crate::check_parse::reserved_reason;
use crate::lexer::{tokenize, TokenKind};
use crate::span::Span;

/// Dynare NAME token: letter or underscore, then letters, digits, underscores.
pub fn is_legal_ident(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    reserved_reason(name).is_none()
}

/// Ident token whose span contains `byte_offset` (`start <= offset < end`).
pub fn ident_at(src: &str, byte_offset: u32) -> Option<(String, Span)> {
    for tok in tokenize(src) {
        if tok.kind != TokenKind::Ident {
            continue;
        }
        if tok.span.start <= byte_offset && byte_offset < tok.span.end {
            return Some((tok.text(src).to_string(), tok.span));
        }
    }
    None
}

/// Ident tokens whose text equals `name`. Comments and strings are trivia / not Ident.
pub fn occurrences(src: &str, name: &str) -> Vec<Span> {
    if name.is_empty() {
        return Vec::new();
    }
    tokenize(src)
        .into_iter()
        .filter(|tok| tok.kind == TokenKind::Ident && tok.text(src) == name)
        .map(|tok| tok.span)
        .collect()
}

/// Rewrite Ident occurrences of `old` to `new`. Illegal / reserved / no hits → original.
pub fn rename_in_text(src: &str, old: &str, new: &str) -> String {
    if old.is_empty() || old == new || !is_legal_ident(new) {
        return src.to_string();
    }
    let mut spans = occurrences(src, old);
    if spans.is_empty() {
        return src.to_string();
    }
    spans.sort_by_key(|s| s.start);
    spans.reverse();
    let mut out = src.to_string();
    for span in spans {
        let start = span.start as usize;
        let end = span.end as usize;
        if end > out.len() || start > end {
            continue;
        }
        out.replace_range(start..end, new);
    }
    out
}

/// Identifier immediately before the parenthesis that contains `byte_offset`.
pub fn option_owner_at(src: &str, byte_offset: u32) -> Option<String> {
    let tokens = tokenize(src);
    let mut owners: Vec<String> = Vec::new();
    let mut last_ident: Option<String> = None;
    for tok in &tokens {
        if tok.kind == TokenKind::Eof {
            break;
        }
        if tok.span.start >= byte_offset {
            break;
        }
        match tok.kind {
            TokenKind::Semi => {
                owners.clear();
                last_ident = None;
            }
            TokenKind::Ident => {
                last_ident = Some(tok.text(src).to_string());
            }
            TokenKind::LParen => {
                owners.push(last_ident.take().unwrap_or_default());
            }
            TokenKind::RParen => {
                owners.pop();
                last_ident = None;
            }
            _ => {
                last_ident = None;
            }
        }
    }
    let cmd = owners.last()?;
    if cmd.is_empty() {
        None
    } else {
        Some(cmd.clone())
    }
}

/// True when `name` appears in the parenthesis group that contains `byte_offset`.
/// The innermost group wins. A `;` closes any open groups before it.
pub fn enclosing_paren_has_ident(src: &str, byte_offset: u32, name: &str) -> bool {
    let tokens = tokenize(src);
    let mut open_at: Vec<usize> = Vec::new();
    let mut containing: Option<(usize, usize)> = None;
    for (i, tok) in tokens.iter().enumerate() {
        if tok.kind == TokenKind::Eof {
            break;
        }
        match tok.kind {
            TokenKind::Semi => open_at.clear(),
            TokenKind::LParen => open_at.push(i),
            TokenKind::RParen => {
                if let Some(start) = open_at.pop() {
                    let inside =
                        tokens[start].span.end <= byte_offset && byte_offset < tok.span.start;
                    if inside && containing.is_none() {
                        containing = Some((start, i));
                    }
                }
            }
            _ => {}
        }
    }
    let Some((start, end)) = containing else {
        return false;
    };
    tokens[start + 1..end]
        .iter()
        .any(|tok| tok.kind == TokenKind::Ident && tok.text(src).eq_ignore_ascii_case(name))
}

/// Command name if `byte_offset` sits inside a known command's parenthesised option list.
pub fn option_command_at(src: &str, byte_offset: u32) -> Option<String> {
    let cmd = option_owner_at(src, byte_offset)?;
    if crate::catalog::is_known_command(&cmd) {
        Some(cmd.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ident_at_skips_comment_and_string() {
        let src = "parameters betta;\n// betta\nfoo = \"betta\";\n";
        let real = src.find("betta").unwrap() as u32;
        let (name, span) = ident_at(src, real).expect("decl");
        assert_eq!(name, "betta");
        assert_eq!(&src[span.start as usize..span.end as usize], "betta");
        let comment = src.find("// betta").unwrap() as u32 + 3;
        assert!(ident_at(src, comment).is_none());
        let in_string = src.find("\"betta\"").unwrap() as u32 + 1;
        assert!(ident_at(src, in_string).is_none());
    }

    #[test]
    fn occurrences_skip_comment_empty_name() {
        let src = "parameters betta;\n// betta\nbetta = 0.99;\n";
        let hits = occurrences(src, "betta");
        assert_eq!(hits.len(), 2);
        assert!(occurrences(src, "").is_empty());
    }

    #[test]
    fn enclosing_paren_sees_heterogeneity_on_either_side_of_overwrite() {
        let before = "shocks(heterogeneity=d, overwrite);";
        let on_overwrite = before.find("overwrite").unwrap() as u32;
        assert!(enclosing_paren_has_ident(
            before,
            on_overwrite,
            "heterogeneity"
        ));
        let after = "shocks(overwrite, heterogeneity=d);";
        let on_overwrite = after.find("overwrite").unwrap() as u32;
        assert!(enclosing_paren_has_ident(
            after,
            on_overwrite,
            "heterogeneity"
        ));
        let regular = "shocks(overwrite);";
        let on_overwrite = regular.find("overwrite").unwrap() as u32;
        assert!(!enclosing_paren_has_ident(
            regular,
            on_overwrite,
            "heterogeneity"
        ));
    }

    #[test]
    fn rename_rejects_illegal_and_reserved() {
        let src = "parameters betta;\nbetta = 0.99;\n";
        assert_eq!(rename_in_text(src, "betta", "1bad"), src);
        assert_eq!(rename_in_text(src, "betta", "log"), src);
        assert_eq!(rename_in_text(src, "betta", "stoch_simul"), src);
        assert_eq!(rename_in_text(src, "betta", "var"), src);
        let steady = rename_in_text(src, "betta", "steady");
        assert!(steady.contains("steady"));
        assert!(!steady.contains("betta"));
        let out = rename_in_text(src, "betta", "beta_disc");
        assert!(out.contains("beta_disc"));
        assert!(!out.contains("betta"));
    }
}
