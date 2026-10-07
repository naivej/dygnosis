//! Native `@#define` / `@#if` / `@#for` / `@{NAME}` expansion over the token stream.

use std::collections::{HashMap, HashSet};

use crate::lexer::{Token, TokenKind};
use crate::macro_expr::MacroBudget;
use crate::span::Span;

const RANGE_CAP: usize = 10_000;

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
    Vec<MacroMessage>,
);

/// One `@#echo` or `@#echomacrovars` result, in execution order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacroMessage {
    pub kind: &'static str,
    pub message: String,
    pub span: Span,
    /// Written file that owns `span` after include splicing. `None` is the
    /// buffer that was expanded.
    pub file: Option<String>,
}

/// One verified incomplete-expansion failure for diagnostics and status hover.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncompleteReason {
    pub span: Span,
    pub code: &'static str,
    pub message: String,
}

#[derive(Clone, Debug)]
pub(crate) enum MacroVal {
    /// Stored numbers are [`MacroVal::Real`]. This variant keeps older match arms total.
    #[allow(dead_code)]
    Int(i64),
    Real(f64),
    Bool(bool),
    Text(String),
    /// A string slice that is not valid UTF-8. Indexing and `length` use bytes.
    /// Rendering it into model text is the `non-UTF-8 byte slice` limit.
    Bytes(Vec<u8>),
    Tuple(Vec<MacroVal>),
    Array(Vec<MacroVal>),
    Function {
        params: Vec<String>,
        body: String,
    },
    Unresolved,
}

impl MacroVal {
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
pub(crate) enum MacroEvalError {
    UnknownVariable(String),
    UnknownFunction(String),
    /// Pinned refusal for a proven non-array/tuple right operand of `in`.
    InOperandType,
    SyntaxEol,
    SyntaxUnexpected(&'static str),
    /// Pinned macro-processor sentence with its diagnostic family.
    Official {
        code: &'static str,
        message: String,
    },
    Unsupported,
    /// Local evaluator resource limit (depth or collection size).
    Limit(&'static str),
    /// An earlier failed definition already owns the reason; withhold quietly.
    PriorFailure,
}

impl MacroEvalError {
    fn at_end(self, token: &'static str) -> Self {
        match self {
            Self::SyntaxEol | Self::SyntaxUnexpected("EOL") => Self::SyntaxUnexpected(token),
            other => other,
        }
    }

    fn fatal(&self) -> bool {
        self.diagnostic().is_some() || matches!(self, Self::Limit(_))
    }

    fn diagnostic(&self) -> Option<(&'static str, String)> {
        match self {
            Self::UnknownVariable(name) => Some(("E063", format!("Unknown variable {name}"))),
            Self::UnknownFunction(name) => Some(("E063", format!("Unknown function {name}"))),
            Self::InOperandType => Some((
                "E285",
                "Second argument of `in` operator must be an array".to_string(),
            )),
            Self::SyntaxEol => Some(("E062", "syntax error, unexpected EOL".to_string())),
            Self::SyntaxUnexpected(token) => {
                Some(("E062", format!("syntax error, unexpected {token}")))
            }
            Self::Official { code, message } => Some((*code, message.clone())),
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
    state.stopped = true;
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
    state.stopped = true;
}

fn note_eval_failure(
    state: &mut ExpandState<'_, '_>,
    span: Span,
    error: MacroEvalError,
    expression: &str,
) {
    if error.fatal() {
        state.stopped = true;
    }
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
    /// An official macro error already owns this root. Later directives do not run.
    stopped: bool,
    /// A missing include with no file visitor. Later messages are dropped.
    /// The model text still emits so a later parse can see the written model.
    quiet_after: Option<u32>,
    file_visitor: Option<&'w mut dyn MacroFileVisitor>,
    source: Option<&'w mut SourceRecorder<'src>>,
    messages: &'w mut Vec<MacroMessage>,
    /// Spliced-buffer span and the 1-based written line where that span starts.
    line_segments: &'w [(Span, u32)],
    /// Bindings of the `for` currently executing, printed as macro expressions.
    loop_values: Vec<(String, String)>,
    /// One counter for this root, shared with included files and expressions.
    budget: &'w mut MacroBudget,
}

trait MacroFileVisitor {
    fn visit(
        &mut self,
        span: Span,
        filename: &str,
        defines: &mut HashMap<String, MacroVal>,
        certain: bool,
        bindings: &[(String, String)],
        budget: &mut MacroBudget,
    ) -> bool;
    fn path(&mut self, span: Span, path: &str, certain: bool) -> bool;
}

pub(crate) enum MacroFileDirective<'a> {
    Include(&'a str),
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
    /// Loop indexes in scope, printed as macro expressions.
    pub bindings: &'a [(String, String)],
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
        filename: &str,
        defines: &mut HashMap<String, MacroVal>,
        certain: bool,
        bindings: &[(String, String)],
        budget: &mut MacroBudget,
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
            directive: MacroFileDirective::Include(filename),
            certain,
            bindings,
        })
        else {
            return false;
        };
        if self.files.contains(&target) {
            return false;
        }
        self.parents.push((file, span));
        self.files.push(target);
        // Nested sites are recorded while the child runs. A loaded file is not
        // a missing include when its own blocks or later directives fail.
        let _child_complete = macro_file_complete(&source, defines, self, budget);
        self.files.pop();
        self.parents.pop();
        true
    }

    fn path(&mut self, span: Span, path: &str, certain: bool) -> bool {
        matches!(
            (self.load)(MacroFileEvent {
                file: self.files.last().expect("macro root"),
                span,
                parents: &self.parents,
                directive: MacroFileDirective::IncludePath(path),
                certain,
                bindings: &[],
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
    let mut budget = MacroBudget::new();
    let complete = macro_file_complete(source, &mut HashMap::new(), &mut includes, &mut budget);
    MacroFileProof {
        complete,
        sites: includes.sites,
    }
}

fn macro_file_complete(
    text: &str,
    defines: &mut HashMap<String, MacroVal>,
    includes: &mut dyn MacroFileVisitor,
    budget: &mut MacroBudget,
) -> bool {
    let source = crate::parser::normalize_newlines(text);
    let tokens = scan_macro_tokens(&source);
    // An unclosed block in this file can still contain an active include whose
    // closer lives in the child. Visit those sites; the joined text is checked
    // again after the splice. Block structure still fails this file's proof.
    let blocks_ok = macro_blocks_complete(&source, &tokens);
    let mut arena = Vec::new();
    let mut errors = Vec::new();
    let mut discarded = Vec::new();
    let mut incomplete = None;
    let mut incomplete_reasons = Vec::new();
    let mut messages = Vec::new();
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
        stopped: false,
        quiet_after: None,
        file_visitor: Some(includes),
        source: None,
        messages: &mut messages,
        line_segments: &[],
        loop_values: Vec::new(),
        budget,
    };
    expand_seq(&mut state, &tokens);
    blocks_ok && incomplete.is_none() && errors.is_empty()
}

pub fn expand_macros(src: &str, tokens: Vec<Token>) -> Vec<Token> {
    expand_macros_full(src, tokens).0
}

pub fn expand_macros_full(
    src: &str,
    tokens: Vec<Token>,
) -> (Vec<Token>, Vec<(Span, &'static str, String)>) {
    let (out, _, _, errors, _, _, _, _, _) = expand_macros_traced_full(src, tokens, &[]);
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
    expand_macros_with_status_lines(src, tokens, &[])
}

pub(crate) fn expand_macros_with_status_lines(
    src: &str,
    tokens: Vec<Token>,
    line_segments: &[(Span, u32)],
) -> (
    Vec<Token>,
    Vec<MacroTypeError>,
    Option<Span>,
    Vec<IncompleteReason>,
) {
    let (out, _, _, errors, _, incomplete, reasons, _, _) =
        expand_macros_traced_full(src, tokens, line_segments);
    (out, errors, incomplete, reasons)
}

#[allow(dead_code)]
pub(crate) fn expand_macros_traced_with_status(
    src: &str,
    tokens: Vec<Token>,
) -> (
    Vec<Token>,
    Vec<TokenTrace>,
    Vec<FrameRec>,
    bool,
    bool,
    Vec<MacroMessage>,
) {
    expand_macros_traced_with_lines(src, tokens, &[])
}

pub(crate) fn expand_macros_traced_with_lines(
    src: &str,
    tokens: Vec<Token>,
    line_segments: &[(Span, u32)],
) -> (
    Vec<Token>,
    Vec<TokenTrace>,
    Vec<FrameRec>,
    bool,
    bool,
    Vec<MacroMessage>,
) {
    // Block structure follows the macro scanner, including directives that sit
    // in block comments. The .mod lexer has not decided comments yet.
    let scanned = scan_macro_tokens(src);
    let blocks_complete = macro_blocks_complete(src, &scanned);
    let (out, traces, arena, errors, _, incomplete, _, _, messages) =
        expand_macros_traced_full(src, tokens, line_segments);
    // Some existing macro checks record an error while still unrolling. Keep
    // legacy incomplete unchanged, but consume that same error proof for jumps.
    (
        out,
        traces,
        arena,
        incomplete.is_some(),
        blocks_complete && errors.is_empty(),
        messages,
    )
}

/// Expand while recording a source-layout display copy. The token stream matches
/// ordinary expansion; fragments are a side structure for the editor preview.
///
/// `gaps` marks leftover include-directive indent and line endings in an
/// already spliced `src`. Pass an empty slice when there are no active includes.
pub(crate) fn expand_macros_with_source_layout(
    src: &str,
    _tokens: Vec<Token>,
    gaps: &[SourceLayoutGap],
    line_segments: &[(Span, u32)],
) -> (
    Vec<Token>,
    Vec<MacroTypeError>,
    Option<Span>,
    SourceLayoutBuild,
) {
    let mut recorder = SourceRecorder::new(src, gaps);
    let mut budget = MacroBudget::new();
    let (out, type_errors, incomplete, stopped) = {
        let mut defines = HashMap::new();
        let mut arena = Vec::new();
        let mut type_errors = Vec::new();
        let mut discarded = Vec::new();
        let mut incomplete = None;
        let mut incomplete_reasons = Vec::new();
        let mut messages = Vec::new();
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
            stopped: false,
            quiet_after: None,
            file_visitor: None,
            source: Some(&mut recorder),
            messages: &mut messages,
            line_segments,
            loop_values: Vec::new(),
            budget: &mut budget,
        };
        let (provisional, traces) = expand_seq(&mut state, &scan_macro_tokens(src));
        let stopped = state.stopped;
        let out = match realize_model_tokens(src, &provisional, &traces, state.budget) {
            Ok((out, _)) => out,
            Err(error) => {
                let span = provisional
                    .first()
                    .map(|token| token.span)
                    .unwrap_or(Span::new(0, 0));
                note_eval_failure(&mut state, span, error, "");
                provisional
            }
        };
        (out, type_errors, incomplete, stopped)
    };
    if !stopped && (recorder.cursor as usize) < src.len() {
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
    // The directive token already owns its terminating newline. A following
    // blank line is a text statement and must not be stripped again.
    let opener_owns_newline = opener.end > opener.start
        && source.as_bytes().get((opener.end - 1) as usize) == Some(&b'\n');
    let body = if opener_owns_newline {
        gap
    } else {
        gap.strip_prefix("\r\n")
            .or_else(|| gap.strip_prefix('\n'))
            .unwrap_or(gap)
    };
    body.chars().all(|ch| matches!(ch, ' ' | '\t' | '\r'))
}

/// Source ranges of `@#if` / `@#ifndef` branches that expansion discarded.
pub(crate) fn inactive_macro_spans(src: &str) -> Vec<Span> {
    let tokens = crate::lexer::tokenize(src);
    let (_, _, _, _, discarded, _, _, _, _) = expand_macros_traced_full(src, tokens, &[]);
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
    let (_, _, _, _, _, incomplete, _, include_seen, _) =
        expand_macros_traced_full(&source, crate::lexer::tokenize(&source), &[]);
    !include_seen && incomplete.is_none()
}

fn expand_macros_traced_full(
    src: &str,
    tokens: Vec<Token>,
    line_segments: &[(Span, u32)],
) -> ExpandTracedFull {
    let _ = tokens;
    let scanned = scan_macro_tokens(src);
    let mut budget = MacroBudget::new();
    let mut defines = HashMap::new();
    let mut arena = Vec::new();
    let mut type_errors = Vec::new();
    let mut discarded = Vec::new();
    let mut incomplete = None;
    let mut incomplete_reasons = Vec::new();
    let mut messages = Vec::new();
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
            stopped: false,
            quiet_after: None,
            file_visitor: None,
            source: None,
            messages: &mut messages,
            line_segments,
            loop_values: Vec::new(),
            budget: &mut budget,
        };
        let (provisional, provisional_traces) = expand_seq(&mut state, &scanned);
        let realized = realize_model_tokens(src, &provisional, &provisional_traces, state.budget);
        match realized {
            Ok((out, traces)) => (out, traces, state.include_seen),
            Err(error) => {
                let span = provisional
                    .first()
                    .map(|token| token.span)
                    .unwrap_or(Span::new(0, 0));
                note_eval_failure(&mut state, span, error, "");
                (provisional, provisional_traces, state.include_seen)
            }
        }
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
        messages,
    )
}

/// Macro-phase scan. Block comments and line comments are text. `@#` is a
/// directive only at the start of a line. `@{` is an interpolation anywhere
/// outside a directive, including inside comments and quotes. A directive's
/// trailing newline belongs to the directive so it is not copied twice.
pub(crate) fn scan_macro_tokens(src: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut bol = true;
    let mut text_start: Option<usize> = None;
    let flush = |tokens: &mut Vec<Token>, text_start: &mut Option<usize>, i: usize| {
        if let Some(start) = text_start.take() {
            if i > start {
                tokens.push(Token::new(TokenKind::Ident, Span::new(start, i)));
            }
        }
    };
    while i < src.len() {
        if bol {
            let bytes = src.as_bytes();
            let mut j = i;
            while j < src.len() && matches!(bytes[j], b' ' | b'\t') {
                j += 1;
            }
            if src[j..].starts_with("@#") {
                flush(&mut tokens, &mut text_start, i);
                i = push_directive(&mut tokens, src, j);
                bol = true;
                continue;
            }
        } else if src[i..].starts_with("@#") && !line_comment_hides(src, i) {
            // The model lexer accepts `@#` after other tokens on the line.
            // `@#if "hello"` and `beta = @#if 0` are directives.
            flush(&mut tokens, &mut text_start, i);
            i = push_directive(&mut tokens, src, i);
            bol = true;
            continue;
        }
        if src[i..].starts_with("@{") {
            flush(&mut tokens, &mut text_start, i);
            let end = scan_interp_end(src, i);
            tokens.push(Token::new(TokenKind::MacroInterp, Span::new(i, end)));
            i = end;
            bol = false;
            continue;
        }
        if text_start.is_none() {
            text_start = Some(i);
        }
        let ch = src[i..].chars().next().unwrap();
        i += ch.len_utf8();
        bol = ch == '\n';
    }
    flush(&mut tokens, &mut text_start, i);
    tokens.push(Token::new(TokenKind::Eof, Span::new(src.len(), src.len())));
    tokens
}

/// `//` before `@#` on the same line is a comment, not a directive.
fn line_comment_hides(src: &str, at: usize) -> bool {
    let line_start = src[..at].rfind('\n').map(|index| index + 1).unwrap_or(0);
    src[line_start..at].contains("//")
}

/// The directive token stops before its terminating newline. That newline is
/// skipped so it is not copied as model text and is not part of the span.
fn push_directive(tokens: &mut Vec<Token>, src: &str, at: usize) -> usize {
    let end = scan_directive_end(src, at);
    tokens.push(Token::new(TokenKind::MacroDir, Span::new(at, end)));
    let mut next = end;
    if src.as_bytes().get(next) == Some(&b'\n') {
        next += 1;
    }
    next
}

fn scan_directive_end(src: &str, at_mark: usize) -> usize {
    let bytes = src.as_bytes();
    let mut i = at_mark;
    let mut line_start = at_mark;
    let mut in_string = false;
    while i < src.len() {
        if in_string {
            if bytes[i] == b'"' {
                in_string = false;
            }
            if bytes[i] == b'\n' {
                line_start = i + 1;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b'\n' {
            let continued = crate::lexer::macro_line_continues(&src[line_start..i]);
            if !continued {
                return i;
            }
            i += 1;
            line_start = i;
            continue;
        }
        let ch = src[i..].chars().next().unwrap_or('\0');
        i += ch.len_utf8().max(1);
    }
    i
}

fn scan_interp_end(src: &str, at: usize) -> usize {
    let mut i = at + 2;
    let mut quoted = false;
    while i < src.len() {
        let ch = src[i..].chars().next().unwrap();
        if quoted {
            if ch == '"' {
                quoted = false;
            }
            i += ch.len_utf8();
            continue;
        }
        if ch == '"' {
            quoted = true;
            i += ch.len_utf8();
            continue;
        }
        if ch == '}' {
            return i + 1;
        }
        i += ch.len_utf8();
    }
    src.len()
}

struct EmitContrib {
    emit_start: usize,
    emit_end: usize,
    span: Span,
    frames: Vec<usize>,
    exact: bool,
}

/// Lex the concatenated macro output in .mod context. Comment and quote state
/// come from that text, including substitutions. Token spans point at the
/// written source; a joined name keeps the substituted lexeme.
fn realize_model_tokens(
    src: &str,
    tokens: &[Token],
    traces: &[TokenTrace],
    budget: &mut MacroBudget,
) -> Result<(Vec<Token>, Vec<TokenTrace>), MacroEvalError> {
    let mut emitted = String::new();
    let mut contribs = Vec::new();
    for (tok, trace) in tokens.iter().zip(traces) {
        if tok.kind == TokenKind::Eof {
            continue;
        }
        let text = tok.text(src);
        if text.is_empty() {
            continue;
        }
        let start = emitted.len();
        emitted.push_str(text);
        contribs.push(EmitContrib {
            emit_start: start,
            emit_end: emitted.len(),
            span: tok.span,
            frames: trace.frames.clone(),
            exact: tok.glue_left,
        });
    }
    let lexed = crate::lexer::tokenize(&emitted);
    budget.spend_work(contribs.len().saturating_add(lexed.len()).max(1))?;
    let mut out: Vec<Token> = Vec::new();
    let mut out_traces = Vec::new();
    let mut prev_end: Option<usize> = None;
    let mut cursor = 0usize;
    for tok in &lexed {
        if tok.kind == TokenKind::Eof {
            break;
        }
        let a = tok.span.start as usize;
        let b = tok.span.end as usize;
        while cursor < contribs.len() && contribs[cursor].emit_end <= a {
            cursor += 1;
        }
        let mut end = cursor;
        while end < contribs.len() && contribs[end].emit_start < b {
            end += 1;
        }
        let overlapping = &contribs[cursor..end];
        if overlapping.is_empty() {
            continue;
        }
        let first = &overlapping[0];
        let last = &overlapping[overlapping.len() - 1];
        let piece = tok.text(&emitted).to_string();
        let (span, lexeme) = if overlapping.len() == 1 {
            if let Some(mapped) = copy_subspan(&emitted, src, first, a, b) {
                (mapped, None)
            } else {
                (first.span, Some(piece.clone()))
            }
        } else {
            let start = if a <= first.emit_start {
                first.span.start
            } else {
                source_offset(first, a)
            };
            let end = if b >= last.emit_end {
                last.span.end
            } else {
                source_offset(last, b)
            };
            (
                Span {
                    start,
                    end: end.max(start),
                },
                Some(piece.clone()),
            )
        };
        let mut token = if let Some(lexeme) = lexeme {
            Token::with_lexeme(tok.kind, span, lexeme)
        } else {
            Token::new(tok.kind, span)
        };
        if let Some(prev) = prev_end {
            let gap_empty = emitted.get(prev..a).is_some_and(str::is_empty);
            let inside_generated = overlapping
                .iter()
                .any(|contrib| contrib.exact && prev > contrib.emit_start && a < contrib.emit_end);
            if let Some(previous) = out.last_mut() {
                let next_generated = token.lexeme.is_some();
                if inside_generated {
                    previous.expanded_adjacent_next = Some(gap_empty);
                } else if !gap_empty && (previous.lexeme.is_some() || next_generated) {
                    // A space in the replacement must not inherit adjacency
                    // from the shared `@{…}` span.
                    previous.expanded_adjacent_next = Some(false);
                }
            }
            // Keep `.` and `;` tight inside one generated line. `=` stays spaced.
            if gap_empty && inside_generated {
                let prev_byte = emitted
                    .as_bytes()
                    .get(..a)
                    .and_then(|bytes| bytes.last().copied());
                if matches!(token.kind, TokenKind::Dot | TokenKind::Semi) || prev_byte == Some(b'.')
                {
                    token.glue_left = true;
                }
            }
        }
        let mut frames = Vec::new();
        for contrib in overlapping {
            for id in &contrib.frames {
                if !frames.contains(id) {
                    frames.push(*id);
                }
            }
        }
        out_traces.push(TokenTrace { frames });
        out.push(token);
        prev_end = Some(b);
    }
    out.push(Token::new(TokenKind::Eof, Span::new(src.len(), src.len())));
    out_traces.push(TokenTrace { frames: Vec::new() });
    let origins = contribs
        .iter()
        .map(|contrib| crate::native_line::EmittedOrigin {
            emitted: Span::new(contrib.emit_start, contrib.emit_end),
            written: contrib.span,
            copied: emitted.get(contrib.emit_start..contrib.emit_end)
                == src.get(contrib.span.start as usize..contrib.span.end as usize),
        })
        .collect();
    let emitted = std::sync::Arc::new(crate::native_line::EmittedSource {
        text: emitted,
        origins,
    });
    for (token, lexed) in out.iter_mut().zip(&lexed) {
        token.emitted = Some(crate::native_line::EmittedToken {
            source: emitted.clone(),
            span: lexed.span,
        });
    }
    Ok((out, out_traces))
}

fn source_offset(contrib: &EmitContrib, emit_at: usize) -> u32 {
    let delta = emit_at.saturating_sub(contrib.emit_start);
    contrib.span.start.saturating_add(delta as u32)
}

fn copy_subspan(
    emitted: &str,
    src: &str,
    contrib: &EmitContrib,
    emit_from: usize,
    emit_to: usize,
) -> Option<Span> {
    let slice = src.get(contrib.span.start as usize..contrib.span.end as usize)?;
    let contributed = emitted.get(contrib.emit_start..contrib.emit_end)?;
    // Same length is not enough: `@{a}` and `beta` are both four bytes.
    if slice != contributed {
        return None;
    }
    let delta = emit_from.checked_sub(contrib.emit_start)?;
    let len = emit_to.checked_sub(emit_from)?;
    if delta.saturating_add(len) > slice.len() {
        return None;
    }
    Some(Span::new(
        contrib.span.start as usize + delta,
        contrib.span.start as usize + delta + len,
    ))
}

fn expand_seq(state: &mut ExpandState<'_, '_>, tokens: &[Token]) -> (Vec<Token>, Vec<TokenTrace>) {
    let mut out = Vec::new();
    let mut traces = Vec::new();
    let mut i = 0;
    let mut stack: Vec<IfFrame> = Vec::new();

    while i < tokens.len() {
        if state.stopped {
            let tok = &tokens[i];
            if tok.kind == TokenKind::Eof {
                emit(state, &mut out, &mut traces, tok.clone());
                break;
            }
            // A fatal macro failure ends this root. Later directives, echoes,
            // includes, and model text are not executed or emitted.
            i += 1;
            continue;
        }
        let tok = &tokens[i];
        if tok.kind == TokenKind::Eof {
            emit(state, &mut out, &mut traces, tok.clone());
            break;
        }
        if tok.kind == TokenKind::MacroDir {
            let mut owned = tok.clone();
            if owned.text(state.src).contains("\\\\") {
                let raw = owned.text(state.src).to_string();
                owned.lexeme = Some(crate::parser::collapse_continuations(&raw));
            }
            let tok = &owned;
            match dir_kind(state.src, tok) {
                Dir::Define => {
                    let em = emitting(&stack);
                    let mut kept = false;
                    if em {
                        match parse_define_eval(tok.text(state.src), state.defines, state.budget) {
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
                        Some(name) => !variable_defined(state.defines, &name),
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
                    if stack.is_empty() {
                        // `Tokenizer.ll` accepts `@#` only at line start. A
                        // mid-line `@#if` is text, so this `@#else` has no if.
                        push_type_error(
                            state,
                            tok.span,
                            "E062",
                            "syntax error, unexpected ELSE, expecting end of file".to_string(),
                        );
                        i += 1;
                        continue;
                    }
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
                            // A fatal failure already stopped this root. Do not
                            // copy the loop body back in after that abort.
                            if !state.stopped {
                                for original in &tokens[i..next] {
                                    emit(state, &mut out, &mut traces, original.clone());
                                }
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
                    let name = directive_name(tok.text(state.src)).to_ascii_lowercase();
                    if em && matches!(name.as_str(), "error" | "echo" | "echomacrovars" | "line") {
                        match exec_debug_directive(state, tok, &name) {
                            DebugOutcome::Done => {}
                            DebugOutcome::Failed => {
                                if !tok.comment_context {
                                    emit(state, &mut out, &mut traces, tok.clone());
                                    kept = true;
                                }
                            }
                            DebugOutcome::Generated { text, prepaid } => {
                                if !tok.comment_context {
                                    emit_generated(
                                        state,
                                        &mut out,
                                        &mut traces,
                                        tok.span,
                                        &text,
                                        prepaid,
                                    );
                                }
                            }
                        }
                    } else if name == "include" && em {
                        match eval_include_argument(state, tok) {
                            Ok(path) => {
                                let certain = state.incomplete.is_none();
                                let has_visitor = state.file_visitor.is_some();
                                let loaded = if let Err(error) = state.budget.spend_work(1) {
                                    note_eval_failure(state, tok.span, error, "");
                                    false
                                } else if state.budget.exec_depth
                                    >= crate::macro_expr::EXEC_DEPTH_CAP
                                {
                                    note_eval_failure(
                                        state,
                                        tok.span,
                                        MacroEvalError::Limit("execution depth"),
                                        "",
                                    );
                                    true
                                } else if let Some(visitor) = state.file_visitor.as_deref_mut() {
                                    state.budget.exec_depth += 1;
                                    let loaded = visitor.visit(
                                        tok.span,
                                        &path,
                                        state.defines,
                                        certain,
                                        &state.loop_values,
                                        state.budget,
                                    );
                                    state.budget.exec_depth -= 1;
                                    loaded
                                } else {
                                    false
                                };
                                if !loaded {
                                    // A missing include is fatal at the macro stage.
                                    // The workspace reports it as E061. Only a visitor
                                    // that refused the file marks this expansion
                                    // incomplete; a later parse with no visitor must
                                    // stay a model-incomplete file.
                                    state.include_seen = true;
                                    if has_visitor {
                                        state.incomplete.get_or_insert(tok.span);
                                        state.stopped = true;
                                    } else {
                                        state.quiet_after.get_or_insert(tok.span.end);
                                    }
                                }
                            }
                            Err(error) => {
                                let suppress = state.include_seen
                                    && matches!(
                                        error,
                                        MacroEvalError::UnknownVariable(_)
                                            | MacroEvalError::UnknownFunction(_)
                                    );
                                if suppress {
                                    state.incomplete.get_or_insert(tok.span);
                                } else {
                                    let expression = define_rhs_expression(tok.text(state.src));
                                    let fatal = error.fatal();
                                    note_eval_failure(state, tok.span, error, expression);
                                    if fatal {
                                        state.stopped = true;
                                    }
                                }
                                if !tok.comment_context {
                                    emit(state, &mut out, &mut traces, tok.clone());
                                    kept = true;
                                }
                            }
                        }
                    } else if directive_name(tok.text(state.src))
                        .eq_ignore_ascii_case("includepath")
                        && em
                    {
                        if let Some(path) = eval_includepath(state, tok) {
                            if let Some(visitor) = state.file_visitor.as_deref_mut() {
                                if !visitor.path(tok.span, &path, state.incomplete.is_none()) {
                                    // IncludePath::interpret throws before any later
                                    // lookup. The missing directory is E304; the
                                    // expansion is not a finished navigation source.
                                    state.incomplete.get_or_insert(tok.span);
                                    state.stopped = true;
                                }
                            }
                        } else {
                            state.incomplete.get_or_insert(tok.span);
                            if !tok.comment_context {
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
            }
            continue;
        }
        if tok.kind == TokenKind::MacroInterp
            || (tok.kind == TokenKind::String && tok.text(state.src).contains("@{"))
        {
            if emitting(&stack) {
                let replacements = if tok.kind == TokenKind::String {
                    subst_quoted(state.src, tok, state.defines, state.budget)
                        .map(|expansion| (expansion.tokens, expansion.substitutions))
                } else {
                    subst_interp(state.src, tok, state.defines, state.budget)
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
                        if !tok.comment_context {
                            emit(state, &mut out, &mut traces, tok.clone());
                        }
                    }
                }
            }
            i += 1;
            continue;
        }
        if emitting(&stack) && !tok.comment_context {
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

fn emit_generated(
    state: &mut ExpandState<'_, '_>,
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
    span: Span,
    text: &str,
    prepaid: usize,
) {
    if text.is_empty() {
        return;
    }
    // One exact lexeme. `glue_left` marks it so the model lexer keeps this
    // spacing instead of inventing spaces around `.` and `;`.
    let mut token = Token::with_lexeme(TokenKind::Ident, span, text);
    token.glue_left = true;
    token.output_prepaid = prepaid;
    emit(state, out, traces, token);
}

/// `@{...}` and `@#echo` use pinned `to_string`. Strings are copied raw.
/// `@#echomacrovars` uses [`crate::macro_expr::render_macro_val_budget`], which quotes.
fn interpolation_text(
    value: &MacroVal,
    budget: &mut MacroBudget,
) -> Result<String, MacroEvalError> {
    crate::macro_expr::render_interpolation_budget(value, budget)
}

fn note_prepaid(tokens: &mut [Token], mut prepaid: usize) {
    for token in tokens {
        let len = token.lexeme.as_ref().map(String::len).unwrap_or(0);
        let take = prepaid.min(len);
        token.output_prepaid = take;
        prepaid -= take;
    }
}

fn emit(
    state: &mut ExpandState<'_, '_>,
    out: &mut Vec<Token>,
    traces: &mut Vec<TokenTrace>,
    tok: Token,
) {
    if tok.kind != TokenKind::Eof {
        let bytes = tok.text(state.src).len().saturating_sub(tok.output_prepaid);
        if let Err(error) = state.budget.spend_output(bytes) {
            note_eval_failure(state, tok.span, error, "");
            return;
        }
    }
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

/// The macro text token owns the line break before the closer. Origins name
/// the written body, so that break is not part of the frame.
fn trimmed_body_span(src: &str, mut span: Span) -> Span {
    while span.end > span.start {
        match src.as_bytes().get((span.end - 1) as usize) {
            Some(b'\n' | b'\r') => span.end -= 1,
            _ => break,
        }
    }
    span
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
    let collection_expr = match &condition {
        Some(filter) => {
            let index = if vars.len() == 1 {
                vars[0].clone()
            } else {
                format!("({})", vars.join(", "))
            };
            format!("[{index} in ({collection}) when ({filter})]")
        }
        None => collection.clone(),
    };
    let values = match eval_macro_expr(&collection_expr, state.defines, state.budget) {
        Ok(values) => values,
        Err(error) => {
            note_eval_failure(state, for_tok.span, error, &collection_expr);
            return false;
        }
    };
    let items = match values {
        MacroVal::Array(items) if items.len() <= RANGE_CAP => items,
        other => {
            if oversized_collection(&other) {
                push_i211(state, for_tok.span, i211_limit_message("range size"));
            } else {
                push_type_error(
                    state,
                    for_tok.span,
                    "E285",
                    "The index must loop through an array".to_string(),
                );
            }
            return false;
        }
    };
    let mut planned = Vec::new();
    for value in &items {
        let value = match crate::macro_expr::clone_macro_val(value, state.budget) {
            Ok(value) => value,
            Err(error) => {
                note_eval_failure(state, for_tok.span, error, &collection_expr);
                return false;
            }
        };
        let members = match (&vars[..], &value) {
            ([_], _) => match crate::macro_expr::clone_macro_val(&value, state.budget) {
                Ok(member) => vec![member],
                Err(error) => {
                    note_eval_failure(state, for_tok.span, error, &collection_expr);
                    return false;
                }
            },
            (_, MacroVal::Tuple(tuple_items)) if tuple_items.len() == vars.len() => {
                let mut cloned = Vec::with_capacity(tuple_items.len());
                for item in tuple_items {
                    match crate::macro_expr::clone_macro_val(item, state.budget) {
                        Ok(item) => cloned.push(item),
                        Err(error) => {
                            note_eval_failure(state, for_tok.span, error, &collection_expr);
                            return false;
                        }
                    }
                }
                cloned
            }
            (_, MacroVal::Tuple(tuple_items)) => {
                push_type_error(
                    state,
                    for_tok.span,
                    "E284",
                    format!(
                        "Encountered tuple of size {} but only have {} index variables",
                        tuple_items.len(),
                        vars.len()
                    ),
                );
                return false;
            }
            _ => Vec::new(),
        };
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
    let body_span = trimmed_body_span(state.src, tokens_body_span(body));
    // A collection or `when` filter that yields no iteration leaves the written
    // body inactive; mark it discarded like an untaken `@#if` branch.
    if planned.is_empty() && body_span.end > body_span.start {
        push_discarded(state, body_span.start, body_span.end);
    }
    for (value, members) in planned {
        if let Err(error) = state.budget.spend_work(1) {
            note_eval_failure(state, for_tok.span, error, "");
            break;
        }
        let shown = match crate::macro_expr::render_macro_val_budget(&value, false, state.budget) {
            Ok(shown) => shown,
            Err(error) => {
                note_eval_failure(state, for_tok.span, error, &collection_expr);
                break;
            }
        };
        let mut printed_members = Vec::with_capacity(members.len());
        let mut render_failed = false;
        for member in &members {
            match crate::macro_expr::render_macro_val_budget(member, false, state.budget) {
                Ok(printed) => printed_members.push(printed),
                Err(error) => {
                    note_eval_failure(state, for_tok.span, error, &collection_expr);
                    render_failed = true;
                    break;
                }
            }
        }
        if render_failed {
            break;
        }
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
            value: Some(shown),
        });
        state.origin_stack.push(frame_id);
        let saved_bindings = std::mem::take(&mut state.loop_values);
        let mut bindings = saved_bindings.clone();
        for (name, printed) in vars.iter().zip(printed_members) {
            if let Some(slot) = bindings.iter_mut().find(|(bound, _)| bound == name) {
                slot.1 = printed;
            } else {
                bindings.push((name.clone(), printed));
            }
        }
        state.loop_values = bindings;
        if state.budget.exec_depth >= crate::macro_expr::EXEC_DEPTH_CAP {
            state.loop_values = saved_bindings;
            state.origin_stack.pop();
            note_eval_failure(
                state,
                for_tok.span,
                MacroEvalError::Limit("execution depth"),
                &collection_expr,
            );
            break;
        }
        if let Some(recorder) = state.source.as_deref_mut() {
            recorder.begin_loop_body(body_text.start);
        }
        state.budget.exec_depth += 1;
        let (expanded, expanded_traces) = expand_seq(state, body);
        state.budget.exec_depth -= 1;
        state.loop_values = saved_bindings;
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
        if state.stopped {
            break;
        }
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
    defines: &mut HashMap<String, MacroVal>,
    budget: &mut MacroBudget,
) -> Result<(String, Vec<Token>), MacroEvalError> {
    let text = tok.text(src);
    let inner = text
        .strip_prefix("@{")
        .and_then(|s| s.strip_suffix('}'))
        .ok_or(MacroEvalError::Unsupported)?
        .trim();
    let val = eval_macro_expr(inner, defines, budget)?;
    let repl = interpolation_text(&val, budget)?;
    // The replacement is text. The model lexer reads the whole emitted stream
    // later, so a value may open a comment, close a quote, or join a name.
    // Generated `@#` and `@{` stay characters; that later lexer does not run
    // the macro processor again. Rendering already charged these bytes.
    if repl.is_empty() {
        return Ok((repl, Vec::new()));
    }
    let mut token = Token::with_lexeme(TokenKind::Ident, tok.span, repl.clone());
    token.glue_left = true;
    token.output_prepaid = repl.len();
    Ok((repl, vec![token]))
}

/// Substitute inside a quoted .mod value without changing its string boundary.
/// Dynare expands before string lexing. A replacement that closes the quote or
/// adds a line needs a surrounding-source lexer pass and stays incomplete here.
struct QuotedExpansion {
    tokens: Vec<Token>,
    substitutions: Vec<(Span, String)>,
}

fn subst_quoted(
    src: &str,
    tok: &Token,
    defines: &mut HashMap<String, MacroVal>,
    budget: &mut MacroBudget,
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
    let mut prepaid = 0usize;
    let mut cursor = 0;
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
        match eval_macro_expr(&text[body..end], defines, budget) {
            Ok(value) => {
                let replacement =
                    interpolation_text(&value, budget).map_err(|error| (span, error))?;
                prepaid = prepaid.saturating_add(replacement.len());
                output.push_str(&replacement);
                substitutions.push((span, replacement));
            }
            Err(error) => return Err((span, error)),
        }
        cursor = end + 1;
    }
    output.push_str(&text[cursor..]);
    let generated = crate::lexer::tokenize(&output);
    let pieces: Vec<_> = generated
        .iter()
        .filter(|piece| piece.kind != TokenKind::Eof)
        .collect();
    let mut tokens = if pieces.len() == 1 && pieces[0].kind == TokenKind::String {
        vec![Token::with_lexeme(TokenKind::String, tok.span, output)]
    } else {
        pieces
            .iter()
            .map(|piece| Token::with_lexeme(piece.kind, tok.span, piece.text(&output)))
            .collect()
    };
    note_prepaid(&mut tokens, prepaid);
    Ok(QuotedExpansion {
        tokens,
        substitutions,
    })
}

/// One `@#for` whose body contains `site`, outermost first.
#[derive(Clone, Debug)]
pub(crate) struct ContainingLoop {
    pub header: Span,
    pub variables: Vec<String>,
    pub body_start: u32,
    pub body_end: u32,
    pub closer: Span,
}

pub(crate) fn loops_containing(source: &str, site: Span) -> Vec<ContainingLoop> {
    let tokens = scan_macro_tokens(source);
    let mut open = Vec::new();
    let mut pairs = Vec::new();
    for token in &tokens {
        if token.kind != TokenKind::MacroDir {
            continue;
        }
        match dir_kind(source, token) {
            Dir::For => {
                let variables = parse_for(token.text(source))
                    .map(|(variables, _, _)| variables)
                    .unwrap_or_default();
                open.push((token.span, variables));
            }
            Dir::Endfor => {
                if let Some((header, variables)) = open.pop() {
                    pairs.push(ContainingLoop {
                        header,
                        variables,
                        body_start: header.end,
                        body_end: token.span.start,
                        closer: token.span,
                    });
                }
            }
            _ => {}
        }
    }
    let mut found: Vec<_> = pairs
        .into_iter()
        .filter(|pair| pair.header.start <= site.start && pair.closer.end >= site.end)
        .collect();
    found.sort_by_key(|pair| pair.header.start);
    found
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
    dir_arg_ident(tok.text(state.src), kw)
        .is_some_and(|name| variable_defined(state.defines, &name))
}

fn variable_defined(defines: &HashMap<String, MacroVal>, name: &str) -> bool {
    defines
        .get(name)
        .is_some_and(|value| !matches!(value, MacroVal::Function { .. } | MacroVal::Unresolved))
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

fn eval_include_argument(
    state: &mut ExpandState<'_, '_>,
    tok: &Token,
) -> Result<String, MacroEvalError> {
    let Some(argument) = strip_kw(tok.text(state.src), "include") else {
        return Err(MacroEvalError::SyntaxUnexpected("INCLUDE"));
    };
    let argument = strip_line_comment(argument).trim();
    if argument.is_empty() {
        return Err(MacroEvalError::SyntaxEol);
    }
    match eval_macro_expr(argument, state.defines, state.budget)? {
        MacroVal::Text(path) => Ok(path),
        _ => Err(MacroEvalError::Official {
            code: "E305",
            message: "File name does not evaluate to a string".to_string(),
        }),
    }
}

enum DebugOutcome {
    Done,
    Failed,
    Generated { text: String, prepaid: usize },
}

fn exec_debug_directive(state: &mut ExpandState<'_, '_>, tok: &Token, name: &str) -> DebugOutcome {
    let text = tok.text(state.src);
    let argument = strip_kw(text, name).unwrap_or("");
    let argument = strip_line_comment(argument).trim();
    match name {
        "line" => match line_directive(argument) {
            Ok(()) => {
                // The pin emits one newline per source line the directive occupies.
                let count = 1 + state.src[tok.span.start as usize..tok.span.end as usize]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count();
                DebugOutcome::Generated {
                    text: "\n".repeat(count),
                    prepaid: 0,
                }
            }
            Err(message) => {
                push_type_error(state, tok.span, "E062", message);
                DebugOutcome::Failed
            }
        },
        "error" => {
            if argument.is_empty() {
                push_type_error(
                    state,
                    tok.span,
                    "E064",
                    "Macro-processing error".to_string(),
                );
                return DebugOutcome::Done;
            }
            match eval_macro_expr(argument, state.defines, state.budget) {
                Ok(value) => {
                    let body = match interpolation_text(&value, state.budget) {
                        Ok(body) => body,
                        Err(error) => {
                            note_eval_failure(state, tok.span, error, argument);
                            return DebugOutcome::Failed;
                        }
                    };
                    let message = if body.is_empty() {
                        "Macro-processing error".to_string()
                    } else {
                        format!("Macro-processing error: {body}")
                    };
                    push_type_error(state, tok.span, "E064", message);
                    DebugOutcome::Done
                }
                Err(error) => {
                    note_eval_failure(state, tok.span, error, argument);
                    DebugOutcome::Failed
                }
            }
        }
        "echo" => match eval_macro_expr(argument, state.defines, state.budget) {
            Ok(value) => match interpolation_text(&value, state.budget) {
                Ok(text) => {
                    push_macro_message(state, "echo", text, tok.span);
                    DebugOutcome::Done
                }
                Err(error) => {
                    note_eval_failure(state, tok.span, error, argument);
                    DebugOutcome::Failed
                }
            },
            Err(error) => {
                note_eval_failure(state, tok.span, error, argument);
                DebugOutcome::Failed
            }
        },
        "echomacrovars" => match parse_macrovars_argument(argument) {
            Ok((save, names)) => {
                let line = written_line_number(state.src, tok.span, state.line_segments);
                match format_macrovars(state.defines, &names, line, save, state.budget) {
                    Ok((text, prepaid)) => {
                        if save {
                            DebugOutcome::Generated { text, prepaid }
                        } else {
                            push_macro_message(state, "macrovars", text, tok.span);
                            DebugOutcome::Done
                        }
                    }
                    Err(error) => {
                        note_eval_failure(state, tok.span, error, argument);
                        DebugOutcome::Failed
                    }
                }
            }
            Err(message) => {
                push_type_error(state, tok.span, "E062", message);
                DebugOutcome::Failed
            }
        },
        _ => DebugOutcome::Done,
    }
}

fn line_directive(argument: &str) -> Result<(), String> {
    let mut lexer = ArgLexer::new(argument);
    match lexer.next_token() {
        ArgTok::Quoted(_) => {}
        other => {
            return Err(syntax_expecting(other.name(), "QUOTED_STRING"));
        }
    }
    match lexer.next_token() {
        ArgTok::Number(_) => {}
        other => return Err(syntax_expecting(other.name(), "NUMBER")),
    }
    match lexer.next_token() {
        ArgTok::End => Ok(()),
        other => Err(syntax_expecting(other.name(), "EOL")),
    }
}

fn source_line_number(source: &str, span: Span) -> usize {
    let end = (span.start as usize).min(source.len());
    source[..end].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn written_line_number(source: &str, span: Span, segments: &[(Span, u32)]) -> usize {
    for (segment, base_line) in segments {
        if span.start >= segment.start && span.start < segment.end {
            let from = segment.start as usize;
            let to = (span.start as usize).min(source.len());
            let extra = source[from..to]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            return *base_line as usize + extra;
        }
    }
    source_line_number(source, span)
}

fn syntax_expecting(found: &str, expected: &str) -> String {
    format!("syntax error, unexpected {found}, expecting {expected}")
}

fn parse_macrovars_argument(argument: &str) -> Result<(bool, Vec<String>), String> {
    let mut lexer = ArgLexer::new(argument);
    let mut save = false;
    let mut names = Vec::new();
    match lexer.next_token() {
        ArgTok::End => return Ok((false, names)),
        ArgTok::LParen => {
            match lexer.next_token() {
                ArgTok::Ident(name) if name.eq_ignore_ascii_case("save") => {}
                other => return Err(syntax_expecting(other.name(), "save")),
            }
            match lexer.next_token() {
                ArgTok::RParen => {}
                other => return Err(syntax_expecting(other.name(), "RPAREN")),
            }
            save = true;
        }
        ArgTok::Ident(name) => names.push(name),
        other => return Err(syntax_expecting(other.name(), "EOL")),
    }
    loop {
        match lexer.next_token() {
            ArgTok::End => return Ok((save, names)),
            ArgTok::Ident(name) => names.push(name),
            other => return Err(syntax_expecting(other.name(), "EOL")),
        }
    }
}

#[allow(dead_code)]
enum ArgTok {
    Ident(String),
    Number(String),
    Quoted(String),
    LParen,
    RParen,
    End,
    Other(&'static str),
}

impl ArgTok {
    fn name(&self) -> &str {
        match self {
            Self::Ident(_) => "IDENTIFIER",
            Self::Number(_) => "NUMBER",
            Self::Quoted(_) => "QUOTED_STRING",
            Self::LParen => "LPAREN",
            Self::RParen => "RPAREN",
            Self::End => "EOL",
            Self::Other(name) => name,
        }
    }
}

struct ArgLexer<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> ArgLexer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn next_token(&mut self) -> ArgTok {
        let bytes = self.src.as_bytes();
        while self.pos < bytes.len() && matches!(bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n') {
            self.pos += 1;
        }
        if self.pos >= bytes.len() {
            return ArgTok::End;
        }
        let rest = &self.src[self.pos..];
        if rest.starts_with("//") {
            return ArgTok::End;
        }
        let ch = rest.chars().next().unwrap();
        if ch == '(' {
            self.pos += 1;
            return ArgTok::LParen;
        }
        if ch == ')' {
            self.pos += 1;
            return ArgTok::RParen;
        }
        if ch == '"' {
            if let Some(end) = rest[1..].find('"') {
                let inner = rest[1..1 + end].to_string();
                self.pos += end + 2;
                return ArgTok::Quoted(inner);
            }
            self.pos = self.src.len();
            return ArgTok::Other("TEXT");
        }
        if ch.is_ascii_digit() || ch == '.' {
            let start = self.pos;
            self.pos += ch.len_utf8();
            while self.pos < bytes.len() {
                let c = self.src[self.pos..].chars().next().unwrap();
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-') {
                    self.pos += c.len_utf8();
                } else {
                    break;
                }
            }
            return ArgTok::Number(self.src[start..self.pos].to_string());
        }
        if ch.is_ascii_alphabetic() || ch == '_' {
            let start = self.pos;
            self.pos += ch.len_utf8();
            while self.pos < bytes.len() {
                let c = self.src[self.pos..].chars().next().unwrap();
                if c.is_ascii_alphanumeric() || c == '_' {
                    self.pos += c.len_utf8();
                } else {
                    break;
                }
            }
            return ArgTok::Ident(self.src[start..self.pos].to_string());
        }
        self.pos += ch.len_utf8();
        ArgTok::Other("TEXT")
    }
}

fn format_macrovars(
    defines: &HashMap<String, MacroVal>,
    selected: &[String],
    line: usize,
    save: bool,
    budget: &mut MacroBudget,
) -> Result<(String, usize), MacroEvalError> {
    let wanted = |name: &str| {
        selected.is_empty() || selected.iter().any(|item| item.eq_ignore_ascii_case(name))
    };
    let mut variables = Vec::new();
    let mut functions = Vec::new();
    for (name, value) in defines {
        match value {
            MacroVal::Function { .. } => functions.push(name.clone()),
            MacroVal::Unresolved => {}
            _ => variables.push(name.clone()),
        }
    }
    let order =
        |left: &String, right: &String| left.to_ascii_lowercase().cmp(&right.to_ascii_lowercase());
    variables.sort_by(order);
    functions.sort_by(order);
    let digits = if line == 0 {
        1
    } else {
        (line.ilog10() as usize) + 1
    };
    let mut lines: Vec<MacrovarLine> = Vec::new();
    let mut prepaid = 0usize;
    let mut total = 0usize;
    if !save && !variables.is_empty() {
        total = total.saturating_add("Macro Variables (at line ".len() + digits + "):\n".len());
    }
    let variable_names: Vec<_> = if selected.is_empty() {
        variables.clone()
    } else {
        selected
            .iter()
            .filter(|name| variables.iter().any(|have| have.eq_ignore_ascii_case(name)))
            .cloned()
            .collect()
    };
    for name in variable_names {
        let Some(value) = defines.get(&name) else {
            continue;
        };
        if !save && !wanted(&name) {
            continue;
        }
        let printed = crate::macro_expr::render_macro_val_budget(value, save, budget)?;
        prepaid = prepaid.saturating_add(printed.len());
        let wrapper = if save {
            "options_.macrovars_line_".len() + digits + 1 + name.len() + " = ".len() + ";\n".len()
        } else {
            "  ".len() + name.len() + " = ".len() + "\n".len()
        };
        total = total.saturating_add(wrapper).saturating_add(printed.len());
        lines.push(MacrovarLine::Variable { name, printed });
    }
    if !save && !functions.is_empty() {
        total = total.saturating_add("Macro Functions (at line ".len() + digits + "):\n".len());
    }
    let function_names: Vec<_> = if selected.is_empty() {
        functions.clone()
    } else {
        selected
            .iter()
            .filter(|name| functions.iter().any(|have| have.eq_ignore_ascii_case(name)))
            .cloned()
            .collect()
    };
    for name in function_names {
        let Some(MacroVal::Function { params, body }) = defines.get(&name) else {
            continue;
        };
        let printed = crate::macro_expr::print_expression_budget(body, budget)?;
        let joined = params.len().saturating_sub(1).saturating_mul(2);
        let signature_len =
            name.len() + 1 + params.iter().map(String::len).sum::<usize>() + joined + 1;
        let wrapper = if save {
            "options_.macrovars_line_".len()
                + digits
                + ".function.".len()
                + name.len()
                + " = '".len()
                + signature_len
                + " = ".len()
                + "';\n".len()
        } else {
            "  ".len() + signature_len + " = ".len() + "\n".len()
        };
        total = total.saturating_add(wrapper).saturating_add(printed.len());
        lines.push(MacrovarLine::Function {
            name,
            params: params.clone(),
            printed,
        });
    }
    if total > crate::macro_expr::STRING_CAP {
        return Err(MacroEvalError::Limit("string size"));
    }
    let wrappers = total.saturating_sub(prepaid);
    budget.spend_work(total.max(1))?;
    budget.spend_output(wrappers)?;
    let mut text = String::with_capacity(total);
    if !save && !variables.is_empty() {
        text.push_str("Macro Variables (at line ");
        text.push_str(&line.to_string());
        text.push_str("):\n");
    }
    for line_item in &lines {
        match line_item {
            MacrovarLine::Variable { name, printed } => {
                if save {
                    text.push_str("options_.macrovars_line_");
                    text.push_str(&line.to_string());
                    text.push('.');
                    text.push_str(name);
                    text.push_str(" = ");
                    text.push_str(printed);
                    text.push_str(";\n");
                } else {
                    text.push_str("  ");
                    text.push_str(name);
                    text.push_str(" = ");
                    text.push_str(printed);
                    text.push('\n');
                }
            }
            MacrovarLine::Function { .. } => {}
        }
    }
    if !save && !functions.is_empty() {
        text.push_str("Macro Functions (at line ");
        text.push_str(&line.to_string());
        text.push_str("):\n");
    }
    for line_item in &lines {
        let MacrovarLine::Function {
            name,
            params,
            printed,
        } = line_item
        else {
            continue;
        };
        let signature = format!("{name}({})", params.join(", "));
        if save {
            text.push_str("options_.macrovars_line_");
            text.push_str(&line.to_string());
            text.push_str(".function.");
            text.push_str(name);
            text.push_str(" = '");
            text.push_str(&signature);
            text.push_str(" = ");
            text.push_str(printed);
            text.push_str("';\n");
        } else {
            text.push_str("  ");
            text.push_str(&signature);
            text.push_str(" = ");
            text.push_str(printed);
            text.push('\n');
        }
    }
    Ok((text, prepaid))
}

enum MacrovarLine {
    Variable {
        name: String,
        printed: String,
    },
    Function {
        name: String,
        params: Vec<String>,
        printed: String,
    },
}

const MESSAGE_CAP: usize = 1_000_000;

fn push_macro_message(
    state: &mut ExpandState<'_, '_>,
    kind: &'static str,
    message: String,
    span: Span,
) {
    if state.quiet_after.is_some_and(|at| span.start >= at) {
        return;
    }
    let used: usize = state.messages.iter().map(|item| item.message.len()).sum();
    if used.saturating_add(message.len()) > MESSAGE_CAP {
        push_i211(state, span, i211_limit_message("message size"));
        state.stopped = true;
        return;
    }
    state.messages.push(MacroMessage {
        kind,
        message,
        span,
        file: None,
    });
}

fn eval_condition(state: &mut ExpandState<'_, '_>, tok: &Token, kw: &str) -> Option<bool> {
    let arg = strip_kw(tok.text(state.src), kw)?;
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    match eval_macro_expr(arg, state.defines, state.budget) {
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
    match eval_macro_expr(argument, state.defines, state.budget) {
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
    defines: &mut HashMap<String, MacroVal>,
    budget: &mut MacroBudget,
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
        let params = if after_open[..close].trim().is_empty() {
            Vec::new()
        } else {
            let params: Vec<_> = after_open[..close]
                .split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect();
            if params.iter().any(|param| !is_simple_ident(param)) {
                return Err(MacroEvalError::SyntaxUnexpected("TEXT"));
            }
            params
        };
        let Some(body) = after_open[close + 1..].trim_start().strip_prefix('=') else {
            return Err(MacroEvalError::SyntaxUnexpected("EOL"));
        };
        let body = strip_line_comment(body).trim();
        crate::macro_expr::check_macro_syntax_budget(body, budget)?;
        budget.spend_work(body.len().max(1))?;
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
    Ok(Some((name, eval_macro_expr(body, defines, budget)?)))
}

fn eval_macro_expr(
    source: &str,
    defines: &mut HashMap<String, MacroVal>,
    budget: &mut MacroBudget,
) -> Result<MacroVal, MacroEvalError> {
    crate::macro_expr::eval_macro_expr_budget(source, defines, budget)
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

fn dir_arg_ident(text: &str, kw: &str) -> Option<String> {
    let rest = strip_kw(text, kw)?;
    let rest = rest.trim_start();
    let n = ident_len(rest)?;
    Some(rest[..n].to_string())
}

fn strip_kw<'a>(text: &'a str, kw: &str) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix("@#")?;
    strip_word(rest.trim_start(), kw)
}

fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let head = text.get(..word.len())?;
    if !head.eq_ignore_ascii_case(word) {
        return None;
    }
    let after = text.get(word.len()..)?;
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
