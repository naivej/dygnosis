//! Companion file mentions harvested from a root `.mod` (path resolve only).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::engine::language::command_skip::{command_stmt_spans, is_parse_skip_command};
use crate::engine::language::expr::{ExprId, ExprKind};
use crate::engine::language::lexer::{Token, TokenKind, tokenize};
use crate::engine::language::macro_expand::expand_macros;
use crate::engine::language::model::Model;
use crate::engine::language::span::Span;
use crate::engine::workspace::include_resolver::{
    normalize_separators, path_key, supplied_path_key,
};

const RANK_OPTION: u8 = 3;
const RANK_CONVENTION: u8 = 2;
const RANK_LEFTOVER: u8 = 1;

const DATAFILE_SUFFIXES: &[&str] = &[".m", ".mat", ".csv", ".xls", ".xlsx"];
const M_SUFFIXES: &[&str] = &[".m"];

#[derive(Clone, Copy)]
pub(crate) enum CompanionIdentity {
    Supplied,
    Host,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompanionKind {
    SteadyStateFile,
    PriorRestrictions,
    Datafile,
    ModeFile,
    IrfMatchingFile,
    HelperM,
    RunScript,
}

impl CompanionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SteadyStateFile => "steady_state_file",
            Self::PriorRestrictions => "prior_restrictions",
            Self::Datafile => "datafile",
            Self::ModeFile => "mode_file",
            Self::IrfMatchingFile => "irf_matching_file",
            Self::HelperM => "helper_m",
            Self::RunScript => "run_script",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanionRecord {
    pub kind: CompanionKind,
    pub named_in: Span,
    pub name: String,
    pub path: Option<PathBuf>,
}

struct Pending {
    kind: CompanionKind,
    named_in: Span,
    name: String,
    path: Option<PathBuf>,
    rank: u8,
}

pub(crate) fn extra_suffixes(kind: CompanionKind) -> &'static [&'static str] {
    match kind {
        CompanionKind::Datafile => DATAFILE_SUFFIXES,
        CompanionKind::HelperM | CompanionKind::ModeFile | CompanionKind::IrfMatchingFile => {
            M_SUFFIXES
        }
        CompanionKind::SteadyStateFile
        | CompanionKind::PriorRestrictions
        | CompanionKind::RunScript => &[],
    }
}

pub(crate) fn first_line_span(src: &str) -> Span {
    Span::new(0, src.find('\n').unwrap_or(src.len()))
}

/// Command that loads the convention file, or the first line if that command is absent.
///
/// `FILENAME_steadystate.m` is used from `steady`. `FILENAME_prior_restrictions.m`
/// is called from `estimation`; 7.1 `method_of_moments` only `isfile`s it.
/// `run_FILENAME.m` is a MATLAB driver, not a `.mod` command — first line.
fn convention_named_in(
    kind: CompanionKind,
    source: &str,
    tokens: &[Token],
    cmd_spans: &[Span],
) -> Span {
    let names: &[&str] = match kind {
        CompanionKind::SteadyStateFile => &["steady"],
        CompanionKind::PriorRestrictions => &["estimation", "method_of_moments"],
        CompanionKind::RunScript
        | CompanionKind::Datafile
        | CompanionKind::ModeFile
        | CompanionKind::IrfMatchingFile
        | CompanionKind::HelperM => return first_line_span(source),
    };
    first_command_ident(source, tokens, cmd_spans, names).unwrap_or_else(|| first_line_span(source))
}

fn first_command_ident(
    source: &str,
    tokens: &[Token],
    cmd_spans: &[Span],
    names: &[&str],
) -> Option<Span> {
    for span in cmd_spans {
        let Some(tok) = tokens.iter().find(|t| {
            t.kind == TokenKind::Ident && t.span.start >= span.start && t.span.end <= span.end
        }) else {
            continue;
        };
        let ident = tok.text(source);
        if names.iter().any(|n| ident.eq_ignore_ascii_case(n)) {
            return Some(tok.span);
        }
    }
    None
}

pub(crate) fn harvest(
    source: &str,
    model: &Model,
    convention: &[(CompanionKind, String)],
    identity: CompanionIdentity,
    mut resolve: impl FnMut(&str, &[&str]) -> Option<PathBuf>,
) -> Vec<CompanionRecord> {
    let raw = tokenize(source);
    let tokens = expand_macros(source, raw);
    let cmd_spans = command_stmt_spans(&tokens, source);

    let mut pending = Vec::new();
    let mut option_value_spans = HashSet::new();

    for span in &cmd_spans {
        harvest_command_options(
            source,
            &tokens,
            *span,
            &mut resolve,
            &mut pending,
            &mut option_value_spans,
        );
    }

    harvest_leftover_quotes(
        source,
        &tokens,
        &option_value_spans,
        &mut resolve,
        &mut pending,
    );
    harvest_ident_helpers(
        source,
        model,
        &tokens,
        &cmd_spans,
        &mut resolve,
        &mut pending,
    );

    for (kind, name) in convention {
        if let Some(path) = resolve(name, extra_suffixes(*kind)) {
            pending.push(Pending {
                kind: *kind,
                named_in: convention_named_in(*kind, source, &tokens, &cmd_spans),
                name: name.clone(),
                path: Some(path),
                rank: RANK_CONVENTION,
            });
        }
    }

    merge(pending, identity)
}

fn harvest_command_options(
    source: &str,
    tokens: &[Token],
    span: Span,
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
    pending: &mut Vec<Pending>,
    option_value_spans: &mut HashSet<(u32, u32)>,
) {
    let inner: Vec<usize> = tokens
        .iter()
        .enumerate()
        .filter(|(_, t)| t.span.start >= span.start && t.span.end <= span.end)
        .map(|(i, _)| i)
        .collect();
    if inner.is_empty() {
        return;
    }
    let cmd_i = inner[0];
    if tokens[cmd_i].kind != TokenKind::Ident {
        return;
    }
    let command = tokens[cmd_i].text(source).to_string();
    let mut depth = 0i32;
    let mut i = 0usize;
    while i < inner.len() {
        let ti = inner[i];
        match tokens[ti].kind {
            TokenKind::LParen => {
                depth += 1;
                i += 1;
                continue;
            }
            TokenKind::RParen => {
                depth -= 1;
                i += 1;
                continue;
            }
            TokenKind::Ident if depth == 1 => {
                let next_kind = inner.get(i + 1).map(|&j| tokens[j].kind);
                if next_kind != Some(TokenKind::Eq) {
                    i += 1;
                    continue;
                }
                let option = tokens[ti].text(source);
                let Some(&val_i) = inner.get(i + 2) else {
                    i += 1;
                    continue;
                };
                let val = &tokens[val_i];
                if let Some(kind) = option_kind(&command, option) {
                    let value = if command.eq_ignore_ascii_case("external_function")
                        && val.kind == TokenKind::Ident
                    {
                        let (name, named_in, end) = qualified_name(source, tokens, val_i);
                        i += end - val_i;
                        Some((name, named_in))
                    } else {
                        option_value(source, val, kind)
                    };
                    if let Some((name, named_in)) = value {
                        option_value_spans.insert((named_in.start, named_in.end));
                        let path = if kind == CompanionKind::HelperM {
                            resolve_helper(
                                &name,
                                val.kind == TokenKind::String
                                    && name.rsplit_once('.').is_some_and(|(_, extension)| {
                                        extension.eq_ignore_ascii_case("m")
                                    }),
                                resolve,
                            )
                        } else {
                            resolve(&name, extra_suffixes(kind))
                        };
                        pending.push(Pending {
                            kind,
                            named_in,
                            name,
                            path,
                            rank: RANK_OPTION,
                        });
                    }
                }
                i += 3;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
}

/// The full written function name, including namespace components and trivia.
fn qualified_name(source: &str, tokens: &[Token], start: usize) -> (String, Span, usize) {
    let mut name = tokens[start].text(source).to_string();
    let mut end = start;
    while tokens
        .get(end + 1)
        .is_some_and(|t| t.kind == TokenKind::Dot)
        && tokens
            .get(end + 2)
            .is_some_and(|t| t.kind == TokenKind::Ident)
    {
        name.push('.');
        name.push_str(tokens[end + 2].text(source));
        end += 2;
    }
    (
        name,
        Span::new(
            tokens[start].span.start as usize,
            tokens[end].span.end as usize,
        ),
        end,
    )
}

/// Package function identities resolve relative to their package's parent.
/// Explicit quoted `.m` paths retain the collector's existing file semantics.
fn resolve_helper(
    name: &str,
    literal_path: bool,
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
) -> Option<PathBuf> {
    if !literal_path && name.contains('.') {
        let parts: Vec<_> = name.split('.').collect();
        if parts.iter().all(|part| {
            let mut bytes = part.bytes();
            bytes
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                && bytes.all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }) {
            let mut path = parts[..parts.len() - 1]
                .iter()
                .map(|part| format!("+{part}/"))
                .collect::<String>();
            path.push_str(parts.last().unwrap());
            path.push_str(".m");
            return resolve(&path, &[]);
        }
    }
    resolve(name, extra_suffixes(CompanionKind::HelperM))
}

fn option_kind(command: &str, option: &str) -> Option<CompanionKind> {
    let cmd = command.to_ascii_lowercase();
    let opt = option.to_ascii_lowercase();
    match opt.as_str() {
        "datafile" => Some(CompanionKind::Datafile),
        "file" if cmd == "data" => Some(CompanionKind::Datafile),
        "filename" if cmd == "initval_file" || cmd == "histval_file" => {
            Some(CompanionKind::Datafile)
        }
        "mode_file" => Some(CompanionKind::ModeFile),
        "irf_matching_file" => Some(CompanionKind::IrfMatchingFile),
        "gsa_sample_file" => Some(CompanionKind::Datafile),
        "function" if cmd == "prior_function" || cmd == "posterior_function" => {
            Some(CompanionKind::HelperM)
        }
        "name" | "first_deriv_provided" | "second_deriv_provided" if cmd == "external_function" => {
            Some(CompanionKind::HelperM)
        }
        _ => None,
    }
}

fn option_value(source: &str, val: &Token, kind: CompanionKind) -> Option<(String, Span)> {
    match val.kind {
        TokenKind::String => {
            let raw = val.text(source);
            let name = strip_quotes(raw).to_string();
            if name.is_empty() {
                return None;
            }
            Some((name, val.span))
        }
        TokenKind::Ident => {
            let name = val.text(source);
            if kind == CompanionKind::Datafile && matches!(name, "0" | "1") {
                return None;
            }
            if name.is_empty() {
                return None;
            }
            Some((name.to_string(), val.span))
        }
        TokenKind::Number => {
            let name = val.text(source);
            if kind == CompanionKind::Datafile && matches!(name, "0" | "1") {
                return None;
            }
            None
        }
        _ => None,
    }
}

fn harvest_leftover_quotes(
    source: &str,
    tokens: &[Token],
    option_value_spans: &HashSet<(u32, u32)>,
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
    pending: &mut Vec<Pending>,
) {
    for tok in tokens {
        if tok.kind != TokenKind::String {
            continue;
        }
        if option_value_spans.contains(&(tok.span.start, tok.span.end)) {
            continue;
        }
        let name = strip_quotes(tok.text(source)).to_string();
        let Some(kind) = leftover_kind(&name) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        let path = resolve(&name, &[]);
        pending.push(Pending {
            kind,
            named_in: tok.span,
            name,
            path,
            rank: RANK_LEFTOVER,
        });
    }
}

fn leftover_kind(name: &str) -> Option<CompanionKind> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".m") {
        Some(CompanionKind::HelperM)
    } else if lower.ends_with(".mat")
        || lower.ends_with(".csv")
        || lower.ends_with(".xls")
        || lower.ends_with(".xlsx")
    {
        Some(CompanionKind::Datafile)
    } else {
        None
    }
}

fn harvest_ident_helpers(
    source: &str,
    model: &Model,
    tokens: &[Token],
    cmd_spans: &[Span],
    resolve: &mut impl FnMut(&str, &[&str]) -> Option<PathBuf>,
    pending: &mut Vec<Pending>,
) {
    // Declaration metadata has Ident(...) syntax but is not a native call.
    // Use parser token receipts, not spelling or written spans: macro copies
    // can share a span while a separate call with the same name remains real.
    let declarations: HashSet<_> = model
        .written_declarations
        .iter()
        .filter_map(|declaration| {
            let order = declaration.token_range.start;
            let original = model.expanded_token_origins.get(order)?;
            let parsed = model.expanded_tokens.get(order)?;
            let token = tokens.get(original.start)?;
            (original.len() == 1
                && token.kind == TokenKind::Ident
                && token.span == parsed.span
                && token.text(source) == model.name(declaration.declaration.name))
            .then_some(original.start)
        })
        .collect();
    let mut call_name_spans = HashMap::new();
    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i].kind != TokenKind::Ident
            || i > 0 && tokens[i - 1].kind == TokenKind::Dot
            || declarations.contains(&i)
        {
            continue;
        }
        let (name, named_in, end) = qualified_name(source, tokens, i);
        if tokens
            .get(end + 1)
            .is_some_and(|t| t.kind == TokenKind::LParen)
        {
            call_name_spans.insert((named_in.start, name), named_in);
        }
    }
    let mut calls = Vec::new();
    for a in model
        .param_assignments
        .iter()
        .chain(&model.helper_assignments)
        .chain(&model.initval)
        .chain(&model.endval)
    {
        if let Some(id) = a.expr {
            collect_calls(model, id, &mut calls);
        }
    }
    for eq in &model.steady_state_equations {
        if let Some(id) = eq.lhs_expr {
            collect_calls(model, id, &mut calls);
        }
        if let Some(id) = eq.rhs_expr {
            collect_calls(model, id, &mut calls);
        }
    }
    for (name, named_in) in calls {
        let named_in = call_name_spans
            .get(&(named_in.start, name.clone()))
            .copied()
            .unwrap_or(named_in);
        if let Some(path) = resolve_helper(&name, false, resolve) {
            pending.push(Pending {
                kind: CompanionKind::HelperM,
                named_in,
                name,
                path: Some(path),
                rank: RANK_LEFTOVER,
            });
        }
    }

    for i in 0..tokens.len().saturating_sub(1) {
        if tokens[i].kind != TokenKind::Ident
            || i > 0 && tokens[i - 1].kind == TokenKind::Dot
            || declarations.contains(&i)
        {
            continue;
        }
        let (name, named_in, end) = qualified_name(source, tokens, i);
        if tokens
            .get(end + 1)
            .is_none_or(|t| t.kind != TokenKind::LParen)
        {
            continue;
        }
        let ident = &tokens[i];
        if let Some(block) = model.model_block
            && span_contains(block, ident.span)
        {
            continue;
        }
        if cmd_spans.iter().any(|s| span_contains(*s, ident.span)) {
            continue;
        }
        if is_parse_skip_command(&name) {
            continue;
        }
        if end == i && is_timing_paren(tokens, source, end + 1) {
            continue;
        }
        if name.is_empty() {
            continue;
        }
        if let Some(path) = resolve_helper(&name, false, resolve) {
            pending.push(Pending {
                kind: CompanionKind::HelperM,
                named_in,
                name,
                path: Some(path),
                rank: RANK_LEFTOVER,
            });
        }
    }
}

fn collect_calls(model: &Model, id: ExprId, out: &mut Vec<(String, Span)>) {
    let expr = model.exprs.get(id);
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let name = model.name(*callee).to_string();
            out.push((name, expr.span));
            for arg in args {
                collect_calls(model, *arg, out);
            }
        }
        ExprKind::Unary { arg, .. }
        | ExprKind::SteadyState { arg }
        | ExprKind::Expectation { arg, .. } => {
            collect_calls(model, *arg, out);
        }
        ExprKind::Binary { lhs, rhs, .. } => {
            collect_calls(model, *lhs, out);
            collect_calls(model, *rhs, out);
        }
        ExprKind::Ident { .. }
        | ExprKind::Number
        | ExprKind::String
        | ExprKind::Error
        | ExprKind::PathNamespace { .. } => {}
    }
}

fn is_timing_paren(tokens: &[Token], source: &str, lparen_i: usize) -> bool {
    let mut k = lparen_i + 1;
    if tokens
        .get(k)
        .is_some_and(|t| matches!(t.kind, TokenKind::Plus | TokenKind::Minus))
    {
        k += 1;
    }
    match (tokens.get(k), tokens.get(k + 1)) {
        (Some(num), Some(rp)) if num.kind == TokenKind::Number && rp.kind == TokenKind::RParen => {
            is_integer_lexeme(num.text(source))
        }
        _ => false,
    }
}

fn is_integer_lexeme(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn span_contains(outer: Span, inner: Span) -> bool {
    inner.start >= outer.start && inner.end <= outer.end
}

fn strip_quotes(s: &str) -> &str {
    let b = s.as_bytes();
    if b.len() >= 2
        && ((b[0] == b'\'' && b[b.len() - 1] == b'\'') || (b[0] == b'"' && b[b.len() - 1] == b'"'))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn merge(pending: Vec<Pending>, identity: CompanionIdentity) -> Vec<CompanionRecord> {
    let mut map: HashMap<String, Pending> = HashMap::new();
    for p in pending {
        let key = record_key(&p, identity);
        match map.get(&key) {
            Some(old) if old.rank >= p.rank => {}
            _ => {
                map.insert(key, p);
            }
        }
    }
    let mut out: Vec<CompanionRecord> = map
        .into_values()
        .map(|p| CompanionRecord {
            kind: p.kind,
            named_in: p.named_in,
            name: p.name,
            path: p.path,
        })
        .collect();
    out.sort_by(|a, b| {
        a.named_in
            .start
            .cmp(&b.named_in.start)
            .then(a.named_in.end.cmp(&b.named_in.end))
            .then(a.name.cmp(&b.name))
    });
    out
}

fn record_key(p: &Pending, identity: CompanionIdentity) -> String {
    match &p.path {
        Some(path) => format!(
            "path:{}",
            match identity {
                CompanionIdentity::Supplied => supplied_path_key(&path.to_string_lossy()),
                CompanionIdentity::Host => path_key(path),
            }
        ),
        None => {
            let n = match identity {
                CompanionIdentity::Supplied => supplied_path_key(&p.name),
                CompanionIdentity::Host => {
                    let n = normalize_separators(&p.name);
                    if cfg!(windows) {
                        n.to_ascii_lowercase()
                    } else {
                        n
                    }
                }
            };
            format!("unresolved:{n}")
        }
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::{CompanionIdentity, harvest};
    use crate::engine::language::{
        lexer::{TokenKind, tokenize},
        macro_expand::expand_macros,
        parser::parse,
    };
    use crate::engine::workspace::Workspace;
    use std::collections::BTreeMap;

    #[test]
    fn declaration_metadata_does_not_probe_a_helper_file() {
        // Guards display metadata adding false related files and missing-file
        // inputs that every later semantic request must validate.
        for present in [false, true] {
            let mut files = BTreeMap::from([(
                "root.mod".into(),
                "var sample(long_name='Sample'); model; sample=0; end;".into(),
            )]);
            if present {
                files.insert("sample.m".into(), "function y=sample(x)\ny=x;\nend;".into());
            }
            let mut workspace = Workspace::overlay_documents(&files);
            workspace.input_revision("root.mod").unwrap();
            assert!(
                !workspace
                    .companion_records("root.mod")
                    .unwrap()
                    .iter()
                    .any(|record| record.name == "sample"),
                "metadata became a helper"
            );
            assert!(
                !workspace
                    .input_snapshots
                    .get("root.mod")
                    .expect("captured overlay inputs")
                    .keys()
                    .any(|path| matches!(
                        std::path::Path::new(path)
                            .file_name()
                            .and_then(|name| name.to_str()),
                        Some("sample" | "sample.m")
                    )),
                "metadata added lookup guards"
            );
        }
    }

    #[test]
    fn metadata_filter_keeps_a_real_helper_with_the_same_name() {
        // Guards filtering by spelling rather than a proven declaration site.
        let source =
            "var sample(long_name='Sample'); parameters p; p=sample(.5); model; sample=p; end;";
        let files = BTreeMap::from([
            ("root.mod".into(), source.into()),
            ("sample.m".into(), "function y=sample(x)\ny=x;\nend;".into()),
        ]);
        let mut workspace = Workspace::overlay_documents(&files);
        workspace.input_revision("root.mod").unwrap();
        assert!(
            workspace.input_snapshots["root.mod"].contains_key("sample.m"),
            "real helper lost its file guard"
        );
        let record = workspace
            .companion_records("root.mod")
            .unwrap()
            .iter()
            .find(|record| record.name == "sample")
            .expect("real helper call");
        assert!(record.path.is_some());
        assert_eq!(
            &source[record.named_in.start as usize..record.named_in.end as usize],
            "sample"
        );
        assert_eq!(
            record.named_in.start as usize,
            source.find("sample(.5)").unwrap()
        );
    }

    #[test]
    fn shared_written_span_keeps_native_call_after_a_declaration_copy() {
        // Guards a span-based exclusion removing a real native call when a
        // macro loop gives the same written Ident(...) two different roles.
        let source = "@#for declare in [1,0]\n@#if declare\nvar\n@#endif\nsample(long_name='Sample');\n@#endfor\nmodel; sample=0; end;\n";
        let model = parse(source);
        let tokens = expand_macros(source, tokenize(source));
        let copies: Vec<_> = tokens
            .windows(2)
            .filter(|pair| {
                pair[0].kind == TokenKind::Ident
                    && pair[0].text(source) == "sample"
                    && pair[1].kind == TokenKind::LParen
            })
            .map(|pair| pair[0].span)
            .collect();
        assert_eq!(copies.len(), 2);
        assert_eq!(copies[0], copies[1], "fixture must share the written span");
        let mut probes = Vec::new();
        let records = harvest(
            source,
            &model,
            &[],
            CompanionIdentity::Supplied,
            |name, _| {
                probes.push(name.to_owned());
                Some(std::path::PathBuf::from("sample.m"))
            },
        );
        assert_eq!(
            probes,
            ["sample"],
            "only the native occurrence may probe a helper"
        );
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name, "sample");
    }
}
