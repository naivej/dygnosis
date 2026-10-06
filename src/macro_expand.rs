//! Native `@#define` / `@#if` / `@#for` / `@{NAME}` expansion over the token stream.

use std::collections::{HashMap, HashSet};

use crate::lexer::{Token, TokenKind};
use crate::span::Span;

const RANGE_CAP: usize = 10_000;
const MACRO_DEPTH_CAP: usize = 32;

type MacroTypeError = (Span, &'static str, String);
type ExpandTracedFull = (
    Vec<Token>,
    Vec<TokenTrace>,
    Vec<FrameRec>,
    Vec<MacroTypeError>,
    Vec<Span>,
    Option<Span>,
    Vec<IncompleteReason>,
    bool,
);

/// One verified incomplete-expansion failure for diagnostics and status hover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncompleteReason {
    pub span: Span,
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug)]
enum MacroVal {
    Int(i64),
    Real(f64),
    Bool(bool),
    Range { start: i64, end: i64 },
    Text(String),
    Tuple(Vec<MacroVal>),
    Array(Vec<MacroVal>),
    Function { params: Vec<String>, body: String },
    Unresolved,
}

impl MacroVal {
    fn display(&self) -> String {
        match self {
            MacroVal::Int(n) => n.to_string(),
            MacroVal::Real(n) => n.to_string(),
            MacroVal::Bool(b) => b.to_string(),
            MacroVal::Range { start, end } => format!("{start}:{end}"),
            MacroVal::Text(s) => s.clone(),
            MacroVal::Tuple(values) => format!(
                "({})",
                values
                    .iter()
                    .map(MacroVal::display)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            MacroVal::Array(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(MacroVal::display)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            MacroVal::Function { .. } => String::new(),
            MacroVal::Unresolved => String::new(),
        }
    }

    fn loop_values(&self) -> Option<Vec<MacroVal>> {
        match self {
            MacroVal::Range { start, end }
                if (*end as i128 - *start as i128 + 1) <= RANGE_CAP as i128 =>
            {
                Some(
                    inclusive_range(*start, *end)
                        .into_iter()
                        .map(MacroVal::Int)
                        .collect(),
                )
            }
            MacroVal::Array(values) if values.len() <= RANGE_CAP => Some(values.clone()),
            _ => None,
        }
    }

    fn condition(&self) -> Option<bool> {
        match self {
            MacroVal::Bool(b) => Some(*b),
            MacroVal::Int(n) => Some(*n != 0),
            MacroVal::Real(n) => Some(*n != 0.0),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum MacroEvalError {
    UnknownVariable(String),
    UnknownFunction(String),
    TypeMismatch(&'static str),
    /// Pinned refusal for a proven non-array/tuple right operand of `in`.
    InOperandType,
    MissingInOperand,
    SyntaxEol,
    SyntaxUnexpected(&'static str),
    Unsupported,
    /// Local evaluator resource limit (depth or collection size).
    Limit(&'static str),
    /// An earlier failed definition already owns the reason; withhold quietly.
    PriorFailure,
}

impl MacroEvalError {
    fn at_end(self, token: &'static str) -> Self {
        if matches!(self, Self::MissingInOperand) {
            Self::SyntaxUnexpected(token)
        } else {
            self
        }
    }

    fn diagnostic(&self) -> Option<(&'static str, String)> {
        match self {
            Self::UnknownVariable(name) => Some(("E063", format!("Unknown variable {name}"))),
            Self::UnknownFunction(name) => Some(("E063", format!("Unknown function {name}"))),
            Self::TypeMismatch(op) => Some((
                "E285",
                format!("Type mismatch for operands of {op} operator"),
            )),
            Self::InOperandType => Some((
                "E285",
                "Second argument of `in` operator must be an array".to_string(),
            )),
            Self::SyntaxEol => Some(("E062", "syntax error, unexpected EOL".to_string())),
            Self::MissingInOperand => Some(("E062", "syntax error, unexpected EOL".to_string())),
            Self::SyntaxUnexpected(token) => {
                Some(("E062", format!("syntax error, unexpected {token}")))
            }
            Self::Unsupported | Self::Limit(_) | Self::PriorFailure => None,
        }
    }
}

fn i211_expression_message(expression: &str) -> String {
    let expression = expression.trim();
    if expression.is_empty() {
        "Macro expansion is incomplete; some model checks were withheld.".to_string()
    } else {
        format!(
            "Macro expression '{expression}' could not be evaluated; some model checks were withheld."
        )
    }
}

fn i211_limit_message(limit: &str) -> String {
    format!("Macro expansion stopped at the {limit} limit; some model checks were withheld.")
}

fn push_type_error(
    state: &mut ExpandState<'_, '_>,
    span: Span,
    code: &'static str,
    message: String,
) {
    if !state
        .type_errors
        .iter()
        .any(|(existing, existing_code, existing_message)| {
            *existing == span && *existing_code == code && existing_message == &message
        })
    {
        state.type_errors.push((span, code, message));
    }
    state.incomplete.get_or_insert(span);
}

fn push_i211(state: &mut ExpandState<'_, '_>, span: Span, message: String) {
    if !state
        .incomplete_reasons
        .iter()
        .any(|reason| reason.span == span && reason.code == "I211" && reason.message == message)
    {
        state.incomplete_reasons.push(IncompleteReason {
            span,
            code: "I211",
            message,
        });
    }
    state.incomplete.get_or_insert(span);
}

fn note_eval_failure(
    state: &mut ExpandState<'_, '_>,
    span: Span,
    error: MacroEvalError,
    expression: &str,
) {
    if let Some((code, message)) = error.diagnostic() {
        push_type_error(state, span, code, message);
        return;
    }
    match error {
        MacroEvalError::Limit(limit) => push_i211(state, span, i211_limit_message(limit)),
        MacroEvalError::PriorFailure => {
            state.incomplete.get_or_insert(span);
        }
        MacroEvalError::Unsupported => {
            push_i211(state, span, i211_expression_message(expression));
        }
        _ => {
            state.incomplete.get_or_insert(span);
        }
    }
}

fn oversized_collection(value: &MacroVal) -> bool {
    match value {
        MacroVal::Range { start, end } => (*end as i128 - *start as i128 + 1) > RANGE_CAP as i128,
        MacroVal::Array(values) | MacroVal::Tuple(values) => values.len() > RANGE_CAP,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Define,
    Ifdef,
    Ifndef,
    If,
    Elseif,
    Else,
    Endif,
    For,
    Endfor,
    Unknown,
}

struct IfFrame {
    /// This branch of the chain is the one being emitted.
    active: bool,
    /// An earlier branch in this chain was selected, so later clauses stay inactive.
    taken: bool,
    frame_id: usize,
    body_start: u32,
    /// First byte of this branch, just after the directive that opened it.
    branch_start: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct TokenTrace {
    pub frames: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct FrameRec {
    pub kind: &'static str,
    pub directive_span: Span,
    pub body_span: Span,
    /// Loop index name. Set on a `@#for` iteration frame.
    pub variable: Option<String>,
    /// Loop index value for this iteration, as written after substitution.
    pub value: Option<String>,
}

/// Display fragment for the source-layout preview. The preview publishes
/// `source_navigation` from these records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceFragment {
    pub display: Span,
    /// Span in the expander source (include-spliced text).
    pub written: Option<Span>,
    pub kind: SourceFragmentKind,
    /// Emitted while a macro frame was on the origin stack.
    pub macro_active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceFragmentKind {
    Copy,
    Substitution,
}

/// Leftover bytes from an include directive line in the spliced expander source.
///
/// The splice keeps the written indent and the directive's trailing newline so
/// the token stream stays unchanged. Source layout omits the indent, and either
/// omits the newline (body already ended a line) or emits it with no written
/// target (body has no final newline).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceLayoutGap {
    Omit(Span),
    SyntheticNewline(Span),
}

impl SourceLayoutGap {
    pub(crate) fn span(self) -> Span {
        match self {
            Self::Omit(span) | Self::SyntheticNewline(span) => span,
        }
    }

    pub(crate) fn shift(self, offset: u32) -> Self {
        let span = self.span();
        let shifted = Span {
            start: span.start + offset,
            end: span.end + offset,
        };
        match self {
            Self::Omit(_) => Self::Omit(shifted),
            Self::SyntheticNewline(_) => Self::SyntheticNewline(shifted),
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SourceLayoutBuild {
    pub text: String,
    pub fragments: Vec<SourceFragment>,
}

struct SourceRecorder<'a> {
    src: &'a str,
    text: String,
    fragments: Vec<SourceFragment>,
    cursor: u32,
    suppress: bool,
    gaps: &'a [SourceLayoutGap],
}

impl<'a> SourceRecorder<'a> {
    fn new(src: &'a str, gaps: &'a [SourceLayoutGap]) -> Self {
        Self {
            src,
            text: String::with_capacity(src.len()),
            fragments: Vec::new(),
            cursor: 0,
            suppress: false,
            gaps,
        }
    }

    fn finish(self) -> SourceLayoutBuild {
        SourceLayoutBuild {
            text: self.text,
            fragments: self.fragments,
        }
    }

    fn skip_directive(&mut self, dir: Span, copy_leading_gap: bool, macro_active: bool) {
        let line = directive_line_extent(self.src, dir);
        if copy_leading_gap && self.cursor < line.start {
            self.copy_range(self.cursor, line.start, macro_active);
        }
        self.cursor = line.end;
    }

    fn copy_through_token(&mut self, span: Span, macro_active: bool) {
        let end = span.end;
        if self.cursor <= span.start && (end as usize) <= self.src.len() {
            self.copy_range(self.cursor, end, macro_active);
        } else if (span.start as usize) < self.src.len() && (end as usize) <= self.src.len() {
            self.copy_range(span.start, end, macro_active);
            self.cursor = end;
        }
    }

    fn substitute(&mut self, written: Span, text: &str, macro_active: bool) {
        if self.cursor < written.start {
            self.copy_range(self.cursor, written.start, macro_active);
        }
        let start = self.text.len() as u32;
        self.text.push_str(text);
        self.fragments.push(SourceFragment {
            display: Span {
                start,
                end: self.text.len() as u32,
            },
            written: Some(written),
            kind: SourceFragmentKind::Substitution,
            macro_active,
        });
        self.cursor = written.end;
    }

    fn copy_range(&mut self, start: u32, end: u32, macro_active: bool) {
        if end <= start || (end as usize) > self.src.len() {
            return;
        }
        let mut pos = start;
        while pos < end {
            if let Some(gap) = self.gaps.iter().find(|gap| gap.span().start == pos) {
                match gap {
                    SourceLayoutGap::Omit(span) => {
                        pos = span.end.min(end);
                    }
                    SourceLayoutGap::SyntheticNewline(span) => {
                        let display_start = self.text.len() as u32;
                        self.text.push('\n');
                        self.fragments.push(SourceFragment {
                            display: Span {
                                start: display_start,
                                end: self.text.len() as u32,
                            },
                            written: None,
                            kind: SourceFragmentKind::Copy,
                            macro_active,
                        });
                        pos = span.end.min(end);
                    }
                }
                continue;
            }
            let next_gap = self
                .gaps
                .iter()
                .map(|gap| gap.span().start)
                .filter(|&gap_start| gap_start > pos && gap_start < end)
                .min()
                .unwrap_or(end);
            self.copy_range_plain(pos, next_gap, macro_active);
            pos = next_gap;
        }
        self.cursor = end;
    }

    fn copy_range_plain(&mut self, start: u32, end: u32, macro_active: bool) {
        if end <= start || (end as usize) > self.src.len() {
            return;
        }
        let piece = &self.src[start as usize..end as usize];
        if piece.is_empty() {
            return;
        }
        // Keep each collapsed CRLF separate. Length alone cannot locate CRLF
        // among mixed line endings or split a copy at an included-file boundary.
        let mut cursor = start;
        for (offset, _) in piece.match_indices("\r\n") {
            let newline = start + offset as u32;
            self.push_copy(cursor, newline, macro_active);
            self.push_copy(newline, newline + 2, macro_active);
            cursor = newline + 2;
        }
        self.push_copy(cursor, end, macro_active);
    }

    fn push_copy(&mut self, start: u32, end: u32, macro_active: bool) {
        let piece = &self.src[start as usize..end as usize];
        let normalized = piece.replace("\r\n", "\n").replace('\r', "\n");
        if normalized.is_empty() {
            return;
        }
        let display_start = self.text.len() as u32;
        self.text.push_str(&normalized);
        self.fragments.push(SourceFragment {
            display: Span {
                start: display_start,
                end: self.text.len() as u32,
            },
            written: Some(Span { start, end }),
            kind: SourceFragmentKind::Copy,
            macro_active,
        });
    }

    fn begin_loop_body(&mut self, body_start: u32) {
        self.cursor = body_start;
    }

    fn finish_loop_body(&mut self, body_end: u32, macro_active: bool) {
        if self.cursor < body_end {
            self.copy_range(self.cursor, body_end, macro_active);
        }
    }
}

/// Line occupied by a macro directive: indent, directive text, and trailing `\n`.
pub(crate) fn directive_line_extent(src: &str, dir: Span) -> Span {
    let mut start = dir.start as usize;
    while start > 0 && matches!(src.as_bytes()[start - 1], b' ' | b'\t') {
        start -= 1;
    }
    let mut end = dir.end as usize;
    if end < src.len() && src.as_bytes()[end] == b'\n' {
        end += 1;
    }
    Span::new(start, end)
}

struct ExpandState<'src, 'w> {
    src: &'src str,
    defines: &'w mut HashMap<String, MacroVal>,
    origin_stack: Vec<usize>,
    arena: &'w mut Vec<FrameRec>,
    type_errors: &'w mut Vec<MacroTypeError>,
    discarded: &'w mut Vec<Span>,
    incomplete: &'w mut Option<Span>,
    incomplete_reasons: &'w mut Vec<IncompleteReason>,
    include_seen: bool,
    file_visitor: Option<&'w mut dyn MacroFileVisitor>,
    source: Option<&'w mut SourceRecorder<'src>>,
}

trait MacroFileVisitor {
    fn visit(&mut self, span: Span, defines: &mut HashMap<String, MacroVal>, certain: bool)
        -> bool;
    fn path(&mut self, span: Span, path: &str, certain: bool) -> bool;
}

pub(crate) enum MacroFileDirective<'a> {
    Include,
    IncludePath(&'a str),
}

pub(crate) enum MacroFileLoad {
    Source { file: String, source: String },
    Path,
}

/// Executed directive with the caller chain needed for written error locations.
pub(crate) struct MacroFileEvent<'a> {
    pub file: &'a str,
    pub span: Span,
    pub parents: &'a [(String, Span)],
    pub directive: MacroFileDirective<'a>,
    /// Earlier unsupported execution may have changed definitions/search paths.
    pub certain: bool,
}

type MacroFileLoader<'a> = dyn FnMut(MacroFileEvent<'_>) -> Option<MacroFileLoad> + 'a;

struct MacroFiles<'a> {
    load: &'a mut MacroFileLoader<'a>,
    files: Vec<String>,
    parents: Vec<(String, Span)>,
    sites: HashSet<(String, Span)>,
}

pub(crate) struct MacroFileProof {
    pub complete: bool,
    pub sites: HashSet<(String, Span)>,
}

impl MacroFileVisitor for MacroFiles<'_> {
    fn visit(
        &mut self,
        span: Span,
        defines: &mut HashMap<String, MacroVal>,
        certain: bool,
    ) -> bool {
        let file = self.files.last().expect("macro root").clone();
        self.sites.insert((file.clone(), span));
        let Some(MacroFileLoad::Source {
            file: target,
            source,
        }) = (self.load)(MacroFileEvent {
            file: &file,
            span,
            parents: &self.parents,
            directive: MacroFileDirective::Include,
            certain,
        })
        else {
            return false;
        };
        if self.files.contains(&target) {
            return false;
        }
        self.parents.push((file, span));
        self.files.push(target);
        let complete = macro_file_complete(&source, defines, self);
        self.files.pop();
        self.parents.pop();
        complete
    }

    fn path(&mut self, span: Span, path: &str, certain: bool) -> bool {
        matches!(
            (self.load)(MacroFileEvent {
                file: self.files.last().expect("macro root"),
                span,
                parents: &self.parents,
                directive: MacroFileDirective::IncludePath(path),
                certain,
            }),
            Some(MacroFileLoad::Path)
        )
    }
}

/// Visit original files in macro execution order, sharing definitions and the
/// existing branch/loop evaluator. Dormant directives never call the loader.
pub(crate) fn walk_macro_files(
    root: &str,
    source: &str,
    mut load: impl FnMut(MacroFileEvent<'_>) -> Option<MacroFileLoad>,
) -> MacroFileProof {
    let mut includes = MacroFiles {
        load: &mut load,
        files: vec![root.to_string()],
        parents: Vec::new(),
        sites: HashSet::new(),
    };
    let complete = macro_file_complete(source, &mut HashMap::new(), &mut includes);
    MacroFileProof {
        complete,
        sites: includes.sites,
    }
}

fn macro_file_complete(
    text: &str,
    defines: &mut HashMap<String, MacroVal>,
    includes: &mut dyn MacroFileVisitor,
) -> bool {
    let source = crate::parser::normalize_newlines(text);
    let tokens = crate::lexer::tokenize(&source);
    if !macro_blocks_complete(&source, &tokens) {
        return false;
    }
    let mut arena = Vec::new();
    let mut errors = Vec::new();
    let mut discarded = Vec::new();
    let mut incomplete = None;
    let mut incomplete_reasons = Vec::new();
    let mut state = ExpandState {
        src: &source,
        defines,
        origin_stack: Vec::new(),
        arena: &mut arena,
        type_errors: &mut errors,
        discarded: &mut discarded,
        incomplete: &mut incomplete,
        incomplete_reasons: &mut incomplete_reasons,
        include_seen: false,
        file_visitor: Some(includes),
        source: None,
    };
    expand_seq(&mut state, &tokens);
    incomplete.is_none() && errors.is_empty()
}

pub fn expand_macros(src: &str, tokens: Vec<Token>) -> Vec<Token> {
    expand_macros_full(src, tokens).0
}

pub fn expand_macros_full(
    src: &str,
    tokens: Vec<Token>,
) -> (Vec<Token>, Vec<(Span, &'static str, String)>) {
    let (out, _, _, errors, _, _, _, _) = expand_macros_traced_full(src, tokens);
    (out, errors)
}

pub(crate) fn expand_macros_with_status(
    src: &str,
    tokens: Vec<Token>,
) -> (
    Vec<Token>,
    Vec<MacroTypeError>,
    Option<Span>,
    Vec<IncompleteReason>,
) {
    let (out, _, _, errors, _, incomplete, reasons, _) = expand_macros_traced_full(src, tokens);
    (out, errors, incomplete, reasons)
}

pub(crate) fn expand_macros_traced_with_status(
    src: &str,
    tokens: Vec<Token>,
) -> (Vec<Token>, Vec<TokenTrace>, Vec<FrameRec>, bool, bool) {
    // Check the original stream, including bodies skipped by inactive branches
    // or empty loops. This metadata proof does not change expansion or errors.
    let blocks_complete = macro_blocks_complete(src, &tokens);
    let (out, traces, arena, errors, _, incomplete, _, _) = expand_macros_traced_full(src, tokens);
    // Some existing macro checks record an error while still unrolling. Keep
    // legacy incomplete unchanged, but consume that same error proof for jumps.
    (
        out,
        traces,
        arena,
        incomplete.is_some(),
        blocks_complete && errors.is_empty(),
    )
}

/// Expand while recording a source-layout display copy. The token stream matches
/// ordinary expansion; fragments are a side structure for the editor preview.
///
/// `gaps` marks leftover include-directive indent and line endings in an
/// already spliced `src`. Pass an empty slice when there are no active includes.
pub(crate) fn expand_macros_with_source_layout(
    src: &str,
    tokens: Vec<Token>,
    gaps: &[SourceLayoutGap],
) -> (
    Vec<Token>,
    Vec<MacroTypeError>,
    Option<Span>,
    SourceLayoutBuild,
) {
    let mut recorder = SourceRecorder::new(src, gaps);
    let (out, type_errors, incomplete) = {
        let mut defines = HashMap::new();
        let mut arena = Vec::new();
        let mut type_errors = Vec::new();
        let mut discarded = Vec::new();
        let mut incomplete = None;
        let mut incomplete_reasons = Vec::new();
        let mut state = ExpandState {
            src,
            defines: &mut defines,
            origin_stack: Vec::new(),
            arena: &mut arena,
            type_errors: &mut type_errors,
            discarded: &mut discarded,
            incomplete: &mut incomplete,
            incomplete_reasons: &mut incomplete_reasons,
            include_seen: false,
            file_visitor: None,
            source: Some(&mut recorder),
        };
        let (out, _) = expand_seq(&mut state, &tokens);
        (out, type_errors, incomplete)
    };
    if (recorder.cursor as usize) < src.len() {
        let end = src.len() as u32;
        let start = recorder.cursor;
        recorder.copy_range(start, end, false);
    }
    (out, type_errors, incomplete, recorder.finish())
}

fn macro_blocks_complete(src: &str, tokens: &[Token]) -> bool {
    let mut stack = Vec::new();
    for token in tokens {
        if token.kind != TokenKind::MacroDir {
            continue;
        }
        match dir_kind(src, token) {
            Dir::If | Dir::Ifdef | Dir::Ifndef => stack.push((Dir::If, false, token.span)),
            Dir::For => stack.push((Dir::For, false, token.span)),
            Dir::Endif => {
                if stack.pop().map(|(kind, _, _)| kind) != Some(Dir::If) {
                    return false;
                }
            }
            Dir::Endfor => {
                let Some((Dir::For, _, opener)) = stack.pop() else {
                    return false;
                };
                if for_body_is_empty(src, opener, token.span) {
                    return false;
                }
            }
            kind @ (Dir::Elseif | Dir::Else) => {
                let Some((Dir::If, seen_else, _)) = stack.last_mut() else {
                    return false;
                };
                if *seen_else {
                    return false;
                }
                *seen_else = kind == Dir::Else;
            }
            _ => {}
        }
    }
    stack.is_empty()
}

/// The macro parser needs a statement between `for` and `endfor`. A blank
/// line or comment is a text statement, even when .mod tokenization skips it.
pub(crate) fn for_body_is_empty(source: &str, opener: Span, closer: Span) -> bool {
    let Some(gap) = source.get(opener.end as usize..closer.start as usize) else {
        return false;
    };
    let body = gap
        .strip_prefix("\r\n")
        .or_else(|| gap.strip_prefix('\n'))
        .unwrap_or(gap);
    body.chars().all(|ch| matches!(ch, ' ' | '\t'))
}

/// Source ranges of `@#if` / `@#ifndef` branches that expansion discarded.
pub(crate) fn inactive_macro_spans(src: &str) -> Vec<Span> {
    let tokens = crate::lexer::tokenize(src);
    let (_, _, _, _, discarded, _, _, _) = expand_macros_traced_full(src, tokens);
    discarded
}

/// Executed path directives in an already joined source. This uses the same
/// branch/loop walker as file loading; empty loops never visit their body.
pub(crate) fn executed_includepaths(src: &str) -> Vec<(Span, String)> {
    let mut paths = Vec::new();
    walk_macro_files("<joined>", src, |event| {
        if let MacroFileDirective::IncludePath(path) = event.directive {
            if event.certain {
                paths.push((event.span, path.to_string()));
            }
            return Some(MacroFileLoad::Path);
        }
        None
    });
    paths
}

pub(crate) fn has_include_directives(src: &str) -> bool {
    let source = crate::parser::normalize_newlines(src);
    crate::lexer::tokenize(&source).iter().any(|token| {
        token.kind == TokenKind::MacroDir
            && directive_name(token.text(&source)).eq_ignore_ascii_case("include")
    })
}

/// Proof for a metadata splice whose unresolved/cyclic directives remain in
/// place. The visitor already observes executed includes; normal output is unchanged.
pub(crate) fn required_includes_complete(src: &str) -> bool {
    let source = crate::parser::normalize_newlines(src);
    if !has_include_directives(&source) {
        return true;
    }
    let (_, _, _, _, _, incomplete, _, include_seen) =
        expand_macros_traced_full(&source, crate::lexer::tokenize(&source));
    !include_seen && incomplete.is_none()
}

fn expand_macros_traced_full(src: &str, tokens: Vec<Token>) -> ExpandTracedFull {
    let mut defines = HashMap::new();
    let mut arena = Vec::new();
    let mut type_errors = Vec::new();
    let mut discarded = Vec::new();
    let mut incomplete = None;
    let mut incomplete_reasons = Vec::new();
    let (out, traces, include_seen) = {
        let mut state = ExpandState {
            src,
            defines: &mut defines,
            origin_stack: Vec::new(),
            arena: &mut arena,
            type_errors: &mut type_errors,
            discarded: &mut discarded,
            incomplete: &mut incomplete,
            incomplete_reasons: &mut incomplete_reasons,
            include_seen: false,
            file_visitor: None,
            source: None,
        };
        let (out, traces) = expand_seq(&mut state, &tokens);
        (out, traces, state.include_seen)
    };
    (
        out,
        traces,
        arena,
        type_errors,
        discarded,
        incomplete,
        incomplete_reasons,
        include_seen,
    )
}

fn expand_seq(state: &mut ExpandState<'_, '_>, tokens: &[Token]) -> (Vec<Token>, Vec<TokenTrace>) {
    let mut out = Vec::new();
    let mut traces = Vec::new();
    let mut i = 0;
    let mut stack: Vec<IfFrame> = Vec::new();

    while i < tokens.len() {
        let tok = &tokens[i];
        if tok.kind == TokenKind::Eof {
            emit(state, &mut out, &mut traces, tok.clone());
            break;
        }
        if tok.kind == TokenKind::MacroDir {
            match dir_kind(state.src, tok) {
                Dir::Define => {
                    let em = emitting(&stack);
                    let mut kept = false;
                    if em {
                        match parse_define_eval(tok.text(state.src), state.defines) {
                            Ok(Some((name, val))) => {
                                state.defines.insert(name, val);
                            }
                            Ok(None) => {}
                            Err(error) => {
                                if let Some(name) = defined_name(tok.text(state.src)) {
                                    state.defines.insert(name, MacroVal::Unresolved);
                                }
                                let expression = define_rhs_expression(tok.text(state.src));
                                note_eval_failure(state, tok.span, error, expression);
                                emit(state, &mut out, &mut traces, tok.clone());
                                kept = true;
                            }
                        }
                    }
                    if !kept {
                        record_source_directive(state, tok.span, em);
                    }
                    i += 1;
                }
                Dir::Ifdef => {
                    let em = emitting(&stack);
                    let cond = name_is_defined(state, tok, "ifdef");
                    i += 1;
                    push_if_frame(state, &mut stack, tokens, i, tok.span, "ifdef", cond);
                    record_source_directive(state, tok.span, em);
                }
                Dir::Ifndef => {
                    let em = emitting(&stack);
                    let cond = match dir_arg_ident(tok.text(state.src), "ifndef") {
                        Some(name) => !state.defines.contains_key(&name),
                        None => false,
                    };
                    i += 1;
                    push_if_frame(state, &mut stack, tokens, i, tok.span, "ifndef", cond);
                    record_source_directive(state, tok.span, em);
                }
                Dir::If => {
                    let em = emitting(&stack);
                    let cond = if em {
                        eval_condition(state, tok, "if")
                    } else {
                        Some(false)
                    };
                    if let Some(cond) = cond {
                        i += 1;
                        push_if_frame(state, &mut stack, tokens, i, tok.span, "if", cond);
                        record_source_directive(state, tok.span, em);
                    } else {
                        let next = take_if_end(state.src, tokens, i);
                        retain_raw_macro(state, &tokens[i..next], &mut out, &mut traces);
                        i = next;
                    }
                }
                Dir::Elseif => {
                    let boundary = tok.span;
                    let em = emitting(&stack);
                    let outer_emitting = stack
                        .get(..stack.len().saturating_sub(1))
                        .is_some_and(|outer| outer.iter().all(|frame| frame.active));
                    let active = match stack.last() {
                        Some(frame) if !frame.taken && outer_emitting => {
                            eval_condition(state, tok, "elseif")
                        }
                        _ => Some(false),
                    };
                    let Some(active) = active else {
                        let next = take_if_end(state.src, tokens, i);
                        retain_raw_macro(state, &tokens[i..next], &mut out, &mut traces);
                        if let Some(frame) = stack.pop() {
                            backpatch(
                                state.arena,
                                frame.frame_id,
                                frame.body_start,
                                boundary.start,
                            );
                            state.origin_stack.pop();
                        }
                        i = next;
                        continue;
                    };
                    i += 1;
                    if let Some(frame) = stack.last_mut() {
                        frame.taken = frame.taken || active;
                        open_next_branch(state, frame, tokens, i, boundary, "elseif", active);
                    }
                    record_source_directive(state, boundary, em);
                }
                Dir::Else => {
                    let boundary = tok.span;
                    let em = emitting(&stack);
                    i += 1;
                    if let Some(frame) = stack.last_mut() {
                        let active = !frame.taken;
                        frame.taken = true;
                        open_next_branch(state, frame, tokens, i, boundary, "else", active);
                    }
                    record_source_directive(state, boundary, em);
                }
                Dir::Endif => {
                    let end_span = tok.span;
                    let em = emitting(&stack);
                    i += 1;
                    if let Some(frame) = stack.pop() {
                        if !frame.active {
                            push_discarded(state, frame.branch_start, end_span.start);
                        }
                        backpatch(
                            state.arena,
                            frame.frame_id,
                            frame.body_start,
                            end_span.start,
                        );
                        state.origin_stack.pop();
                    }
                    record_source_directive(state, end_span, em);
                }
                Dir::For => {
                    let em = emitting(&stack);
                    let (body, next) = take_for_body(state.src, tokens, i);
                    if em {
                        check_for_tuple(state, tok);
                        if !unroll_for(state, tok, body, &tokens[i..next], &mut out, &mut traces) {
                            state.incomplete.get_or_insert(tok.span);
                            for original in &tokens[i..next] {
                                emit(state, &mut out, &mut traces, original.clone());
                            }
                        }
                    } else {
                        record_source_directive(state, tok.span, false);
                        if next > i + 1 {
                            let endfor = tokens[next - 1].span;
                            record_source_directive(state, endfor, false);
                        }
                    }
                    i = next;
                }
                Dir::Endfor | Dir::Unknown => {
                    let em = emitting(&stack);
                    let mut kept = false;
                    if directive_name(tok.text(state.src)).eq_ignore_ascii_case("include") && em {
                        if let Some(visitor) = state.file_visitor.as_deref_mut() {
                            if !visitor.visit(tok.span, state.defines, state.incomplete.is_none()) {
                                state.incomplete.get_or_insert(tok.span);
                            }
                        } else {
                            state.include_seen = true;
                        }
                    } else if directive_name(tok.text(state.src))
                        .eq_ignore_ascii_case("includepath")
                        && em
                    {
                        if let Some(path) = eval_includepath(state, tok) {
                            if let Some(visitor) = state.file_visitor.as_deref_mut() {
                                if !visitor.path(tok.span, &path, state.incomplete.is_none()) {
                                    state.incomplete.get_or_insert(tok.span);
                                }
                            }
                        } else {
                            state.incomplete.get_or_insert(tok.span);
                            emit(state, &mut out, &mut traces, tok.clone());
                            kept = true;
                        }
                    }
                    if !kept {
                        record_source_directive(state, tok.span, em);
                    }
                    i += 1;
                }
            }
            continue;
        }
        if tok.kind == TokenKind::MacroInterp
            || (tok.kind == TokenKind::String && tok.text(state.src).contains("@{"))
        {
            if emitting(&stack) {
                let replacements = if tok.kind == TokenKind::String {
                    subst_quoted(state.src, tok, state.defines)
                        .map(|expansion| (vec![expansion.token], expansion.substitutions))
                } else {
                    subst_interp(state.src, tok, state.defines)
                        .map_err(|error| (tok.span, error))
                        .map(|(display, tokens)| (tokens, vec![(tok.span, display)]))
                };
                match replacements {
                    Ok((replacements, substitutions)) => {
                        let macro_active = !state.origin_stack.is_empty();
                        if let Some(recorder) = state.source.as_deref_mut() {
                            for (span, display) in substitutions {
                                recorder.substitute(span, &display, macro_active);
                            }
                            recorder.copy_range(recorder.cursor, tok.span.end, macro_active);
                        }
                        if let Some(recorder) = state.source.as_deref_mut() {
                            recorder.suppress = true;
                        }
                        for replacement in replacements {
                            emit(state, &mut out, &mut traces, replacement);
                        }
                        if let Some(recorder) = state.source.as_deref_mut() {
                            recorder.suppress = false;
                        }
                    }
                    Err((span, error)) => {
                        if state.include_seen
                            && matches!(
                                error,
                                MacroEvalError::UnknownVariable(_)
                                    | MacroEvalError::UnknownFunction(_)
                            )
                        {
                            state.incomplete.get_or_insert(span);
                        } else {
                            let expression = interp_expression(state.src, span);
                            let error = error.at_end("END_EVAL");
                            note_eval_failure(state, span, error, &expression);
                        }
                        emit(state, &mut out, &mut traces, tok.clone());
                    }
                }
            }
            i += 1;
            continue;
        }
        if emitting(&stack) {
            emit(state, &mut out, &mut traces, tok.clone());
        }
        i += 1;
    }
    while let Some(frame) = stack.pop() {
        if !frame.active {
            let end = tokens
                .last()
                .map(|tok| tok.span.end)
                .unwrap_or(frame.branch_start);
            push_discarded(state, frame.branch_start, end);
        }
        state.origin_stack.pop();
    }
    debug_assert_eq!(out.len(), traces.len());
    (out, traces)
}

fn push_discarded(state: &mut ExpandState<'_, '_>, start: u32, end: u32) {
    if end > start {
        state.discarded.push(Span { start, end });
    }
}

fn emit(
    state: &mut ExpandState<'_, '_>,
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
    tok: Token,
) {
    record_source_emit(state, &tok);
    if let Some(prev) = out.last() {
        if let Some(merged) = merge_adjacent(state.src, prev, &tok) {
            *out.last_mut().expect("token just read") = merged;
            return;
        }
    }
    traces.push(TokenTrace {
        frames: state.origin_stack.clone(),
    });
    out.push(tok);
}

fn record_source_emit(state: &mut ExpandState<'_, '_>, tok: &Token) {
    if state.source.is_none() {
        return;
    }
    let suppress = state
        .source
        .as_ref()
        .is_some_and(|recorder| recorder.suppress);
    if suppress || tok.kind == TokenKind::Eof {
        return;
    }
    let macro_active = !state.origin_stack.is_empty();
    let text = tok.text(state.src).to_string();
    let span = tok.span;
    let has_lexeme = tok.lexeme.is_some();
    let recorder = state.source.as_deref_mut().expect("checked above");
    if has_lexeme {
        recorder.substitute(span, &text, macro_active);
    } else {
        recorder.copy_through_token(span, macro_active);
    }
}

fn record_source_directive(state: &mut ExpandState<'_, '_>, dir: Span, copy_leading_gap: bool) {
    let macro_active = !state.origin_stack.is_empty();
    if let Some(recorder) = state.source.as_deref_mut() {
        recorder.skip_directive(dir, copy_leading_gap, macro_active);
    }
}

/// Glue `x@{i}` into one identifier when the pieces touch in the source.
///
/// Dynare substitutes `@{…}` as text before it lexes, so `x@{i}` with `i = 1`
/// is the identifier `x1`. A space, or a join that is not an identifier, stays
/// two tokens.
fn merge_adjacent(src: &str, prev: &Token, next: &Token) -> Option<Token> {
    if !matches!(prev.kind, TokenKind::Ident | TokenKind::Number)
        || !matches!(next.kind, TokenKind::Ident | TokenKind::Number)
    {
        return None;
    }
    if prev.span.end != next.span.start {
        return None;
    }
    let combined = format!("{}{}", prev.text(src), next.text(src));
    if !is_dynare_ident(&combined) {
        return None;
    }
    let mut merged = Token::with_lexeme(
        TokenKind::Ident,
        Span {
            start: prev.span.start,
            end: next.span.end,
        },
        combined,
    );
    merged.expanded_adjacent_next = next.expanded_adjacent_next;
    Some(merged)
}

fn is_dynare_ident(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn emitting(stack: &[IfFrame]) -> bool {
    stack.iter().all(|f| f.active)
}

fn next_body_start(tokens: &[Token], i: usize, fallback: u32) -> u32 {
    tokens.get(i).map(|t| t.span.start).unwrap_or(fallback)
}

fn alloc_frame(
    arena: &mut Vec<FrameRec>,
    kind: &'static str,
    body_start: u32,
    directive_span: Span,
) -> usize {
    let id = arena.len();
    arena.push(FrameRec {
        kind,
        directive_span,
        body_span: Span {
            start: body_start,
            end: body_start,
        },
        variable: None,
        value: None,
    });
    id
}

fn push_if_frame(
    state: &mut ExpandState<'_, '_>,
    stack: &mut Vec<IfFrame>,
    tokens: &[Token],
    next_i: usize,
    dir_span: Span,
    kind: &'static str,
    active: bool,
) {
    let body_start = next_body_start(tokens, next_i, dir_span.end);
    let frame_id = alloc_frame(state.arena, kind, body_start, dir_span);
    stack.push(IfFrame {
        active,
        taken: active,
        frame_id,
        body_start,
        branch_start: dir_span.end,
    });
    state.origin_stack.push(frame_id);
}

fn backpatch(arena: &mut [FrameRec], frame_id: usize, body_start: u32, body_end: u32) {
    if let Some(rec) = arena.get_mut(frame_id) {
        rec.body_span = Span {
            start: body_start,
            end: body_end.max(body_start),
        };
    }
}

fn tokens_body_span(body: &[Token]) -> Span {
    let mut first = None;
    let mut last_end = 0u32;
    for t in body {
        if t.kind == TokenKind::Eof {
            continue;
        }
        if first.is_none() {
            first = Some(t.span.start);
        }
        last_end = t.span.end;
    }
    match first {
        Some(start) => Span {
            start,
            end: last_end,
        },
        None => Span::default(),
    }
}

fn unroll_for(
    state: &mut ExpandState<'_, '_>,
    for_tok: &Token,
    body: &[Token],
    for_range: &[Token],
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
) -> bool {
    let Some((vars, collection, condition)) = parse_for(for_tok.text(state.src)) else {
        push_i211(
            state,
            for_tok.span,
            i211_expression_message(for_tok.text(state.src)),
        );
        return false;
    };
    let values = match eval_macro_expr(&collection, state.defines, 0) {
        Ok(values) => values,
        Err(error) => {
            note_eval_failure(state, for_tok.span, error, &collection);
            return false;
        }
    };
    let Some(values) = values.loop_values() else {
        if oversized_collection(&values) {
            push_i211(state, for_tok.span, i211_limit_message("range size"));
        } else {
            push_i211(state, for_tok.span, i211_expression_message(&collection));
        }
        return false;
    };
    let mut planned = Vec::new();
    for value in values {
        let members = match (&vars[..], &value) {
            ([_], _) => vec![value.clone()],
            (_, MacroVal::Tuple(items)) if items.len() == vars.len() => items.clone(),
            _ => {
                push_i211(state, for_tok.span, i211_expression_message(&collection));
                return false;
            }
        };
        let mut bindings = state.defines.clone();
        for (name, member) in vars.iter().zip(&members) {
            bindings.insert(name.clone(), member.clone());
        }
        if let Some(condition) = &condition {
            match eval_macro_expr(condition, &bindings, 0) {
                Ok(result) => match result.condition() {
                    Some(true) => {}
                    Some(false) => continue,
                    None => {
                        push_i211(state, for_tok.span, i211_expression_message(condition));
                        return false;
                    }
                },
                Err(error) => {
                    note_eval_failure(state, for_tok.span, error, condition);
                    return false;
                }
            }
        }
        planned.push((value, members));
    }
    let endfor_span = for_range.last().map(|token| token.span);
    let body_text = match endfor_span {
        Some(closer) => {
            let start = directive_line_extent(state.src, for_tok.span).end;
            let end = directive_line_extent(state.src, closer).start;
            Span {
                start,
                end: end.max(start),
            }
        }
        None => tokens_body_span(body),
    };
    record_source_directive(state, for_tok.span, true);
    let body_span = tokens_body_span(body);
    // A collection or `when` filter that yields no iteration leaves the written
    // body inactive; mark it discarded like an untaken `@#if` branch.
    if planned.is_empty() && body_span.end > body_span.start {
        push_discarded(state, body_span.start, body_span.end);
    }
    for (value, members) in planned {
        for (name, member) in vars.iter().zip(members) {
            state.defines.insert(name.clone(), member);
        }
        let frame_id = state.arena.len();
        state.arena.push(FrameRec {
            kind: "for",
            directive_span: for_tok.span,
            body_span,
            variable: Some(if vars.len() == 1 {
                vars[0].clone()
            } else {
                format!("({})", vars.join(","))
            }),
            value: Some(value.display()),
        });
        state.origin_stack.push(frame_id);
        if let Some(recorder) = state.source.as_deref_mut() {
            recorder.begin_loop_body(body_text.start);
        }
        let (expanded, expanded_traces) = expand_seq(state, body);
        if let Some(recorder) = state.source.as_deref_mut() {
            recorder.finish_loop_body(body_text.end, true);
        }
        for (tok, trace) in expanded.into_iter().zip(expanded_traces) {
            if tok.kind != TokenKind::Eof {
                if let Some(prev) = out.last() {
                    if let Some(merged) = merge_adjacent(state.src, prev, &tok) {
                        *out.last_mut().expect("token just read") = merged;
                        continue;
                    }
                }
                out.push(tok);
                traces.push(trace);
            }
        }
        state.origin_stack.pop();
    }
    if let Some(closer) = endfor_span {
        record_source_directive(state, closer, false);
    }
    // Dynare defines each index in the shared environment and leaves its
    // final value (including body/nested-loop redefinitions) after the loop.
    // An empty collection never binds the index.
    true
}

fn take_for_body<'a>(src: &str, tokens: &'a [Token], for_idx: usize) -> (&'a [Token], usize) {
    let mut depth = 1usize;
    let mut j = for_idx + 1;
    while j < tokens.len() {
        let t = &tokens[j];
        if t.kind == TokenKind::Eof {
            return (&tokens[for_idx + 1..j], j);
        }
        if t.kind == TokenKind::MacroDir {
            match dir_kind(src, t) {
                Dir::For => depth += 1,
                Dir::Endfor => {
                    depth -= 1;
                    if depth == 0 {
                        return (&tokens[for_idx + 1..j], j + 1);
                    }
                }
                _ => {}
            }
        }
        j += 1;
    }
    (&tokens[for_idx + 1..], tokens.len())
}

fn subst_interp(
    src: &str,
    tok: &Token,
    defines: &HashMap<String, MacroVal>,
) -> Result<(String, Vec<Token>), MacroEvalError> {
    let text = tok.text(src);
    let inner = text
        .strip_prefix("@{")
        .and_then(|s| s.strip_suffix('}'))
        .ok_or(MacroEvalError::Unsupported)?
        .trim();
    let val = eval_macro_expr(inner, defines, 0)?;
    let repl = val.display();
    if repl.is_empty() {
        return Err(MacroEvalError::Unsupported);
    }
    // A macro value is substituted as text before the .mod lexer reads it.
    // Retokenize that text, but do not execute newly generated macro syntax.
    // This lexer drops trivia and unknown characters, so only whitespace may
    // lie between its tokens; comments or skipped characters need a wider
    // surrounding-source lexer pass and remain explicitly incomplete.
    let generated = crate::lexer::tokenize(&repl);
    let pieces: Vec<_> = generated
        .iter()
        .filter(|piece| piece.kind != TokenKind::Eof)
        .collect();
    if pieces.is_empty() {
        return Err(MacroEvalError::Unsupported);
    }
    let mut cursor = 0usize;
    for piece in &pieces {
        let start = piece.span.start as usize;
        let end = piece.span.end as usize;
        if !repl[cursor..start].chars().all(char::is_whitespace)
            || matches!(piece.kind, TokenKind::MacroDir | TokenKind::MacroInterp)
        {
            return Err(MacroEvalError::Unsupported);
        }
        cursor = end;
    }
    if !repl[cursor..].chars().all(char::is_whitespace) {
        return Err(MacroEvalError::Unsupported);
    }
    let leading_space = pieces[0].span.start > 0;
    let trailing_space = pieces
        .last()
        .is_some_and(|piece| (piece.span.end as usize) < repl.len());
    let last = pieces.len() - 1;
    let tokens = pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            let mut span = tok.span;
            if index == 0 && leading_space {
                span.start += 1;
            }
            if index == last && trailing_space {
                span.end -= 1;
            }
            if span.start >= span.end {
                return Err(MacroEvalError::Unsupported);
            }
            let mut replacement = Token::with_lexeme(piece.kind, span, piece.text(&repl));
            replacement.expanded_adjacent_next = pieces
                .get(index + 1)
                .map(|next| piece.span.end == next.span.start);
            Ok(replacement)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((repl, tokens))
}

/// Substitute inside a quoted .mod value without changing its string boundary.
/// Dynare expands before string lexing. A replacement that closes the quote or
/// adds a line needs a surrounding-source lexer pass and stays incomplete here.
struct QuotedExpansion {
    token: Token,
    substitutions: Vec<(Span, String)>,
}

fn subst_quoted(
    src: &str,
    tok: &Token,
    defines: &HashMap<String, MacroVal>,
) -> Result<QuotedExpansion, (Span, MacroEvalError)> {
    let text = tok.text(src);
    let delimiter = text
        .chars()
        .next()
        .ok_or((tok.span, MacroEvalError::Unsupported))?;
    if text.len() < 2 || !text.ends_with(delimiter) {
        return Err((tok.span, MacroEvalError::Unsupported));
    }
    let mut output = String::new();
    let mut substitutions = Vec::new();
    let mut cursor = 0;
    let mut unsafe_span = None;
    while let Some(relative) = text[cursor..].find("@{") {
        let start = cursor + relative;
        let body = start + 2;
        let mut quoted = false;
        let end = text[body..]
            .char_indices()
            .find_map(|(offset, character)| {
                if character == '"' {
                    quoted = !quoted;
                }
                (character == '}' && !quoted).then_some(body + offset)
            })
            .ok_or((tok.span, MacroEvalError::Unsupported))?;
        let span = Span::new(
            tok.span.start as usize + start,
            tok.span.start as usize + end + 1,
        );
        output.push_str(&text[cursor..start]);
        match eval_macro_expr(&text[body..end], defines, 0) {
            Ok(value) => {
                let replacement = value.display();
                if replacement.contains([delimiter, '\r', '\n']) {
                    unsafe_span.get_or_insert(span);
                }
                output.push_str(&replacement);
                substitutions.push((span, replacement));
            }
            Err(MacroEvalError::Unsupported) => {
                unsafe_span.get_or_insert(span);
            }
            Err(error) => return Err((span, error)),
        }
        cursor = end + 1;
    }
    if let Some(span) = unsafe_span {
        return Err((span, MacroEvalError::Unsupported));
    }
    output.push_str(&text[cursor..]);
    Ok(QuotedExpansion {
        token: Token::with_lexeme(TokenKind::String, tok.span, output),
        substitutions,
    })
}

fn dir_kind(src: &str, tok: &Token) -> Dir {
    match directive_name(tok.text(src)).to_ascii_lowercase().as_str() {
        "define" => Dir::Define,
        "ifdef" => Dir::Ifdef,
        "ifndef" => Dir::Ifndef,
        "if" => Dir::If,
        "elseif" => Dir::Elseif,
        "else" => Dir::Else,
        "endif" => Dir::Endif,
        "for" => Dir::For,
        "endfor" => Dir::Endfor,
        _ => Dir::Unknown,
    }
}

fn name_is_defined(state: &ExpandState<'_, '_>, tok: &Token, kw: &str) -> bool {
    dir_arg_ident(tok.text(state.src), kw).is_some_and(|name| state.defines.contains_key(&name))
}

fn open_next_branch(
    state: &mut ExpandState<'_, '_>,
    frame: &mut IfFrame,
    tokens: &[Token],
    next_i: usize,
    boundary: Span,
    kind: &'static str,
    active: bool,
) {
    if !frame.active {
        push_discarded(state, frame.branch_start, boundary.start);
    }
    backpatch(
        state.arena,
        frame.frame_id,
        frame.body_start,
        boundary.start,
    );
    frame.active = active;
    let body_start = next_body_start(tokens, next_i, boundary.end);
    let frame_id = alloc_frame(state.arena, kind, body_start, boundary);
    frame.frame_id = frame_id;
    frame.body_start = body_start;
    frame.branch_start = boundary.end;
    if let Some(last) = state.origin_stack.last_mut() {
        *last = frame_id;
    } else {
        state.origin_stack.push(frame_id);
    }
}

fn directive_name(text: &str) -> &str {
    let Some(rest) = text.trim_start().strip_prefix("@#") else {
        return "";
    };
    let rest = rest.trim_start();
    match ident_len(rest) {
        Some(n) => &rest[..n],
        None => "",
    }
}

fn eval_condition(state: &mut ExpandState<'_, '_>, tok: &Token, kw: &str) -> Option<bool> {
    let arg = strip_kw(tok.text(state.src), kw)?;
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    match eval_macro_expr(arg, state.defines, 0) {
        Ok(value) => match value.condition() {
            Some(condition) => Some(condition),
            None => {
                push_type_error(
                    state,
                    tok.span,
                    "E283",
                    "The condition must evaluate to a boolean or a double".to_string(),
                );
                None
            }
        },
        Err(error) => {
            note_eval_failure(state, tok.span, error, arg);
            None
        }
    }
}

fn eval_includepath(state: &mut ExpandState<'_, '_>, tok: &Token) -> Option<String> {
    let argument = strip_kw(tok.text(state.src), "includepath")?;
    let argument = strip_line_comment(argument).trim();
    match eval_macro_expr(argument, state.defines, 0) {
        Ok(MacroVal::Text(path)) => Some(path),
        Ok(_) => {
            push_type_error(
                state,
                tok.span,
                "E305",
                "File name does not evaluate to a string".to_string(),
            );
            None
        }
        Err(error) => {
            if state.include_seen
                && matches!(
                    error,
                    MacroEvalError::UnknownVariable(_) | MacroEvalError::UnknownFunction(_)
                )
            {
                state.incomplete.get_or_insert(tok.span);
            } else {
                note_eval_failure(state, tok.span, error, argument);
            }
            None
        }
    }
}

fn take_if_end(src: &str, tokens: &[Token], start: usize) -> usize {
    let mut depth = 1usize;
    for (index, token) in tokens.iter().enumerate().skip(start + 1) {
        if token.kind != TokenKind::MacroDir {
            continue;
        }
        match dir_kind(src, token) {
            Dir::If | Dir::Ifdef | Dir::Ifndef => depth += 1,
            Dir::Endif => {
                depth -= 1;
                if depth == 0 {
                    return index + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

fn retain_raw_macro(
    state: &mut ExpandState<'_, '_>,
    tokens: &[Token],
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
) {
    // Condition/definition failures already recorded their reasons. Keep the
    // incomplete flag when a raw block is retained without a new named reason.
    if let Some(first) = tokens.first() {
        state.incomplete.get_or_insert(first.span);
    }
    for token in tokens {
        emit(state, out, traces, token.clone());
    }
}

fn define_rhs_expression(text: &str) -> &str {
    let Some(rest) = strip_kw(text, "define") else {
        return text.trim();
    };
    let rest = rest.trim_start();
    let Some(n) = ident_len(rest) else {
        return rest.trim();
    };
    let rest = rest[n..].trim_start();
    if let Some(after_open) = rest.strip_prefix('(') {
        if let Some(close) = after_open.find(')') {
            let after = after_open[close + 1..].trim_start();
            return after.strip_prefix('=').map(str::trim).unwrap_or(after);
        }
    }
    rest.strip_prefix('=').map(str::trim).unwrap_or(rest.trim())
}

fn interp_expression(src: &str, span: Span) -> String {
    let text = &src[span.start as usize..span.end as usize];
    text.strip_prefix("@{")
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(text)
        .trim()
        .to_string()
}

fn parse_define_eval(
    text: &str,
    defines: &HashMap<String, MacroVal>,
) -> Result<Option<(String, MacroVal)>, MacroEvalError> {
    let Some(rest) = strip_kw(text, "define") else {
        return Ok(None);
    };
    let rest = rest.trim_start();
    let Some(n) = ident_len(rest) else {
        return Ok(None);
    };
    let name = rest[..n].to_string();
    let rest = rest[n..].trim_start();
    if let Some(after_open) = rest.strip_prefix('(') {
        let Some(close) = after_open.find(')') else {
            return Err(MacroEvalError::Unsupported);
        };
        let params: Vec<_> = after_open[..close]
            .split(',')
            .map(str::trim)
            .map(str::to_owned)
            .collect();
        if params.is_empty() || params.iter().any(|param| !is_simple_ident(param)) {
            return Err(MacroEvalError::Unsupported);
        }
        let Some(body) = after_open[close + 1..].trim_start().strip_prefix('=') else {
            return Err(MacroEvalError::Unsupported);
        };
        let body = strip_line_comment(body).trim();
        validate_function_body(body)?;
        return Ok(Some((
            name,
            MacroVal::Function {
                params,
                body: body.to_string(),
            },
        )));
    }
    let Some(body) = rest.strip_prefix('=') else {
        return Ok(Some((name, MacroVal::Bool(true))));
    };
    let body = strip_line_comment(body).trim();
    Ok(Some((name, eval_macro_expr(body, defines, 0)?)))
}

/// Check the function body's expression shape at definition time without
/// looking up its free names. Dynare parses a function body immediately but
/// evaluates names when the function is called. Only an unmistakable missing
/// final operand gets the pinned `unexpected EOL` sentence; uncertain syntax
/// stays incomplete instead of being mislabeled as an official refusal.
fn validate_function_body(body: &str) -> Result<(), MacroEvalError> {
    if body.is_empty() {
        return Err(MacroEvalError::SyntaxEol);
    }
    let chars: Vec<char> = body.chars().collect();
    let mut stack = Vec::new();
    let mut need_operand = true;
    let mut last_open = false;
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            if i == chars.len() {
                return Err(MacroEvalError::SyntaxEol);
            }
            need_operand = false;
            last_open = false;
            i += 1;
            continue;
        }
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.'))
            {
                i += 1;
            }
            need_operand = false;
            last_open = false;
            continue;
        }
        match ch {
            '(' | '[' => {
                stack.push(ch);
                need_operand = true;
                last_open = true;
            }
            ')' | ']' => {
                let expected = if ch == ')' { '(' } else { '[' };
                if stack.pop() != Some(expected) || (need_operand && !last_open) {
                    return Err(MacroEvalError::Unsupported);
                }
                need_operand = false;
                last_open = false;
            }
            ',' => {
                if need_operand {
                    return Err(MacroEvalError::Unsupported);
                }
                need_operand = true;
                last_open = false;
            }
            '+' | '-' | '!' if need_operand => {
                // Macro unary operators are legal at an operand position.
                last_open = false;
            }
            '+' | '-' | '*' | '/' | '^' | ':' | '<' | '>' | '!' | '&' | '|' | '=' => {
                if need_operand {
                    return Err(MacroEvalError::Unsupported);
                }
                if ch == '=' && chars.get(i + 1) != Some(&'=') {
                    return Err(MacroEvalError::Unsupported);
                }
                if (matches!(ch, '=' | '!' | '<' | '>' | '&' | '|')
                    && chars.get(i + 1) == Some(&'='))
                    || (matches!(ch, '&' | '|') && chars.get(i + 1) == Some(&ch))
                {
                    i += 1;
                }
                need_operand = true;
                last_open = false;
            }
            _ => return Err(MacroEvalError::Unsupported),
        }
        i += 1;
    }
    if need_operand || !stack.is_empty() {
        Err(MacroEvalError::SyntaxEol)
    } else {
        Ok(())
    }
}

fn eval_macro_expr(
    source: &str,
    defines: &HashMap<String, MacroVal>,
    depth: usize,
) -> Result<MacroVal, MacroEvalError> {
    if depth >= MACRO_DEPTH_CAP {
        return Err(MacroEvalError::Limit("expression depth"));
    }
    let source = source.trim();
    if source.is_empty() {
        return Err(MacroEvalError::Unsupported);
    }
    // Pinned macro grammar: comparison binds looser than `in`; `in` binds looser
    // than `:` and arithmetic. `in` is non-associative.
    for group in [&['|'][..], &['&'][..], &['=', '!', '<', '>'][..]] {
        if let Some((left, op, right)) = split_macro_binary(source, group) {
            let token = match op {
                "||" => "OR",
                "&&" => "AND",
                "|" => "UNION",
                "&" => "INTERSECTION",
                "==" => "EQUAL_EQUAL",
                "!=" => "NOT_EQUAL",
                "<=" => "LESS_EQUAL",
                ">=" => "GREATER_EQUAL",
                "<" => "LESS",
                ">" => "GREATER",
                _ => unreachable!(),
            };
            let left =
                eval_macro_expr(left, defines, depth + 1).map_err(|error| error.at_end(token))?;
            let right = eval_macro_expr(right, defines, depth + 1)?;
            return eval_binary(left, op, right);
        }
    }
    if let Some((left, right)) = split_top_level_keyword_first(source, "in") {
        let left = left.trim();
        let right = right.trim();
        if left.is_empty() {
            return Err(MacroEvalError::SyntaxUnexpected("IN"));
        }
        if right.is_empty() {
            return Err(MacroEvalError::MissingInOperand);
        }
        if split_top_level_keyword_first(right, "in").is_some() {
            return Err(MacroEvalError::SyntaxUnexpected("IN"));
        }
        let left = eval_macro_expr(left, defines, depth + 1)?;
        let right = eval_macro_expr(right, defines, depth + 1)?;
        return eval_membership(left, right);
    }
    if let Some((left, right)) = split_top_level_char(source, ':') {
        let start = eval_macro_expr(left, defines, depth + 1)?;
        let end = eval_macro_expr(right, defines, depth + 1)?;
        if let (MacroVal::Int(start), MacroVal::Int(end)) = (start, end) {
            return Ok(MacroVal::Range { start, end });
        }
        return Err(MacroEvalError::Unsupported);
    }
    for group in [&['+', '-'][..], &['*', '/'][..]] {
        if let Some((left, op, right)) = split_macro_binary(source, group) {
            let left = eval_macro_expr(left, defines, depth + 1)?;
            let right = eval_macro_expr(right, defines, depth + 1)?;
            return eval_binary(left, op, right);
        }
    }
    if let Some(body) = source.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let parts = split_top_level_list(body);
        let count = parts.len();
        return Ok(MacroVal::Array(
            parts
                .into_iter()
                .enumerate()
                .map(|(index, item)| {
                    eval_macro_expr(item, defines, depth + 1).map_err(|error| {
                        error.at_end(if index + 1 == count {
                            "RBRACKET"
                        } else {
                            "COMMA"
                        })
                    })
                })
                .collect::<Result<_, _>>()?,
        ));
    }
    if let Some(body) = source.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        let parts = split_top_level_list(body);
        let count = parts.len();
        return if parts.len() > 1 {
            Ok(MacroVal::Tuple(
                parts
                    .into_iter()
                    .enumerate()
                    .map(|(index, item)| {
                        eval_macro_expr(item, defines, depth + 1).map_err(|error| {
                            error.at_end(if index + 1 == count {
                                "RPAREN"
                            } else {
                                "COMMA"
                            })
                        })
                    })
                    .collect::<Result<_, _>>()?,
            ))
        } else {
            eval_macro_expr(body, defines, depth + 1).map_err(|error| error.at_end("RPAREN"))
        };
    }
    if let Some(value) = strip_quotes(source) {
        return Ok(MacroVal::Text(value.to_string()));
    }
    if source.eq_ignore_ascii_case("true") || source.eq_ignore_ascii_case("false") {
        return Ok(MacroVal::Bool(source.eq_ignore_ascii_case("true")));
    }
    if let Ok(value) = source.parse::<i64>() {
        return Ok(MacroVal::Int(value));
    }
    if let Ok(value) = source.parse::<f64>() {
        return Ok(MacroVal::Real(value));
    }
    if let Some(rest) = source.strip_prefix('-') {
        return match eval_macro_expr(rest, defines, depth + 1)? {
            MacroVal::Int(value) => value
                .checked_neg()
                .map(MacroVal::Int)
                .ok_or(MacroEvalError::Unsupported),
            MacroVal::Real(value) => Ok(MacroVal::Real(-value)),
            _ => Err(MacroEvalError::TypeMismatch("-")),
        };
    }
    if let Some(rest) = source.strip_prefix('!') {
        return eval_macro_expr(rest, defines, depth + 1)?
            .condition()
            .map(|value| MacroVal::Bool(!value))
            .ok_or(MacroEvalError::TypeMismatch("!"));
    }
    let name_len = ident_len(source).ok_or(MacroEvalError::Unsupported)?;
    let name = &source[..name_len];
    let rest = source[name_len..].trim_start();
    if let Some(args) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if name == "defined" {
            let arg = args.trim();
            if !is_simple_ident(arg) {
                return Err(MacroEvalError::Unsupported);
            }
            return match defines.get(arg) {
                Some(MacroVal::Unresolved) => Err(MacroEvalError::PriorFailure),
                value => Ok(MacroVal::Bool(value.is_some())),
            };
        }
        let Some(MacroVal::Function { params, body }) = defines.get(name) else {
            return Err(if is_pinned_macro_builtin(name) {
                MacroEvalError::Unsupported
            } else {
                MacroEvalError::UnknownFunction(name.to_string())
            });
        };
        let arguments = split_top_level_list(args);
        if arguments.len() != params.len() {
            return Err(MacroEvalError::Unsupported);
        }
        let mut local = defines.clone();
        for (index, (param, arg)) in params.iter().zip(arguments).enumerate() {
            let value = eval_macro_expr(arg, defines, depth + 1).map_err(|error| {
                error.at_end(if index + 1 == params.len() {
                    "RPAREN"
                } else {
                    "COMMA"
                })
            })?;
            local.insert(param.clone(), value);
        }
        return eval_macro_expr(body, &local, depth + 1);
    }
    if let Some(index) = rest.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        let value = defines
            .get(name)
            .ok_or_else(|| MacroEvalError::UnknownVariable(name.to_string()))?;
        let MacroVal::Int(index) =
            eval_macro_expr(index, defines, depth + 1).map_err(|error| error.at_end("RBRACKET"))?
        else {
            return Err(MacroEvalError::Unsupported);
        };
        return match value {
            MacroVal::Array(items) => items
                .get(index.saturating_sub(1) as usize)
                .cloned()
                .ok_or(MacroEvalError::Unsupported),
            _ => Err(MacroEvalError::Unsupported),
        };
    }
    if rest.is_empty() {
        let value = defines
            .get(name)
            .cloned()
            .ok_or_else(|| MacroEvalError::UnknownVariable(name.to_string()))?;
        return if matches!(value, MacroVal::Unresolved) {
            Err(MacroEvalError::PriorFailure)
        } else {
            Ok(value)
        };
    }
    Err(MacroEvalError::Unsupported)
}

fn eval_binary(left: MacroVal, op: &str, right: MacroVal) -> Result<MacroVal, MacroEvalError> {
    // The pinned macro language has array/tuple operators and string ordering
    // that this bounded evaluator does not implement. They remain incomplete,
    // never a fabricated operand-type Error.
    if matches!(
        &left,
        MacroVal::Array(_) | MacroVal::Tuple(_) | MacroVal::Function { .. } | MacroVal::Unresolved
    ) || matches!(
        &right,
        MacroVal::Array(_) | MacroVal::Tuple(_) | MacroVal::Function { .. } | MacroVal::Unresolved
    ) || (matches!((&left, &right), (MacroVal::Text(_), MacroVal::Text(_)))
        && !matches!(op, "+" | "==" | "!="))
    {
        return Err(MacroEvalError::Unsupported);
    }
    if let (Some(a), Some(b)) = (numeric_value(&left), numeric_value(&right)) {
        if let (MacroVal::Int(a), MacroVal::Int(b)) = (&left, &right) {
            match op {
                "+" => {
                    return a
                        .checked_add(*b)
                        .map(MacroVal::Int)
                        .ok_or(MacroEvalError::Unsupported)
                }
                "-" => {
                    return a
                        .checked_sub(*b)
                        .map(MacroVal::Int)
                        .ok_or(MacroEvalError::Unsupported)
                }
                "*" => {
                    return a
                        .checked_mul(*b)
                        .map(MacroVal::Int)
                        .ok_or(MacroEvalError::Unsupported)
                }
                _ => {}
            }
        }
        let result = match op {
            "+" => MacroVal::Real(a + b),
            "-" => MacroVal::Real(a - b),
            "*" => MacroVal::Real(a * b),
            "/" if b != 0.0 => MacroVal::Real(a / b),
            "==" => MacroVal::Bool(a == b),
            "!=" => MacroVal::Bool(a != b),
            "<" => MacroVal::Bool(a < b),
            "<=" => MacroVal::Bool(a <= b),
            ">" => MacroVal::Bool(a > b),
            ">=" => MacroVal::Bool(a >= b),
            "&&" => MacroVal::Bool(a != 0.0 && b != 0.0),
            "||" => MacroVal::Bool(a != 0.0 || b != 0.0),
            _ => return Err(MacroEvalError::Unsupported),
        };
        return match result {
            MacroVal::Real(value) if !value.is_finite() => Err(MacroEvalError::Unsupported),
            other => Ok(other),
        };
    }
    if matches!(op, "&&" | "||") {
        if let (Some(a), Some(b)) = (left.condition(), right.condition()) {
            return Ok(MacroVal::Bool(if op == "&&" { a && b } else { a || b }));
        }
    }
    match (left, op, right) {
        (MacroVal::Text(a), "+", MacroVal::Text(b)) => Ok(MacroVal::Text(a + &b)),
        (MacroVal::Bool(a), "&&", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a && b)),
        (MacroVal::Bool(a), "||", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a || b)),
        (MacroVal::Bool(a), "==", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a == b)),
        (MacroVal::Bool(a), "!=", MacroVal::Bool(b)) => Ok(MacroVal::Bool(a != b)),
        (MacroVal::Text(a), "==", MacroVal::Text(b)) => Ok(MacroVal::Bool(a == b)),
        (MacroVal::Text(a), "!=", MacroVal::Text(b)) => Ok(MacroVal::Bool(a != b)),
        (_, op, _) => Err(MacroEvalError::TypeMismatch(match op {
            "+" => "+",
            "-" => "-",
            "*" => "*",
            "/" => "/",
            "&&" => "&&",
            "||" => "||",
            _ => "comparison",
        })),
    }
}

fn numeric_value(value: &MacroVal) -> Option<f64> {
    match value {
        MacroVal::Int(value) => Some(*value as f64),
        MacroVal::Real(value) => Some(*value),
        _ => None,
    }
}

fn eval_membership(left: MacroVal, right: MacroVal) -> Result<MacroVal, MacroEvalError> {
    // Pin: `in` calls `contains` on the evaluated right operand. Only Array and
    // Tuple implement `contains`. Range::eval materializes to an Array before
    // storage; this evaluator does not materialize, so a still-Range right
    // operand stays Unsupported (incomplete), not a Boolean and not E285.
    let items = match right {
        MacroVal::Array(values) if values.len() <= RANGE_CAP => values,
        MacroVal::Tuple(values) if values.len() <= RANGE_CAP => values,
        MacroVal::Range { .. } => return Err(MacroEvalError::Unsupported),
        MacroVal::Array(_) | MacroVal::Tuple(_) => {
            return Err(MacroEvalError::Limit("range size"));
        }
        MacroVal::Unresolved => return Err(MacroEvalError::PriorFailure),
        MacroVal::Function { .. } => {
            return Err(MacroEvalError::Unsupported);
        }
        _ => return Err(MacroEvalError::InOperandType),
    };
    Ok(MacroVal::Bool(
        items.iter().any(|item| macro_values_equal(&left, item)),
    ))
}

fn macro_values_equal(left: &MacroVal, right: &MacroVal) -> bool {
    match (left, right) {
        (MacroVal::Bool(a), MacroVal::Bool(b)) => a == b,
        (MacroVal::Text(a), MacroVal::Text(b)) => a == b,
        (MacroVal::Int(a), MacroVal::Int(b)) => a == b,
        (MacroVal::Real(a), MacroVal::Real(b)) => a == b,
        (MacroVal::Int(a), MacroVal::Real(b)) => *a as f64 == *b,
        (MacroVal::Real(a), MacroVal::Int(b)) => *a == *b as f64,
        // Endpoint equality for two stored ranges; pin Array::is_equal does not
        // treat a range as equal to the array of its elements.
        (MacroVal::Range { start: s1, end: e1 }, MacroVal::Range { start: s2, end: e2 }) => {
            s1 == s2 && e1 == e2
        }
        (MacroVal::Array(a), MacroVal::Array(b)) | (MacroVal::Tuple(a), MacroVal::Tuple(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(x, y)| macro_values_equal(x, y))
        }
        _ => false,
    }
}

fn split_macro_binary<'a>(source: &'a str, group: &[char]) -> Option<(&'a str, &'a str, &'a str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        let op = [
            "||", "&&", "==", "!=", "<=", ">=", "+", "-", "*", "/", "<", ">",
        ]
        .into_iter()
        .find(|op| tail.starts_with(op) && group.contains(&op.chars().next().unwrap()));
        if let Some(op) = op {
            let left = source[..i].trim();
            let right = source[i + op.len()..].trim();
            if !left.is_empty() && !right.is_empty() && !(op == "-" && left.ends_with(':')) {
                found = Some((left, op, right));
            }
        }
    });
    found
}

fn split_top_level_char(source: &str, needle: char) -> Option<(&str, &str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        if tail.starts_with(needle) {
            found = Some((&source[..i], &source[i + needle.len_utf8()..]));
        }
    });
    found
}

fn split_top_level_keyword<'a>(source: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        if is_top_level_keyword_at(source, i, tail, word) {
            found = Some((&source[..i], &source[i + word.len()..]));
        }
    });
    found
}

/// Leftmost identifier-bounded keyword. Membership uses this so a second
/// top-level `in` on the right can refuse instead of nesting.
fn split_top_level_keyword_first<'a>(source: &'a str, word: &str) -> Option<(&'a str, &'a str)> {
    let mut found = None;
    walk_top_level(source, |i, tail| {
        if found.is_none() && is_top_level_keyword_at(source, i, tail, word) {
            found = Some((&source[..i], &source[i + word.len()..]));
        }
    });
    found
}

fn is_top_level_keyword_at(source: &str, i: usize, tail: &str, word: &str) -> bool {
    if !tail.starts_with(word) {
        return false;
    }
    let identifier_char = |ch: char| ch.is_ascii_alphanumeric() || ch == '_';
    // The pin lexes a number followed immediately by `in` as two tokens.
    // A preceding identifier absorbs it (for example `begin` or `x1in`).
    let prefix = &source[..i];
    let numeric = prefix
        .rsplit(|ch: char| !identifier_char(ch) && ch != '.')
        .next()
        .unwrap_or("");
    let after_number = numeric.starts_with(|ch: char| ch.is_ascii_digit() || ch == '.')
        && numeric.replace(['d', 'D'], "e").parse::<f64>().is_ok();
    (!prefix.chars().last().is_some_and(identifier_char) || after_number)
        && tail[word.len()..]
            .chars()
            .next()
            .is_none_or(|ch| !identifier_char(ch))
}

fn split_top_level_list(source: &str) -> Vec<&str> {
    if source.trim().is_empty() {
        return Vec::new();
    }
    let mut starts = vec![0];
    walk_top_level(source, |i, tail| {
        if tail.starts_with(',') {
            starts.push(i + 1);
        }
    });
    let mut parts = Vec::new();
    for pair in starts.windows(2) {
        parts.push(source[pair[0]..pair[1] - 1].trim());
    }
    parts.push(source[*starts.last().unwrap()..].trim());
    parts
}

fn walk_top_level(source: &str, mut visit: impl FnMut(usize, &str)) {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (i, ch) in source.char_indices() {
        if let Some(delim) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delim {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => quote = Some(ch),
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => visit(i, &source[i..]),
            _ => {}
        }
    }
}

fn strip_line_comment(source: &str) -> &str {
    let mut end = source.len();
    walk_top_level(source, |i, tail| {
        if tail.starts_with("//") && end == source.len() {
            end = i;
        }
    });
    &source[..end]
}

fn strip_quotes(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && *bytes.last()? == b'"' {
        Some(&s[1..s.len() - 1])
    } else {
        None
    }
}

fn check_for_tuple(state: &mut ExpandState<'_, '_>, tok: &Token) {
    let Some(rest) = strip_kw(tok.text(state.src), "for") else {
        return;
    };
    let rest = rest.trim_start();
    if !rest.starts_with('(') {
        return;
    }
    let Some(close) = rest.find(')') else {
        return;
    };
    let names = rest[1..close]
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .count();
    if names == 1 {
        // One index receives the whole tuple; only multiple indices unpack it.
        return;
    }
    let after = rest[close + 1..].trim_start();
    let Some(after) = strip_word(after, "in") else {
        return;
    };
    let after = after.trim_start();
    if !after.starts_with('[') {
        return;
    };
    if let Some(n) = first_tuple_size(after) {
        if n != names {
            push_type_error(
                state,
                tok.span,
                "E284",
                format!("Encountered tuple of size {n} but only have {names} index variables"),
            );
        }
    }
}

fn first_tuple_size(s: &str) -> Option<usize> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?;
    let inner = inner.trim();
    let start = inner.find('(')?;
    let end = inner[start..].find(')')?;
    let tuple = &inner[start + 1..start + end];
    Some(
        tuple
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .count(),
    )
}

fn parse_for(text: &str) -> Option<(Vec<String>, String, Option<String>)> {
    let rest = strip_kw(text, "for")?;
    let rest = rest.trim_start();
    let (vars, rest) = if let Some(after_open) = rest.strip_prefix('(') {
        let close = after_open.find(')')?;
        let names: Vec<_> = after_open[..close]
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        if names.is_empty() || names.iter().any(|name| !is_simple_ident(name)) {
            return None;
        }
        (names, after_open[close + 1..].trim_start())
    } else {
        let n = ident_len(rest)?;
        (vec![rest[..n].to_string()], rest[n..].trim_start())
    };
    let rest = strip_word(rest, "in")?;
    let rest = rest.trim_start();
    let (collection, condition) = match split_top_level_keyword(rest, "when") {
        Some((collection, condition)) => (collection.trim(), Some(condition.trim().to_string())),
        None => (rest.trim(), None),
    };
    if collection.is_empty() || condition.as_ref().is_some_and(String::is_empty) {
        return None;
    }
    Some((vars, collection.to_string(), condition))
}

fn defined_name(text: &str) -> Option<String> {
    let rest = strip_kw(text, "define")?.trim_start();
    let len = ident_len(rest)?;
    Some(rest[..len].to_string())
}

fn is_pinned_macro_builtin(name: &str) -> bool {
    matches!(
        name,
        "max"
            | "min"
            | "mod"
            | "exp"
            | "log"
            | "ln"
            | "log10"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sqrt"
            | "cbrt"
            | "sign"
            | "floor"
            | "ceil"
            | "trunc"
            | "erf"
            | "erfc"
            | "gamma"
            | "lgamma"
            | "round"
            | "normpdf"
            | "normcdf"
            | "length"
            | "empty"
            | "sum"
            | "isboolean"
            | "isreal"
            | "isstring"
            | "istuple"
            | "isarray"
            | "isempty"
            | "defined"
            | "bool"
            | "real"
            | "string"
            | "tuple"
            | "array"
    )
}

fn dir_arg_ident(text: &str, kw: &str) -> Option<String> {
    let rest = strip_kw(text, kw)?;
    let rest = rest.trim_start();
    let n = ident_len(rest)?;
    Some(rest[..n].to_string())
}

fn inclusive_range(start: i64, end: i64) -> Vec<i64> {
    if start > end {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut n = start;
    while n <= end && out.len() < RANGE_CAP {
        out.push(n);
        if n == end {
            break;
        }
        n += 1;
    }
    out
}

fn strip_kw<'a>(text: &'a str, kw: &str) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix("@#")?;
    strip_word(rest.trim_start(), kw)
}

fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    if text.len() < word.len() || !text[..word.len()].eq_ignore_ascii_case(word) {
        return None;
    }
    let after = &text[word.len()..];
    if after
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some(after)
}

fn ident_len(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() && first != '_' {
        return None;
    }
    let mut end = first.len_utf8();
    for (i, c) in chars {
        if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    Some(end)
}

fn is_simple_ident(s: &str) -> bool {
    ident_len(s).is_some_and(|n| n == s.len())
}
