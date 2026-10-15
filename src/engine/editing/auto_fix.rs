//! Apply stored `Diagnostic.fix` edits. Family modules stay pure.

use crate::engine::diagnostics::check_parse::check_parse;
use crate::engine::diagnostics::{analyze, Diagnostic, TextEdit};
use crate::engine::language::lexer::{tokenize, TokenKind};
use crate::engine::macro_work::MacroWorkLimit;
use std::sync::{atomic::AtomicBool, Arc};

const PASS2_CODES: &[&str] = &["E020", "E030", "W013", "W054", "W055", "W056", "W057"];

pub(crate) fn eligible_code(code: &str) -> bool {
    code == "E001" || PASS2_CODES.contains(&code)
}

/// Apply `edits` to `text`. Columns are Unicode scalars (`chars().count()`).
///
/// Order matches Python `_apply_edits`: later position first; overlapping
/// ranges skip; `999999` clamps to end-of-line.
pub fn apply_fix(text: &str, edits: &[TextEdit]) -> String {
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by(|a, b| {
        (a.start_line, a.start_char)
            .cmp(&(b.start_line, b.start_char))
            .reverse()
    });

    let mut applied: Vec<&TextEdit> = Vec::new();
    for edit in sorted {
        if crate::engine::control::cancelled() {
            return text.to_owned();
        }
        if applied.iter().any(|prior| ranges_overlap(edit, prior)) {
            continue;
        }
        let sl = edit.start_line as usize;
        if sl >= lines.len() {
            continue;
        }
        let mut el = edit.end_line as usize;
        let sc = (edit.start_char as usize).min(char_len(&lines[sl]));
        let ec = if el >= lines.len() {
            el = lines.len() - 1;
            char_len(&lines[el])
        } else {
            (edit.end_char as usize).min(char_len(&lines[el]))
        };
        if sl > el {
            continue;
        }
        let before = take_chars(&lines[sl], sc);
        let after = skip_chars(&lines[el], ec);
        let new_content = format!("{before}{}{after}", edit.new_text);
        let new_lines: Vec<String> = new_content.split('\n').map(str::to_string).collect();
        lines.splice(sl..=el, new_lines);
        applied.push(edit);
    }
    lines.join("\n")
}

/// Two-pass auto-fix. A stored edit is applied only when that edit, by itself,
/// reduces the number of diagnostics with the same code and message.
///
/// Refuses when the raw file has both `@#define`/`@#for` and a `@{ident}`
/// interpolation (lexer tokens; comments are trivia).
pub fn auto_fix(text: &str) -> String {
    auto_fix_outcome(text).unwrap_or_else(|_| text.to_string())
}

pub(crate) fn auto_fix_outcome(text: &str) -> Result<String, &'static str> {
    auto_fix_outcome_with_limit(text, MacroWorkLimit::default(), None).map_err(|failure| {
        match failure {
            AutoFixFailure::Refused(message) => message,
            AutoFixFailure::Cancelled => "The request was cancelled",
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AutoFixFailure {
    Cancelled,
    Refused(&'static str),
}

pub(crate) fn auto_fix_outcome_with_limit(
    text: &str,
    work_limit: MacroWorkLimit,
    cancelled: Option<&Arc<AtomicBool>>,
) -> Result<String, AutoFixFailure> {
    crate::engine::control::run(cancelled.cloned(), || {
        auto_fix_worker(text, work_limit, cancelled)
    })
    .map_err(|_| AutoFixFailure::Cancelled)?
}

fn auto_fix_worker(
    text: &str,
    work_limit: MacroWorkLimit,
    cancelled: Option<&Arc<AtomicBool>>,
) -> Result<String, AutoFixFailure> {
    if refuse_macro_rewrite(text) {
        return Err(AutoFixFailure::Refused("Automatic fixes were withheld because macro definitions or loops with identifier interpolation can change the written edit targets."));
    }
    let parsed = |text: &str| {
        crate::engine::language::parser::parse_with_budget(
            text,
            &mut crate::engine::language::macro_expr::MacroBudget::with_limit(
                work_limit,
                cancelled.cloned(),
            ),
        )
    };
    let mut text = text.to_string();
    for _ in 0..3 {
        if crate::engine::control::cancelled() {
            return Err(AutoFixFailure::Cancelled);
        }
        let diags = check_parse(&parsed(&text));
        let fixes = repairing_fixes(&text, &diags, true, work_limit, cancelled);
        if fixes.is_empty() {
            break;
        }
        text = apply_fix(&text, &fixes);
    }
    if crate::engine::control::cancelled() {
        return Err(AutoFixFailure::Cancelled);
    }
    let model = parsed(&text);
    let diags = match cancelled {
        Some(flag) => crate::engine::diagnostics::analyze_controlled(&model, flag)
            .map_err(|_| AutoFixFailure::Cancelled)?,
        None => analyze(&model),
    };
    let fixes = repairing_fixes(&text, &diags, false, work_limit, cancelled);
    if !fixes.is_empty() {
        text = apply_fix(&text, &fixes);
    }
    Ok(text)
}

fn repairing_fixes(
    text: &str,
    diags: &[Diagnostic],
    parse_pass: bool,
    work_limit: MacroWorkLimit,
    cancelled: Option<&Arc<AtomicBool>>,
) -> Vec<TextEdit> {
    diags
        .iter()
        .filter(|diagnostic| {
            if parse_pass {
                diagnostic.code == "E001"
            } else {
                PASS2_CODES.contains(&diagnostic.code.as_str())
            }
        })
        .filter(|diagnostic| diagnostic.fix.is_some())
        .take_while(|_| !crate::engine::control::cancelled())
        .filter(|diagnostic| {
            edit_repairs_owner(text, diags, diagnostic, parse_pass, work_limit, cancelled)
        })
        .filter_map(|diagnostic| diagnostic.fix.clone())
        .collect()
}

fn owner_count(diags: &[Diagnostic], owner: &Diagnostic) -> usize {
    diags
        .iter()
        .filter(|diagnostic| diagnostic.code == owner.code && diagnostic.message == owner.message)
        .count()
}

fn edit_repairs_owner(
    text: &str,
    before_diags: &[Diagnostic],
    owner: &Diagnostic,
    parse_pass: bool,
    work_limit: MacroWorkLimit,
    cancelled: Option<&Arc<AtomicBool>>,
) -> bool {
    if crate::engine::control::cancelled() {
        return false;
    }
    let Some(fix) = owner.fix.clone() else {
        return false;
    };
    let before = owner_count(before_diags, owner);
    if before == 0 {
        return false;
    }
    let candidate = apply_fix(text, &[fix]);
    let model = crate::engine::language::parser::parse_with_budget(
        &candidate,
        &mut crate::engine::language::macro_expr::MacroBudget::with_limit(
            work_limit,
            cancelled.cloned(),
        ),
    );
    let after = if parse_pass {
        check_parse(&model)
    } else {
        analyze(&model)
    };
    owner_count(&after, owner) < before
}

pub(crate) fn refuse_macro_rewrite(text: &str) -> bool {
    let tokens = tokenize(text);
    let mut has_define_or_for = false;
    let mut has_interp = false;
    for tok in &tokens {
        if crate::engine::control::cancelled() {
            return false;
        }
        match tok.kind {
            TokenKind::MacroDir
                if matches!(dir_kind(tok.text(text)).as_deref(), Some("define" | "for")) =>
            {
                has_define_or_for = true;
            }
            TokenKind::MacroInterp if is_ident(interp_inner(tok.text(text))) => {
                has_interp = true;
            }
            _ => {}
        }
        if has_define_or_for && has_interp {
            return true;
        }
    }
    false
}

fn dir_kind(text: &str) -> Option<String> {
    let rest = text.trim_start().strip_prefix("@#")?;
    let rest = rest.trim_start();
    let ident: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphabetic() || *c == '_')
        .collect();
    if ident.is_empty() {
        None
    } else {
        Some(ident.to_ascii_lowercase())
    }
}

fn interp_inner(text: &str) -> &str {
    text.strip_prefix("@{")
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(text)
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

fn ranges_overlap(a: &TextEdit, b: &TextEdit) -> bool {
    let a_start = (a.start_line, a.start_char);
    let a_end = (a.end_line, a.end_char);
    let b_start = (b.start_line, b.start_char);
    let b_end = (b.end_line, b.end_char);
    a_start < b_end && b_start < a_end
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn take_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn skip_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[i..],
        None => "",
    }
}

#[cfg(test)]
mod request_cancellation_tests {
    use super::*;
    use crate::engine::language::parser::parse;
    use std::sync::atomic::Ordering;

    #[test]
    fn cancellation_stops_stored_fix_repair_verification() {
        // Guards bulk Auto Fix continuing with all proposed-input reparses after
        // cancellation. The stop is armed after diagnostics and fixes exist.
        let text = "var y;model;\ny=1\nend;";
        let diagnostics = check_parse(&parse(text));
        assert!(diagnostics.iter().any(|row| row.fix.is_some()));
        let flag = Arc::new(AtomicBool::new(false));
        crate::engine::control::cancel_after(&flag, 4);
        let outcome = crate::engine::control::run(Some(flag.clone()), || {
            repairing_fixes(
                text,
                &diagnostics,
                true,
                MacroWorkLimit::default(),
                Some(&flag),
            )
        });
        assert_eq!(outcome, Err(crate::engine::control::Cancelled));
        assert!(flag.load(Ordering::Relaxed));
        assert_eq!(
            auto_fix_outcome_with_limit(text, MacroWorkLimit::default(), Some(&flag)),
            Err(AutoFixFailure::Cancelled)
        );
        assert_eq!(auto_fix_outcome(text).unwrap(), "var y;model;\ny=1;\nend;");
    }
}
