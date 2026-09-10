//! Native recursive-descent parser over the token stream.

use std::collections::HashMap;

use crate::expr::{BinOp, ExprId, ExprKind, UnOp};
use crate::intern::{Interner, Name};
use crate::lexer::{tokenize, Token, TokenKind};
use crate::macro_expand::expand_macros;
use crate::model::{
    Assignment, Decl, DeprecatedOption, Equation, EstimatedParam, EstimatedParamKind,
    IncludeDirective, IncludePathDirective, MacroDirective, MacroInterp, Model, ObservedVar,
    ParseIssue, ParseIssueKind, PolicyCommand, ShockKind, ShockStmt, ShocksSemiFamily,
};
use crate::span::Span;

pub fn parse(text: &str) -> Model {
    let source = normalize_newlines(text);
    let raw_tokens = tokenize(&source);
    let (includes, includepaths, macro_directives, macro_interps) =
        collect_include_dirs(&source, &raw_tokens);
    let tokens = expand_macros(&source, raw_tokens);
    let mut p = Parser {
        src: &source,
        tokens,
        i: 0,
        intern: Interner::default(),
        model: Model::default(),
    };
    p.parse_file();
    p.model
        .exogenous
        .extend(p.model.deterministic_exogenous.iter().cloned());
    p.model
        .exogenous
        .sort_by_key(|d| (d.span.start, d.span.end));
    p.record_keyword_typos();
    p.record_missing_assign_semis();
    let Parser {
        intern, mut model, ..
    } = p;
    model.source = source;
    model.intern = intern;
    model.includes = includes;
    model.includepaths = includepaths;
    model.macro_directives = macro_directives;
    model.macro_interps = macro_interps;
    model
}

const UNARY_BP: u8 = 7;

/// Static copy of Python `_KEYWORD_TYPO_MAP`.
const KEYWORD_TYPO_MAP: &[(&str, &str)] = &[
    ("mdoel", "model"),
    ("modle", "model"),
    ("modl", "model"),
    ("modelo", "model"),
    ("mdel", "model"),
    ("moel", "model"),
    ("modeel", "model"),
    ("moedl", "model"),
    ("mmodel", "model"),
    ("modell", "model"),
    ("paramters", "parameters"),
    ("parametrs", "parameters"),
    ("paramaters", "parameters"),
    ("paremeters", "parameters"),
    ("parametres", "parameters"),
    ("paraemters", "parameters"),
    ("paramteres", "parameters"),
    ("parmaeters", "parameters"),
    ("prameters", "parameters"),
    ("parmeters", "parameters"),
    ("parametes", "parameters"),
    ("parametera", "parameters"),
    ("paramter", "parameters"),
    ("parametr", "parameters"),
    ("variable", "var"),
    ("vars", "var"),
    ("vasr", "var"),
    ("varexoo", "varexo"),
    ("varrexo", "varexo"),
    ("vaarexo", "varexo"),
    ("varxeo", "varexo"),
    ("vaxero", "varexo"),
    ("varexo0", "varexo"),
    ("shokcs", "shocks"),
    ("shcoks", "shocks"),
    ("shokc", "shocks"),
    ("schocks", "shocks"),
    ("shoks", "shocks"),
    ("initvla", "initval"),
    ("inival", "initval"),
    ("intivals", "initval"),
    ("initvall", "initval"),
    ("initavl", "initval"),
    ("staedy", "steady"),
    ("steday", "steady"),
];

const ASSIGN_FOLLOWERS: &[&str] = &[
    "model",
    "model_remove",
    "model_replace",
    "var",
    "var_remove",
    "varexo",
    "varexo_det",
    "parameters",
    "predetermined_variables",
    "initval",
    "endval",
    "shocks",
    "steady_state_model",
    "steady",
    "check",
    "resid",
    "stoch_simul",
    "simul",
    "estimation",
    "osr",
    "calib_smoother",
    "forecast",
    "identification",
    "dynasave",
    "dynatype",
    "model_diagnostics",
    "model_info",
    "perfect_foresight_setup",
    "perfect_foresight_solver",
];

const ASSIGN_BLOCK_LIKE: &[&str] = &[
    "var",
    "varexo",
    "varexo_det",
    "parameters",
    "model",
    "predetermined_variables",
    "initval",
    "endval",
    "shocks",
    "steady_state_model",
    "end",
    "log",
];

const TERMINAL_COMMANDS: &[&str] = &[
    "stoch_simul",
    "estimation",
    "simul",
    "perfect_foresight_solver",
    "ramsey_policy",
    "discretionary_policy",
    "osr",
    "sensitivity",
    "dynare_sensitivity",
    "send_endogenous_variables_to_workspace",
];

const BLOCK_OPENERS: &[&str] = &["model", "initval", "endval", "shocks", "steady_state_model"];

const PRIOR_SHAPES: &[&str] = &[
    "beta_pdf",
    "gamma_pdf",
    "normal_pdf",
    "inv_gamma_pdf",
    "inv_gamma1_pdf",
    "inv_gamma2_pdf",
    "uniform_pdf",
    "weibull_pdf",
];

enum ExprStop {
    EqOrSemi,
    Semi,
}

fn normalize_newlines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && (i + 1 >= bytes.len() || bytes[i + 1] != b'\n') {
            out.push('\n');
            i += 1;
        } else {
            let ch = text[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out
}

fn collect_include_dirs(
    src: &str,
    tokens: &[Token],
) -> (
    Vec<IncludeDirective>,
    Vec<IncludePathDirective>,
    Vec<MacroDirective>,
    Vec<MacroInterp>,
) {
    let mut includes = Vec::new();
    let mut includepaths = Vec::new();
    let mut macro_directives = Vec::new();
    let mut macro_interps = Vec::new();
    for tok in tokens {
        match tok.kind {
            TokenKind::MacroDir => {
                let text = collapse_continuations(tok.text(src));
                if let Some(filename) = literal_include_filename(&text) {
                    includes.push(IncludeDirective {
                        filename,
                        span: tok.span,
                    });
                } else if let Some(argument) = includepath_argument(&text) {
                    includepaths.push(IncludePathDirective {
                        argument,
                        span: tok.span,
                    });
                }
                if let Some((kind, argument)) = parse_dir_kind_arg(&text) {
                    if kind != "include" {
                        macro_directives.push(MacroDirective {
                            kind,
                            argument,
                            span: tok.span,
                        });
                    }
                }
            }
            TokenKind::MacroInterp => {
                let text = tok.text(src);
                macro_interps.push(MacroInterp {
                    inner: interp_inner(text),
                    span: tok.span,
                });
            }
            _ => {}
        }
    }
    (includes, includepaths, macro_directives, macro_interps)
}

fn parse_dir_kind_arg(text: &str) -> Option<(String, Option<String>)> {
    let rest = text.trim_start().strip_prefix("@#")?;
    let rest = rest.trim_start();
    let ident = leading_ident(rest)?;
    let kind = ident.to_ascii_lowercase();
    let arg = rest[ident.len()..].trim();
    let argument = if arg.is_empty() {
        None
    } else {
        Some(arg.to_string())
    };
    Some((kind, argument))
}

fn interp_inner(text: &str) -> String {
    let rest = text.strip_prefix("@{").unwrap_or(text);
    rest.strip_suffix('}').unwrap_or(rest).to_string()
}

fn collapse_continuations(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                j += 1;
            }
            if j < bytes.len() && (bytes[j] == b'\n' || bytes[j] == b'\r') {
                let mut k = i;
                while k > 0 && (bytes[k - 1] == b' ' || bytes[k - 1] == b'\t') {
                    k -= 1;
                }
                while out.len() > k {
                    out.pop();
                }
                out.push(' ');
                if bytes[j] == b'\r' && j + 1 < bytes.len() && bytes[j + 1] == b'\n' {
                    j += 2;
                } else {
                    j += 1;
                }
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        let ch = text[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn directive_arg<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = text.trim_start().strip_prefix("@#")?;
    strip_word_ci(rest.trim_start(), keyword)
}

fn strip_word_ci<'a>(text: &'a str, word: &str) -> Option<&'a str> {
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

fn literal_include_filename(text: &str) -> Option<String> {
    let rest = directive_arg(text, "include")?;
    let argument = rest.trim();
    if argument.is_empty() {
        return None;
    }
    if let Some(inner) = strip_quoted(argument) {
        return Some(inner.to_string());
    }
    bare_include_literal(argument)
}

fn includepath_argument(text: &str) -> Option<String> {
    let rest = directive_arg(text, "includepath")?;
    let argument = rest.trim();
    if argument.is_empty() {
        return None;
    }
    Some(argument.to_string())
}

fn strip_quoted(s: &str) -> Option<&str> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0];
    if q != b'"' && q != b'\'' {
        return None;
    }
    if bytes[bytes.len() - 1] != q {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    if inner.as_bytes().contains(&q) {
        return None;
    }
    Some(inner)
}

/// Python `_bare_include_literal`: a path, not a bare identifier / expression.
fn bare_include_literal(argument: &str) -> Option<String> {
    let raw = argument.trim();
    if raw.is_empty() {
        return None;
    }
    if raw
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '[' | ']' | '+'))
    {
        return None;
    }
    if is_ident_only(raw) {
        return None;
    }
    Some(raw.to_string())
}

fn is_ident_only(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    i: usize,
    intern: Interner,
    model: Model,
}

impl Parser<'_> {
    fn parse_file(&mut self) {
        while !self.at(TokenKind::Eof) {
            if self.at_ident_ci("var") {
                let decls = self.parse_declaration("var");
                self.model.endogenous.extend(decls);
            } else if self.at_ident_ci("varexo_det") {
                let decls = self.parse_declaration("varexo_det");
                self.model.deterministic_exogenous.extend(decls);
            } else if self.at_ident_ci("varexo") {
                let decls = self.parse_declaration("varexo");
                self.model.exogenous.extend(decls);
            } else if self.at_ident_ci("parameters") {
                let decls = self.parse_declaration("parameters");
                self.model.parameters.extend(decls);
            } else if self.at_ident_ci("predetermined_variables") {
                let decls = self.parse_declaration("predetermined_variables");
                self.model.predetermined.extend(decls);
            } else if self.at_ident_ci("model") {
                self.parse_model_block();
            } else if self.at_ident_ci("steady_state_model") {
                self.parse_ss_block();
            } else if self.at_ident_ci("initval") {
                self.parse_initval_block();
            } else if self.at_ident_ci("endval") {
                self.parse_endval_block();
            } else if self.at_ident_ci("shocks") {
                self.parse_shocks_block(true);
            } else if self.at_ident_ci("mshocks") {
                self.parse_shocks_block(false);
            } else if self.at_ident_ci("varobs") {
                self.parse_varobs();
            } else if self.at_ident_ci("estimated_params") {
                self.parse_estimated_params_block();
            } else if self.at_ident_ci("observation_trends") {
                self.parse_observation_trends_block();
            } else if self.at_ident_ci("planner_objective") {
                self.parse_planner_objective();
            } else if self.at_ident_ci("osr_params") {
                self.parse_osr_params();
            } else if self.at_ident_ci("optim_weights") {
                self.model.has_optim_weights = true;
                self.skip_block();
            } else if let Some(command) = self.at_policy_command() {
                self.parse_policy_command(command);
            } else if self.at_skipped_block() {
                self.skip_block();
            } else if self.at_ident("end") {
                self.bump();
                self.eat(TokenKind::Semi);
            } else if self.at(TokenKind::Ident) && self.peek_kind(1) == Some(TokenKind::Eq) {
                self.parse_top_assignment();
            } else {
                self.skip_until_semi();
            }
        }
    }

    fn parse_declaration(&mut self, keyword: &str) -> Vec<Decl> {
        let start = self.current_start();
        let keyword_is_var = self.at_ident_ci("var");
        self.bump();
        let mut log_transform = false;
        if self.at(TokenKind::LParen) {
            let opt = self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            log_transform = keyword_is_var
                && self.src[opt.start as usize..opt.end as usize]
                    .split(',')
                    .any(|p| p.trim().eq_ignore_ascii_case("log"));
        }
        let kw_range_end = self.current_start();
        let mut decls = Vec::new();
        let mut recorded_missing = false;
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Latex) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if self.at(TokenKind::Comma) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::Ident) {
                if self.at_decl_or_block_keyword() {
                    let next = self.tokens[self.i].span;
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingDeclSemi {
                            keyword: keyword.to_string(),
                            next_is_assign: false,
                            next_span: Some(next),
                        },
                        span: Span {
                            start,
                            end: kw_range_end,
                        },
                    });
                    recorded_missing = true;
                    break;
                }
                if self.peek_kind(1) == Some(TokenKind::Eq) {
                    let next = self.tokens[self.i + 1].span;
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingDeclSemi {
                            keyword: keyword.to_string(),
                            next_is_assign: true,
                            next_span: Some(next),
                        },
                        span: Span {
                            start,
                            end: kw_range_end,
                        },
                    });
                    recorded_missing = true;
                    break;
                }
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                if matches!(name.as_str(), "long_name" | "latex_name" | "long") {
                    continue;
                }
                let id = self.intern.intern(&name);
                decls.push(Decl {
                    name: id,
                    span: tok.span,
                    long_name: None,
                    log_transform,
                });
                continue;
            }
            self.bump();
        }
        if !recorded_missing && !self.at(TokenKind::Semi) {
            self.record_issue(ParseIssue {
                kind: ParseIssueKind::MissingDeclSemi {
                    keyword: keyword.to_string(),
                    next_is_assign: false,
                    next_span: None,
                },
                span: Span {
                    start,
                    end: kw_range_end,
                },
            });
        }
        self.eat(TokenKind::Semi);
        decls
    }

    fn parse_model_block(&mut self) {
        let start = self.bump().span.start;
        let mut linear = false;
        if self.at(TokenKind::LParen) {
            let from = self.i;
            let opt = self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            self.record_deprecated_options_in_range(from, self.i);
            linear = self.src[opt.start as usize..opt.end as usize]
                .to_ascii_lowercase()
                .contains("linear");
        }
        let opener_end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        let opener_span = Span {
            start,
            end: opener_end,
        };
        if linear {
            self.model.is_linear = true;
        }
        let body_i = self.i;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if let Some(eq) = self.parse_equation_statement() {
                self.model.equations.push(eq);
            }
        }
        if self.at_block_end() {
            self.record_missing_final("model", body_i, self.i);
        }
        let end = self.finish_block_named("model", opener_span, body_i);
        self.model.model_block = Some(Span { start, end });
    }

    fn parse_ss_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        while !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if let Some(eq) = self.parse_equation_statement() {
                self.model.steady_state_equations.push(eq);
            }
        }
        if self.at_block_end() {
            self.record_missing_final("steady_state_model", body_i, self.i);
        }
        let end = self.finish_block_named("steady_state_model", opener_span, body_i);
        self.model.ss_block = Some(Span { start, end });
    }

    fn parse_initval_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("initval", opener_span, body_i, body_end_i);
        if self.i > body_end_i {
            self.record_missing_final("initval", body_i, body_end_i);
        }
        let end = self.block_end_after_consume();
        self.model.initval_block = Some(Span { start, end });
        let saved = self.i;
        self.i = body_i;
        while self.i < body_end_i && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            let before = self.i;
            if let Some(a) = self.parse_named_assignment() {
                self.model.initval.push(a);
            }
            if self.i <= before {
                self.bump();
            }
            if self.i > body_end_i {
                self.i = body_end_i;
                break;
            }
        }
        self.i = saved;
    }

    fn parse_endval_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("endval", opener_span, body_i, body_end_i);
        if self.i > body_end_i {
            self.record_missing_final("endval", body_i, body_end_i);
        }
        let end = self.block_end_after_consume();
        self.model.endval_block = Some(Span { start, end });
        for (raw, span) in self.statements_in(body_i, body_end_i) {
            if let Some(a) = self.assignment_from(&raw, span) {
                self.model.endval.push(a);
            }
        }
    }

    fn parse_shocks_block(&mut self, record_stmts: bool) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("shocks", opener_span, body_i, body_end_i);
        let end = self.current_start();
        self.model.shocks_block = Some(Span { start, end });
        self.collect_shock_vars(body_i, body_end_i);
        if record_stmts {
            self.collect_shock_stmts(body_i, body_end_i);
        }
        if self.i > body_end_i {
            self.record_missing_shocks_semis(body_i, body_end_i);
        }
    }

    fn parse_varobs(&mut self) {
        let start = self.current_start();
        self.bump();
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Comma) || self.at(TokenKind::Latex) {
                self.bump();
                continue;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                let id = self.intern.intern(&name);
                self.model.varobs.push(ObservedVar {
                    name: id,
                    span: tok.span,
                });
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        if self.model.varobs_span.is_none() {
            self.model.varobs_span = Some(Span { start, end });
        }
    }

    fn parse_estimated_params_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("estimated_params", opener_span, body_i, body_end_i);
        let end = self.current_start();
        self.model.estimated_params_span = Some(Span { start, end });
        self.collect_estimated_params(body_i, body_end_i, opener_span.end);
    }

    fn parse_observation_trends_block(&mut self) {
        let opener_span = self.bump_plain_opener();
        let start = opener_span.start;
        let body_i = self.i;
        let body_end_i = self.consume_until_end();
        self.record_missing_end_if_unclosed("observation_trends", opener_span, body_i, body_end_i);
        let end = self.current_start();
        self.model.observation_trends_span = Some(Span { start, end });
        self.collect_observation_trends(body_i, body_end_i);
    }

    fn at_policy_command(&self) -> Option<PolicyCommand> {
        if self.at_ident_ci("ramsey_model") {
            Some(PolicyCommand::RamseyModel)
        } else if self.at_ident_ci("ramsey_policy") {
            Some(PolicyCommand::RamseyPolicy)
        } else if self.at_ident_ci("discretionary_policy") {
            Some(PolicyCommand::DiscretionaryPolicy)
        } else if self.at_ident_ci("osr") {
            Some(PolicyCommand::Osr)
        } else {
            None
        }
    }

    fn parse_policy_command(&mut self, command: PolicyCommand) {
        let tok = self.bump();
        self.model.policy_commands.push(command);
        if self.model.policy_command_span.is_none() {
            self.model.policy_command_span = Some(tok.span);
        }
        if command == PolicyCommand::RamseyPolicy && self.model.ramsey_policy_span.is_none() {
            self.model.ramsey_policy_span = Some(tok.span);
        }
        if self.at(TokenKind::LParen) {
            self.parse_policy_options();
        }
        self.eat(TokenKind::Semi);
    }

    fn parse_policy_options(&mut self) {
        self.bump();
        while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) && !self.at(TokenKind::Semi) {
            if self.at_ident_ci("instruments") && self.peek_kind(1) == Some(TokenKind::Eq) {
                self.bump();
                self.bump();
                self.collect_instruments();
            } else if self.at_ident_ci("planner_discount")
                && self.peek_kind(1) == Some(TokenKind::Eq)
            {
                self.bump();
                self.bump();
                let expr = self.parse_expr();
                if self.model.planner_discount.is_none() {
                    if let Some(id) = expr {
                        let known = self.fold_known_params();
                        if let Some(v) = self.fold_expr(id, &known).filter(|v| v.is_finite()) {
                            self.model.planner_discount = Some(v);
                        }
                    }
                }
            } else if self.at(TokenKind::LParen) {
                let from = self.i;
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                self.record_deprecated_options_in_range(from, self.i);
            } else {
                if self.at(TokenKind::Ident) {
                    let tok = self.tokens[self.i].clone();
                    let lex = self.lexeme(&tok).to_string();
                    self.record_deprecated_option_ident(&lex, tok.span);
                }
                self.bump();
            }
        }
        self.eat(TokenKind::RParen);
    }

    fn collect_instruments(&mut self) {
        if self.at(TokenKind::LParen) {
            self.bump();
            while !self.at(TokenKind::Eof) && !self.at(TokenKind::RParen) {
                if self.at(TokenKind::Ident) {
                    self.push_instrument();
                } else {
                    self.bump();
                }
            }
            self.eat(TokenKind::RParen);
            return;
        }
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::Comma)
            && !self.at(TokenKind::RParen)
            && !self.at(TokenKind::Semi)
        {
            if self.at(TokenKind::Ident) {
                self.push_instrument();
            } else {
                self.bump();
            }
        }
    }

    fn push_instrument(&mut self) {
        let tok = self.bump();
        let name = self.lexeme(&tok).to_string();
        let id = self.intern.intern(&name);
        if !self.model.instruments.contains(&id) {
            self.model.instruments.push(id);
        }
    }

    fn parse_planner_objective(&mut self) {
        let start = self.current_start();
        self.bump();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        if self.model.planner_objective_span.is_none() {
            self.model.planner_objective_span = Some(Span { start, end });
        }
    }

    fn parse_osr_params(&mut self) {
        self.bump();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let name = self.lexeme(&tok).to_string();
                let id = self.intern.intern(&name);
                self.model.osr_params.push(id);
            } else {
                self.bump();
            }
        }
        self.eat(TokenKind::Semi);
    }

    fn fold_known_params(&self) -> HashMap<Name, f64> {
        let mut known = HashMap::new();
        for a in &self.model.param_assignments {
            let value = a
                .expr
                .and_then(|id| self.fold_expr(id, &known))
                .filter(|v| v.is_finite());
            match value {
                Some(v) => {
                    known.insert(a.name, v);
                }
                None => {
                    known.remove(&a.name);
                }
            }
        }
        known
    }

    fn fold_expr(&self, id: ExprId, known: &HashMap<Name, f64>) -> Option<f64> {
        match &self.model.exprs.get(id).kind {
            ExprKind::Number => {
                let span = self.model.exprs.get(id).span;
                let raw = self.src.get(span.start as usize..span.end as usize)?;
                raw.parse().ok()
            }
            ExprKind::Ident { name, timing, .. } => {
                if *timing != 0 {
                    return None;
                }
                known.get(name).copied()
            }
            ExprKind::Unary { op, arg } => {
                let v = self.fold_expr(*arg, known)?;
                Some(match op {
                    UnOp::Pos => v,
                    UnOp::Neg => -v,
                })
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.fold_expr(*lhs, known)?;
                let r = self.fold_expr(*rhs, known)?;
                match op {
                    BinOp::Add => Some(l + r),
                    BinOp::Sub => Some(l - r),
                    BinOp::Mul => Some(l * r),
                    BinOp::Div => Some(l / r),
                    BinOp::Pow => Some(l.powf(r)),
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::EqEq | BinOp::Ne => None,
                }
            }
            ExprKind::Call { .. }
            | ExprKind::String
            | ExprKind::Error
            | ExprKind::SteadyState { .. }
            | ExprKind::Expectation { .. } => None,
        }
    }

    fn collect_estimated_params(&mut self, start_i: usize, end_i: usize, body_start: u32) {
        let mut i = start_i;
        let mut entry_start = body_start;
        while i < end_i {
            if self.tokens[i].kind == TokenKind::Semi {
                entry_start = self.tokens[i].span.end;
                i += 1;
                continue;
            }
            let stmt_start = i;
            while i < end_i && self.tokens[i].kind != TokenKind::Semi {
                i += 1;
            }
            let has_semi = i < end_i && self.tokens[i].kind == TokenKind::Semi;
            if !has_semi {
                break;
            }
            let entry_end = self.tokens[i].span.end;
            if let Some(mut entry) = self.parse_estimated_param_entry(stmt_start, i) {
                entry.span = Span {
                    start: entry_start,
                    end: entry_end,
                };
                self.model.estimated_params.push(entry);
            }
            entry_start = entry_end;
            i += 1;
        }
    }

    fn parse_estimated_param_entry(
        &mut self,
        start_i: usize,
        end_i: usize,
    ) -> Option<EstimatedParam> {
        let mut i = start_i;
        while i < end_i && self.tokens[i].kind == TokenKind::Comma {
            i += 1;
        }
        if i >= end_i || self.tokens[i].kind != TokenKind::Ident {
            return None;
        }
        let first = self.tokens[i].text(self.src).to_string();
        i += 1;
        let (kind, name, corr_with) = if first.eq_ignore_ascii_case("stderr") {
            let (name, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            (EstimatedParamKind::Stderr, name, None)
        } else if first.eq_ignore_ascii_case("corr") {
            let (name, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            while i < end_i && self.tokens[i].kind == TokenKind::Comma {
                i += 1;
            }
            let (other, next) = next_ident(&self.tokens, self.src, i, end_i)?;
            i = next;
            (EstimatedParamKind::Corr, name, Some(other))
        } else {
            (EstimatedParamKind::Param, first, None)
        };
        let name = self.intern.intern(&name);
        let corr_with = corr_with.map(|n| self.intern.intern(&n));

        let mut nums = Vec::new();
        loop {
            while i < end_i && self.tokens[i].kind == TokenKind::Comma {
                i += 1;
            }
            if i >= end_i {
                break;
            }
            if self.tokens[i].kind == TokenKind::Ident {
                let ident = self.tokens[i].text(self.src);
                if PRIOR_SHAPES.iter().any(|s| ident.eq_ignore_ascii_case(s)) {
                    break;
                }
            }
            match parse_ep_float(&self.tokens, self.src, i, end_i) {
                Some((val, next)) => {
                    nums.push(val);
                    i = next;
                }
                None => break,
            }
        }

        let init = nums.first().copied();
        let (lower, upper) = if nums.len() >= 3 {
            (Some(nums[1]), Some(nums[2]))
        } else {
            (None, None)
        };
        Some(EstimatedParam {
            name,
            kind,
            corr_with,
            init,
            lower,
            upper,
            span: Span { start: 0, end: 0 },
        })
    }

    fn collect_observation_trends(&mut self, start_i: usize, end_i: usize) {
        let mut i = start_i;
        while i < end_i {
            let stmt_start = i;
            while i < end_i && self.tokens[i].kind != TokenKind::Semi {
                i += 1;
            }
            let mut j = stmt_start;
            while j < i {
                if self.tokens[j].kind == TokenKind::Ident {
                    let next = self.tokens.get(j + 1).filter(|_| j + 1 < i);
                    if next.is_some_and(|t| t.kind == TokenKind::LParen || t.kind == TokenKind::Eq)
                    {
                        let name = self.tokens[j].text(self.src).to_string();
                        let span = self.tokens[j].span;
                        let id = self.intern.intern(&name);
                        if !self.model.observation_trends.iter().any(|(n, _)| *n == id) {
                            self.model.observation_trends.push((id, span));
                        }
                        break;
                    }
                }
                j += 1;
            }
            if i < end_i && self.tokens[i].kind == TokenKind::Semi {
                i += 1;
            }
        }
    }

    fn bump_plain_opener(&mut self) -> Span {
        let start = self.bump().span.start;
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        }
        let end = if self.at(TokenKind::Semi) {
            self.bump().span.end
        } else {
            self.current_start()
        };
        Span { start, end }
    }

    fn collect_shock_vars(&mut self, start_i: usize, end_i: usize) {
        let mut i = start_i;
        while i < end_i {
            let is_var_or_corr = self.tokens[i].kind == TokenKind::Ident && {
                let kw = self.tokens[i].text(self.src);
                kw.eq_ignore_ascii_case("var") || kw.eq_ignore_ascii_case("corr")
            };
            if is_var_or_corr {
                i += 1;
                while i < end_i {
                    let kind = self.tokens[i].kind;
                    if kind == TokenKind::Eq || kind == TokenKind::Semi {
                        break;
                    }
                    if kind == TokenKind::Ident {
                        let name = self.tokens[i].text(self.src).to_string();
                        if !name.eq_ignore_ascii_case("stderr") {
                            let id = self.intern.intern(&name);
                            if !self.model.shocks_vars.contains(&id) {
                                self.model.shocks_vars.push(id);
                            }
                        }
                    }
                    i += 1;
                }
                continue;
            }
            i += 1;
        }
    }

    fn collect_shock_stmts(&mut self, start_i: usize, end_i: usize) {
        let saved = self.i;
        self.i = start_i;
        while self.i < end_i {
            if self.at(TokenKind::Semi) {
                self.bump();
                continue;
            }
            if self.at_ident_ci("var") {
                self.parse_shock_var_stmt(end_i);
            } else if self.at_ident_ci("corr") {
                self.parse_shock_corr_stmt(end_i);
            } else {
                while self.i < end_i && !self.at(TokenKind::Semi) {
                    self.bump();
                }
                if self.at(TokenKind::Semi) {
                    self.bump();
                }
            }
        }
        self.i = saved;
    }

    fn parse_shock_var_stmt(&mut self, end_i: usize) {
        let start = self.current_start();
        self.bump();
        let mut names = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eq) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                if !lex.eq_ignore_ascii_case("stderr") {
                    names.push(self.intern.intern(&lex));
                }
            } else {
                self.bump();
            }
        }
        let rhs = if self.at(TokenKind::Eq) {
            self.bump();
            self.parse_folded_rhs()
        } else {
            None
        };
        let end = self.finish_shock_stmt(end_i);
        if names.is_empty() {
            return;
        }
        let kind = if names.len() == 1 {
            ShockKind::Var(names[0])
        } else {
            ShockKind::Cov(names)
        };
        self.model.shock_stmts.push(ShockStmt {
            kind,
            rhs,
            span: Span { start, end },
        });
    }

    fn parse_shock_corr_stmt(&mut self, end_i: usize) {
        let start = self.current_start();
        self.bump();
        let mut names = Vec::new();
        while self.i < end_i && !self.at(TokenKind::Eq) && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                let lex = self.lexeme(&tok).to_string();
                names.push(self.intern.intern(&lex));
            } else {
                self.bump();
            }
        }
        let has_eq = self.at(TokenKind::Eq);
        let rhs = if has_eq {
            self.bump();
            self.parse_folded_rhs()
        } else {
            None
        };
        let end = self.finish_shock_stmt(end_i);
        if names.len() < 2 || !has_eq {
            return;
        }
        self.model.shock_stmts.push(ShockStmt {
            kind: ShockKind::Corr {
                a: names[0],
                b: names[1],
            },
            rhs,
            span: Span { start, end },
        });
    }

    fn parse_folded_rhs(&mut self) -> Option<f64> {
        let id = self.parse_expr()?;
        let known = self.fold_known_params();
        self.fold_expr(id, &known).filter(|v| v.is_finite())
    }

    fn finish_shock_stmt(&mut self, end_i: usize) -> u32 {
        while self.i < end_i && !self.at(TokenKind::Semi) {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        if self.at(TokenKind::Semi) {
            self.bump();
        }
        end
    }

    fn skip_block(&mut self) {
        self.bump();
        if self.at(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
        }
        self.eat(TokenKind::Semi);
        self.consume_until_end();
    }

    fn parse_top_assignment(&mut self) {
        let Some(assignment) = self.parse_named_assignment() else {
            return;
        };
        if self
            .model
            .parameters
            .iter()
            .any(|d| d.name == assignment.name)
        {
            self.model.param_assignments.push(assignment);
        } else {
            self.model.helper_assignments.push(assignment);
        }
    }

    fn parse_named_assignment(&mut self) -> Option<Assignment> {
        if !self.looks_like_assignment_start() {
            self.skip_to_stmt_end();
            self.eat(TokenKind::Semi);
            return None;
        }
        let tok = self.bump();
        let name = self.lexeme(&tok).to_string();
        self.eat(TokenKind::Eq);
        let expr_i = self.i;
        let expr = self.parse_expr();
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if self.looks_like_assignment_start()
                || self.at_follower_keyword()
                || self.at_block_stop()
            {
                break;
            }
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                continue;
            }
            self.bump();
        }
        let expr_end_i = self.i;
        let stmt_end = if self.at(TokenKind::Semi) {
            self.tokens[self.i].span.end
        } else {
            self.current_start()
        };
        self.eat(TokenKind::Semi);
        let expression = join_lexemes(self.src, &self.tokens[expr_i..expr_end_i]);
        let id = self.intern.intern(&name);
        Some(Assignment {
            name: id,
            expression,
            span: Span {
                start: tok.span.start,
                end: stmt_end,
            },
            expr,
        })
    }

    fn consume_until_end(&mut self) -> usize {
        loop {
            if self.at(TokenKind::Eof) || self.at_block_opener() {
                return self.i;
            }
            if self.at_ident("end") && self.peek_kind(1) == Some(TokenKind::Semi) {
                let idx = self.i;
                self.bump();
                self.bump();
                return idx;
            }
            self.bump();
        }
    }

    fn statements_in(&self, start_i: usize, end_i: usize) -> Vec<(String, Span)> {
        let mut out = Vec::new();
        let mut stmt_start = start_i;
        for i in start_i..end_i {
            let tok = &self.tokens[i];
            if tok.kind == TokenKind::Eof {
                continue;
            }
            if tok.kind == TokenKind::Semi {
                if stmt_start < i {
                    let raw = join_lexemes(self.src, &self.tokens[stmt_start..i]);
                    out.push((
                        raw,
                        Span {
                            start: self.tokens[stmt_start].span.start,
                            end: tok.span.end,
                        },
                    ));
                }
                stmt_start = i + 1;
            }
        }
        if stmt_start < end_i {
            let last = &self.tokens[end_i - 1];
            out.push((
                join_lexemes(self.src, &self.tokens[stmt_start..end_i]),
                Span {
                    start: self.tokens[stmt_start].span.start,
                    end: last.span.end,
                },
            ));
        }
        out
    }

    fn block_end_after_consume(&self) -> u32 {
        if self.i > 0 {
            self.tokens[self.i - 1].span.end
        } else {
            self.current_start()
        }
    }

    fn assignment_from(&mut self, raw: &str, span: Span) -> Option<Assignment> {
        let text = collapse_ws(raw);
        let (lhs, rhs) = split_eq(&text)?;
        if !is_ident(lhs) {
            return None;
        }
        let name = self.intern.intern(lhs);
        Some(Assignment {
            name,
            expression: rhs.to_string(),
            span,
            expr: None,
        })
    }

    fn parse_equation_statement(&mut self) -> Option<Equation> {
        if self.at(TokenKind::Semi) {
            self.bump();
            return None;
        }
        if self.at(TokenKind::Eof) || self.at_block_stop() {
            return None;
        }

        let stmt_i = self.i;
        let stmt_start = self.current_start();
        let mut static_tag = false;
        let mut dynamic_tag = false;
        let mut tags = Vec::new();
        while self.at(TokenKind::LBrack) {
            let (s, d, t) = self.skip_tag();
            static_tag |= s;
            dynamic_tag |= d;
            tags.extend(t);
        }
        let is_local = self.at(TokenKind::Hash);
        if is_local {
            self.bump();
        }

        let (lhs_expr, lhs_ok) = self.parse_expr_side(ExprStop::EqOrSemi);
        let saw_eq = self.at(TokenKind::Eq);
        let (rhs_expr, rhs_ok) = if saw_eq {
            self.bump();
            self.parse_expr_side(ExprStop::Semi)
        } else {
            (None, true)
        };

        if !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) && !self.at_block_stop() {
            self.skip_to_stmt_end();
        }
        let stmt_end_i = self.i;
        let stmt_end = self.current_start();
        self.eat(TokenKind::Semi);

        let raw = join_lexemes(self.src, &self.tokens[stmt_i..stmt_end_i]);
        let mut eq = equation_from_statement(
            &raw,
            Span {
                start: stmt_start,
                end: stmt_end,
            },
        )?;
        eq.is_local = is_local;
        eq.model_local = is_local;
        eq.static_tag = static_tag;
        eq.dynamic_tag = dynamic_tag;
        eq.tags = tags;
        eq.lhs_expr = Some(if lhs_ok {
            lhs_expr.unwrap_or_else(|| self.alloc_error(eq.span))
        } else {
            self.alloc_error(eq.span)
        });
        if saw_eq {
            eq.rhs_expr = Some(if rhs_ok {
                rhs_expr.unwrap_or_else(|| self.alloc_error(eq.span))
            } else {
                self.alloc_error(eq.span)
            });
        }
        Some(eq)
    }

    fn parse_expr_side(&mut self, stop: ExprStop) -> (Option<ExprId>, bool) {
        let id = self.parse_expr();
        let clean = match stop {
            ExprStop::EqOrSemi => {
                self.at(TokenKind::Eq)
                    || self.at(TokenKind::Semi)
                    || self.at(TokenKind::Eof)
                    || self.at_block_stop()
            }
            ExprStop::Semi => {
                self.at(TokenKind::Semi) || self.at(TokenKind::Eof) || self.at_block_stop()
            }
        };
        if !clean {
            self.skip_to_stmt_end();
            return (id, false);
        }
        (id, id.is_some() || self.at_side_empty(stop))
    }

    fn at_side_empty(&self, stop: ExprStop) -> bool {
        match stop {
            ExprStop::EqOrSemi => {
                self.at(TokenKind::Eq) || self.at(TokenKind::Semi) || self.at_block_stop()
            }
            ExprStop::Semi => self.at(TokenKind::Semi) || self.at_block_stop(),
        }
    }

    fn skip_tag(&mut self) -> (bool, bool, Vec<String>) {
        self.bump();
        let mut static_tag = false;
        let mut dynamic_tag = false;
        let mut tags = Vec::new();
        while !self.at(TokenKind::Eof)
            && !self.at(TokenKind::RBrack)
            && !self.at(TokenKind::Semi)
            && !self.at_block_stop()
        {
            if self.at(TokenKind::Ident) {
                let tok = self.bump();
                if self.lexeme(&tok).eq_ignore_ascii_case("static") {
                    static_tag = true;
                    tags.push("static".to_string());
                } else if self.lexeme(&tok).eq_ignore_ascii_case("dynamic") {
                    dynamic_tag = true;
                    tags.push("dynamic".to_string());
                }
            } else {
                self.bump();
            }
        }
        self.eat(TokenKind::RBrack);
        (static_tag, dynamic_tag, tags)
    }

    fn skip_to_stmt_end(&mut self) {
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) && !self.at_block_stop() {
            if self.at(TokenKind::LParen) {
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
            } else if self.at(TokenKind::LBrack) {
                self.skip_balanced(TokenKind::LBrack, TokenKind::RBrack);
            } else {
                self.bump();
            }
        }
    }

    fn finish_block_named(&mut self, keyword: &str, opener_span: Span, body_i: usize) -> u32 {
        if self.at_ident("end") {
            self.bump();
            if self.at(TokenKind::Semi) {
                return self.bump().span.end;
            }
        }
        self.record_missing_end(keyword, opener_span, body_i);
        self.current_start()
    }

    fn at_block_end(&self) -> bool {
        self.at_ident("end") && self.peek_kind(1) == Some(TokenKind::Semi)
    }

    fn at_block_stop(&self) -> bool {
        self.at_block_end() || self.at_block_opener()
    }

    fn at_block_opener(&self) -> bool {
        if !BLOCK_OPENERS.iter().any(|kw| self.at_ident_ci(kw)) {
            return false;
        }
        let mut k = 1;
        if self.peek_kind(k) == Some(TokenKind::LParen) {
            let mut depth = 1;
            k += 1;
            while let Some(kind) = self.peek_kind(k) {
                match kind {
                    TokenKind::LParen => depth += 1,
                    TokenKind::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            k += 1;
                            break;
                        }
                    }
                    TokenKind::Eof => return false,
                    _ => {}
                }
                k += 1;
            }
        }
        self.peek_kind(k) == Some(TokenKind::Semi)
    }

    fn at_decl_or_block_keyword(&self) -> bool {
        const KS: &[&str] = &[
            "varexo_det",
            "var",
            "varexo",
            "parameters",
            "predetermined_variables",
            "model",
            "initval",
            "endval",
            "shocks",
            "steady_state_model",
        ];
        KS.iter().any(|kw| self.at_ident_ci(kw))
    }

    fn looks_like_assignment_start(&self) -> bool {
        self.at(TokenKind::Ident) && self.peek_kind(1) == Some(TokenKind::Eq)
    }

    fn at_follower_keyword(&self) -> bool {
        const KS: &[&str] = &[
            "model",
            "model_remove",
            "model_replace",
            "var",
            "var_remove",
            "varexo",
            "varexo_det",
            "parameters",
            "predetermined_variables",
            "initval",
            "endval",
            "shocks",
            "steady_state_model",
            "steady",
            "check",
            "resid",
            "stoch_simul",
            "simul",
            "estimation",
            "osr",
            "calib_smoother",
            "forecast",
            "identification",
            "dynasave",
            "dynatype",
            "model_diagnostics",
            "model_info",
            "perfect_foresight_setup",
            "perfect_foresight_solver",
        ];
        KS.iter().any(|kw| self.at_ident_ci(kw))
    }

    fn record_issue(&mut self, issue: ParseIssue) {
        self.model.parse_issues.push(issue);
    }

    fn record_missing_end(&mut self, keyword: &str, opener_span: Span, body_i: usize) {
        let insert_offset = if self.at_block_opener() {
            self.current_start()
        } else {
            self.src.len() as u32
        };
        let next_block_label = if self.at_block_opener() {
            let raw = self.lexeme(&self.tokens[self.i]);
            Some(
                BLOCK_OPENERS
                    .iter()
                    .copied()
                    .find(|k| raw.eq_ignore_ascii_case(k))
                    .unwrap_or(raw)
                    .to_string(),
            )
        } else {
            None
        };
        let last_stmt_semi = self.last_semi_in_body(body_i);
        self.record_issue(ParseIssue {
            kind: ParseIssueKind::MissingEnd {
                keyword: keyword.to_string(),
                last_stmt_semi,
                next_block_label,
                insert_offset,
            },
            span: opener_span,
        });
    }

    fn record_missing_end_if_unclosed(
        &mut self,
        keyword: &str,
        opener_span: Span,
        body_i: usize,
        body_end_i: usize,
    ) {
        let closed = self.i > body_end_i;
        if !closed {
            self.record_missing_end(keyword, opener_span, body_i);
        }
    }

    fn record_missing_final(&mut self, keyword: &str, body_i: usize, body_end_i: usize) {
        if body_i >= body_end_i {
            return;
        }
        let last = (body_i..body_end_i)
            .rev()
            .find(|&i| self.tokens[i].kind != TokenKind::Eof);
        let Some(last) = last else {
            return;
        };
        if self.tokens[last].kind == TokenKind::Semi {
            return;
        }
        let body_start = self.tokens[body_i].span.start;
        let body_end = self.tokens[body_end_i].span.start;
        if body_start >= body_end {
            return;
        }
        let body = &self.src[body_start as usize..body_end as usize];
        let body_code = body.trim_end();
        if body_code.is_empty() || body_code.ends_with(';') {
            return;
        }
        let body_code_end = body_start + body_code.len() as u32;
        let mut stmt_rel = body_code.rfind(';').map(|p| p + 1).unwrap_or(0);
        while stmt_rel < body_code.len() && body_code.as_bytes()[stmt_rel].is_ascii_whitespace() {
            stmt_rel += 1;
        }
        let stmt_start = body_start + stmt_rel as u32;
        self.record_issue(ParseIssue {
            kind: ParseIssueKind::MissingFinalSemi {
                keyword: keyword.to_string(),
                body_code_end,
            },
            span: Span {
                start: stmt_start,
                end: body_code_end,
            },
        });
    }

    fn last_semi_in_body(&self, body_i: usize) -> Option<u32> {
        (body_i..self.i)
            .rev()
            .find(|&i| self.tokens[i].kind == TokenKind::Semi)
            .map(|i| self.tokens[i].span.start)
    }

    fn record_keyword_typos(&mut self) {
        let mut check = Vec::new();
        if self.model.equations.is_empty() {
            check.push("model");
        }
        if self.model.endogenous.is_empty() {
            check.push("var");
        }
        if self.model.parameters.is_empty() {
            check.push("parameters");
        }
        if self.model.exogenous.is_empty() {
            check.push("varexo");
        }
        if self.model.shocks_block.is_none() && !self.model.exogenous.is_empty() {
            check.push("shocks");
        }
        if check.is_empty() {
            return;
        }
        let blocks = complete_block_ranges(&self.tokens, self.src);
        for tok in &self.tokens {
            if tok.kind != TokenKind::Ident {
                continue;
            }
            if inside_span(tok.span.start, &blocks) {
                continue;
            }
            let word = tok.text(self.src);
            if word.len() < 3 || word.len() > 12 {
                continue;
            }
            if !at_line_start_ident(self.src, tok.span) {
                continue;
            }
            let lower = word.to_ascii_lowercase();
            let Some((_, correct)) = KEYWORD_TYPO_MAP.iter().find(|(t, _)| *t == lower) else {
                continue;
            };
            if !check.iter().any(|c| c == correct) {
                continue;
            }
            self.model.parse_issues.push(ParseIssue {
                kind: ParseIssueKind::KeywordTypo {
                    found: word.to_string(),
                    correct: (*correct).to_string(),
                },
                span: tok.span,
            });
        }
    }

    fn record_missing_assign_semis(&mut self) {
        let src = self.src;
        let blocks = complete_block_ranges(&self.tokens, src);
        let trailing = trailing_code_line(&self.tokens, src);
        let lines: Vec<&str> = src.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            let i = i as u32;
            if trailing.is_some_and(|t| i > t) {
                break;
            }
            let line_start = if i == 0 {
                0
            } else {
                lines[..i as usize]
                    .iter()
                    .map(|l| l.len() + 1)
                    .sum::<usize>() as u32
            };
            if inside_span(line_start, &blocks) {
                continue;
            }
            let trimmed_start = line.len() - line.trim_start().len();
            let rest = &line[trimmed_start..];
            let Some(name) = leading_ident(rest) else {
                continue;
            };
            let after_name = &rest[name.len()..];
            let after_ws = after_name.trim_start();
            if !after_ws.starts_with('=') || after_ws.starts_with("==") {
                continue;
            }
            if ASSIGN_BLOCK_LIKE
                .iter()
                .any(|k| name.eq_ignore_ascii_case(k))
            {
                continue;
            }
            let rhs_raw = after_ws[1..].trim_start();
            let rhs_code = strip_line_comment(rhs_raw).trim_end();
            if rhs_code.ends_with(';') {
                continue;
            }
            if looks_like_matlab(rhs_raw) {
                continue;
            }
            let mut j = i + 1;
            while (j as usize) < lines.len() {
                let next = lines[j as usize].trim();
                if next.is_empty() {
                    j += 1;
                    continue;
                }
                let next_code = strip_line_comment(next).trim();
                let follower = leading_ident(next_code).is_some_and(|id| {
                    let after = next_code[id.len()..].trim_start();
                    after.starts_with('=') && !after.starts_with("==")
                }) || leading_ident(next_code)
                    .is_some_and(|id| ASSIGN_FOLLOWERS.iter().any(|f| id.eq_ignore_ascii_case(f)));
                if follower {
                    self.model.parse_issues.push(ParseIssue {
                        kind: ParseIssueKind::MissingAssignSemi {
                            name: name.to_string(),
                        },
                        span: Span {
                            start: line_start,
                            end: line_start + line.len() as u32,
                        },
                    });
                }
                break;
            }
        }
    }

    fn record_missing_shocks_semis(&mut self, from: usize, to: usize) {
        let src = self.src;
        let mut i = from;
        while i < to {
            let tok = &self.tokens[i];
            if tok.kind == TokenKind::Ident && tok.text(src).eq_ignore_ascii_case("var") {
                let var_tok = tok.clone();
                let Some(name_tok) = self
                    .tokens
                    .get(i + 1)
                    .filter(|t| t.kind == TokenKind::Ident)
                    .cloned()
                else {
                    i += 1;
                    continue;
                };
                let after_i = i + 2;
                if after_i >= to {
                    i += 1;
                    continue;
                }
                let next = &self.tokens[after_i];
                if next.kind == TokenKind::Eq || next.kind == TokenKind::Comma {
                    i += 1;
                    continue;
                }
                if next.kind == TokenKind::Semi {
                    i += 1;
                    continue;
                }
                if next.kind == TokenKind::Ident
                    && ["var", "stderr", "corr"]
                        .iter()
                        .any(|k| next.text(src).eq_ignore_ascii_case(k))
                {
                    let keyword = next.text(src).to_string();
                    let next_start = next.span.start;
                    let mut fix_start = next_start;
                    while fix_start > var_tok.span.start {
                        let b = src.as_bytes()[(fix_start - 1) as usize];
                        if b == b' ' || b == b'\t' {
                            fix_start -= 1;
                        } else {
                            break;
                        }
                    }
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingShocksSemi {
                            family: ShocksSemiFamily::BeforeKeyword,
                            label: keyword,
                            fix_start,
                            fix_end: next_start,
                        },
                        span: Span {
                            start: var_tok.span.start,
                            end: next_start,
                        },
                    });
                } else {
                    let name = name_tok.text(src).to_string();
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingShocksSemi {
                            family: ShocksSemiFamily::AfterVar,
                            label: name,
                            fix_start: name_tok.span.end,
                            fix_end: name_tok.span.end,
                        },
                        span: Span {
                            start: var_tok.span.start,
                            end: name_tok.span.end,
                        },
                    });
                }
                i += 1;
                continue;
            }
            if tok.kind == TokenKind::Ident
                && ["stderr", "corr"]
                    .iter()
                    .any(|k| tok.text(src).eq_ignore_ascii_case(k))
            {
                let stmt = tok.text(src).to_string();
                let stmt_start = tok.span.start;
                let mut k = i + 1;
                let mut semi_same_line = false;
                while k < to {
                    if self.tokens[k].kind == TokenKind::Semi {
                        let nl = src[tok.span.end as usize..self.tokens[k].span.start as usize]
                            .contains('\n');
                        if !nl {
                            semi_same_line = true;
                        }
                        break;
                    }
                    if self.tokens[k].kind == TokenKind::Ident
                        && ["var", "stderr", "corr"]
                            .iter()
                            .any(|n| self.tokens[k].text(src).eq_ignore_ascii_case(n))
                    {
                        break;
                    }
                    k += 1;
                }
                if !semi_same_line {
                    let line = line_of(src, stmt_start).1;
                    let code = strip_line_comment(line);
                    let line_start = src[..stmt_start as usize]
                        .rfind('\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let abs_end = line_start as u32 + code.len() as u32;
                    self.record_issue(ParseIssue {
                        kind: ParseIssueKind::MissingShocksSemi {
                            family: ShocksSemiFamily::EndOfStmt,
                            label: stmt,
                            fix_start: abs_end,
                            fix_end: abs_end,
                        },
                        span: Span {
                            start: stmt_start,
                            end: abs_end,
                        },
                    });
                }
            }
            i += 1;
        }
    }

    fn parse_expr(&mut self) -> Option<ExprId> {
        self.parse_bp(0)
    }

    fn parse_bp(&mut self, min_bp: u8) -> Option<ExprId> {
        let mut lhs = self.parse_prefix()?;
        while let Some((l_bp, r_bp, op)) = self.infix_op() {
            if l_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = match self.parse_bp(r_bp) {
                Some(id) => id,
                None => self.alloc_error(self.expr_span(lhs)),
            };
            let span = Span {
                start: self.expr_span(lhs).start,
                end: self.expr_span(rhs).end,
            };
            lhs = self.alloc(ExprKind::Binary { op, lhs, rhs }, span);
        }
        Some(lhs)
    }

    fn parse_prefix(&mut self) -> Option<ExprId> {
        if self.at_expr_stop() {
            return None;
        }
        if self.at(TokenKind::Plus) || self.at(TokenKind::Minus) {
            let op = if self.at(TokenKind::Plus) {
                UnOp::Pos
            } else {
                UnOp::Neg
            };
            let tok = self.bump();
            let arg = match self.parse_bp(UNARY_BP) {
                Some(id) => id,
                None => self.alloc_error(tok.span),
            };
            let span = Span {
                start: tok.span.start,
                end: self.expr_span(arg).end,
            };
            return Some(self.alloc(ExprKind::Unary { op, arg }, span));
        }
        if self.at(TokenKind::LParen) {
            self.bump();
            let inner = self.parse_bp(0);
            self.eat(TokenKind::RParen);
            return inner;
        }
        if self.at(TokenKind::Number) {
            let tok = self.bump();
            return Some(self.alloc(ExprKind::Number, tok.span));
        }
        if self.at(TokenKind::String) {
            let tok = self.bump();
            return Some(self.alloc(ExprKind::String, tok.span));
        }
        if self.at(TokenKind::Ident) {
            return Some(self.parse_ident_expr());
        }
        None
    }

    fn parse_ident_expr(&mut self) -> ExprId {
        let tok = self.bump();
        let lexeme = self.lexeme(&tok).to_string();
        let name = self.intern.intern(&lexeme);
        if !self.at(TokenKind::LParen) {
            return self.alloc(
                ExprKind::Ident {
                    name,
                    timing: 0,
                    ident_span: tok.span,
                    timing_span: None,
                },
                tok.span,
            );
        }
        if lexeme.eq_ignore_ascii_case("expectation") && self.looks_like_expectation() {
            return self.parse_expectation(tok);
        }
        if lexeme.eq_ignore_ascii_case("steady_state") {
            return self.parse_steady_state(tok);
        }
        if self.looks_like_timing() && !is_builtin_function(&lexeme) {
            return self.parse_timing(name, tok);
        }
        self.parse_call(name, tok)
    }

    fn parse_expectation(&mut self, kw: Token) -> ExprId {
        let (shift, _) = self.parse_signed_int_in_parens();
        self.eat(TokenKind::LParen);
        let arg = match self.parse_expr() {
            Some(id) => id,
            None => self.alloc_error(kw.span),
        };
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            self.expr_span(arg).end
        };
        self.alloc(
            ExprKind::Expectation { shift, arg },
            Span {
                start: kw.span.start,
                end,
            },
        )
    }

    fn parse_steady_state(&mut self, kw: Token) -> ExprId {
        self.eat(TokenKind::LParen);
        let arg = match self.parse_expr() {
            Some(id) => id,
            None => self.alloc_error(kw.span),
        };
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            self.expr_span(arg).end
        };
        self.alloc(
            ExprKind::SteadyState { arg },
            Span {
                start: kw.span.start,
                end,
            },
        )
    }

    fn parse_timing(&mut self, name: Name, ident: Token) -> ExprId {
        let (timing, timing_span) = self.parse_signed_int_in_parens();
        self.alloc(
            ExprKind::Ident {
                name,
                timing,
                ident_span: ident.span,
                timing_span: Some(timing_span),
            },
            Span {
                start: ident.span.start,
                end: timing_span.end,
            },
        )
    }

    fn parse_call(&mut self, callee: Name, kw: Token) -> ExprId {
        self.eat(TokenKind::LParen);
        let mut args = Vec::new();
        if !self.at(TokenKind::RParen) && !self.at_expr_stop() {
            loop {
                if let Some(id) = self.parse_expr() {
                    args.push(id);
                }
                if self.at(TokenKind::Comma) {
                    self.bump();
                    continue;
                }
                break;
            }
        }
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            args.last()
                .map(|id| self.expr_span(*id).end)
                .unwrap_or(kw.span.end)
        };
        self.alloc(
            ExprKind::Call { callee, args },
            Span {
                start: kw.span.start,
                end,
            },
        )
    }

    fn parse_signed_int_in_parens(&mut self) -> (i32, Span) {
        let start = self.bump().span.start;
        let mut sign = 1i32;
        if self.at(TokenKind::Plus) {
            self.bump();
        } else if self.at(TokenKind::Minus) {
            self.bump();
            sign = -1;
        }
        let num = self.bump();
        let mag: i32 = self.lexeme(&num).parse().unwrap_or(0);
        let end = if self.at(TokenKind::RParen) {
            self.bump().span.end
        } else {
            num.span.end
        };
        (sign * mag, Span { start, end })
    }

    fn looks_like_timing(&self) -> bool {
        if !self.at(TokenKind::LParen) {
            return false;
        }
        let mut k = 1;
        if matches!(self.peek_kind(k), Some(TokenKind::Plus | TokenKind::Minus)) {
            k += 1;
        }
        match (self.peek_tok(k), self.peek_kind(k + 1)) {
            (Some(num), Some(TokenKind::RParen)) if num.kind == TokenKind::Number => {
                is_integer_lexeme(self.lexeme(num))
            }
            _ => false,
        }
    }

    fn looks_like_expectation(&self) -> bool {
        if !self.at(TokenKind::LParen) {
            return false;
        }
        let mut k = 1;
        if matches!(self.peek_kind(k), Some(TokenKind::Plus | TokenKind::Minus)) {
            k += 1;
        }
        match (
            self.peek_tok(k),
            self.peek_kind(k + 1),
            self.peek_kind(k + 2),
        ) {
            (Some(num), Some(TokenKind::RParen), Some(TokenKind::LParen))
                if num.kind == TokenKind::Number =>
            {
                is_integer_lexeme(self.lexeme(num))
            }
            _ => false,
        }
    }

    fn infix_op(&self) -> Option<(u8, u8, BinOp)> {
        match self.tokens.get(self.i)?.kind {
            TokenKind::Plus => Some((3, 4, BinOp::Add)),
            TokenKind::Minus => Some((3, 4, BinOp::Sub)),
            TokenKind::Star => Some((5, 6, BinOp::Mul)),
            TokenKind::Slash => Some((5, 6, BinOp::Div)),
            TokenKind::Caret => Some((9, 8, BinOp::Pow)),
            TokenKind::Lt => Some((1, 2, BinOp::Lt)),
            TokenKind::Gt => Some((1, 2, BinOp::Gt)),
            TokenKind::Le => Some((1, 2, BinOp::Le)),
            TokenKind::Ge => Some((1, 2, BinOp::Ge)),
            TokenKind::EqEq => Some((1, 2, BinOp::EqEq)),
            TokenKind::Ne => Some((1, 2, BinOp::Ne)),
            _ => None,
        }
    }

    fn at_expr_stop(&self) -> bool {
        matches!(
            self.tokens.get(self.i).map(|t| t.kind),
            Some(
                TokenKind::Eof
                    | TokenKind::Semi
                    | TokenKind::Eq
                    | TokenKind::Comma
                    | TokenKind::RParen
                    | TokenKind::RBrack
            )
        ) || self.at_block_stop()
    }

    fn alloc(&mut self, kind: ExprKind, span: Span) -> ExprId {
        self.model.exprs.alloc(kind, span)
    }

    fn alloc_error(&mut self, span: Span) -> ExprId {
        self.alloc(ExprKind::Error, span)
    }

    fn expr_span(&self, id: ExprId) -> Span {
        self.model.exprs.get(id).span
    }

    fn peek_tok(&self, ahead: usize) -> Option<&Token> {
        self.tokens.get(self.i + ahead)
    }

    fn skip_until_semi(&mut self) {
        let mut saw_ident = false;
        while !self.at(TokenKind::Semi) && !self.at(TokenKind::Eof) {
            if !saw_ident && self.at(TokenKind::Ident) {
                saw_ident = true;
                let tok = self.tokens[self.i].clone();
                if self.lexeme(&tok).eq_ignore_ascii_case("simul") {
                    self.model.simul_spans.push(tok.span);
                }
            }
            if self.at(TokenKind::LParen) {
                let from = self.i;
                self.skip_balanced(TokenKind::LParen, TokenKind::RParen);
                self.record_deprecated_options_in_range(from, self.i);
                continue;
            }
            self.bump();
        }
        self.eat(TokenKind::Semi);
    }

    fn record_deprecated_option_ident(&mut self, lex: &str, span: Span) {
        let option = if lex.eq_ignore_ascii_case("aim_solver") {
            DeprecatedOption::AimSolver
        } else if lex.eq_ignore_ascii_case("bytecode") {
            DeprecatedOption::Bytecode
        } else {
            return;
        };
        self.model.deprecated_option_spans.push((option, span));
    }

    fn record_deprecated_options_in_range(&mut self, from: usize, to: usize) {
        let hits: Vec<(String, Span)> = self.tokens[from..to.min(self.tokens.len())]
            .iter()
            .filter(|t| t.kind == TokenKind::Ident)
            .map(|t| (t.text(self.src).to_string(), t.span))
            .collect();
        for (lex, span) in hits {
            self.record_deprecated_option_ident(&lex, span);
        }
    }

    fn skip_balanced(&mut self, open: TokenKind, close: TokenKind) -> Span {
        let start = self.bump().span.start;
        let mut depth = 1;
        while !self.at(TokenKind::Eof) {
            if self.at(open) {
                depth += 1;
                self.bump();
            } else if self.at(close) {
                depth -= 1;
                let tok = self.bump();
                if depth == 0 {
                    return Span {
                        start,
                        end: tok.span.end,
                    };
                }
            } else {
                self.bump();
            }
        }
        Span {
            start,
            end: self.current_start(),
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.tokens.get(self.i).is_some_and(|t| t.kind == kind)
    }

    fn at_skipped_block(&self) -> bool {
        const BLOCKS: &[&str] = &[
            "matched_irfs",
            "verbatim",
            "model_replace",
            "heteroskedastic_shocks",
            "shock_paths",
            "conditional_forecast_paths",
        ];
        BLOCKS.iter().any(|kw| self.at_ident_ci(kw))
    }

    fn at_ident(&self, name: &str) -> bool {
        self.at(TokenKind::Ident) && self.lexeme(&self.tokens[self.i]).eq_ignore_ascii_case(name)
    }

    fn at_ident_ci(&self, name: &str) -> bool {
        self.at_ident(name)
    }

    fn peek_kind(&self, ahead: usize) -> Option<TokenKind> {
        self.tokens.get(self.i + ahead).map(|t| t.kind)
    }

    fn current_start(&self) -> u32 {
        self.tokens
            .get(self.i)
            .map(|t| t.span.start)
            .unwrap_or(self.src.len() as u32)
    }

    fn bump(&mut self) -> Token {
        let tok = self.tokens[self.i].clone();
        if tok.kind != TokenKind::Eof {
            self.i += 1;
        }
        tok
    }

    fn eat(&mut self, kind: TokenKind) {
        if self.at(kind) {
            self.bump();
        }
    }

    fn lexeme<'a>(&'a self, tok: &'a Token) -> &'a str {
        tok.text(self.src)
    }
}

fn skip_balanced_tokens(
    tokens: &[Token],
    mut i: usize,
    open: TokenKind,
    close: TokenKind,
) -> usize {
    if i >= tokens.len() || tokens[i].kind != open {
        return i;
    }
    let mut depth = 1;
    i += 1;
    while i < tokens.len() && depth > 0 {
        if tokens[i].kind == open {
            depth += 1;
        } else if tokens[i].kind == close {
            depth -= 1;
        }
        i += 1;
    }
    i
}

fn ident_in(tok: &Token, src: &str, names: &[&str]) -> bool {
    tok.kind == TokenKind::Ident && names.iter().any(|n| tok.text(src).eq_ignore_ascii_case(n))
}

fn next_ident(tokens: &[Token], src: &str, mut i: usize, end_i: usize) -> Option<(String, usize)> {
    while i < end_i && tokens[i].kind == TokenKind::Comma {
        i += 1;
    }
    if i >= end_i || tokens[i].kind != TokenKind::Ident {
        return None;
    }
    Some((tokens[i].text(src).to_string(), i + 1))
}

fn parse_ep_float(tokens: &[Token], src: &str, i: usize, end_i: usize) -> Option<(f64, usize)> {
    if i >= end_i {
        return None;
    }
    let mut j = i;
    let mut sign = 1.0;
    match tokens[j].kind {
        TokenKind::Plus => {
            j += 1;
        }
        TokenKind::Minus => {
            sign = -1.0;
            j += 1;
        }
        _ => {}
    }
    if j >= end_i {
        return None;
    }
    match tokens[j].kind {
        TokenKind::Number => {
            let val: f64 = tokens[j].text(src).parse().ok()?;
            Some((sign * val, j + 1))
        }
        TokenKind::Ident if tokens[j].text(src).eq_ignore_ascii_case("inf") => {
            Some((sign * f64::INFINITY, j + 1))
        }
        _ => None,
    }
}

fn opener_at(tokens: &[Token], src: &str, i: usize) -> Option<(String, Span, usize)> {
    let tok = tokens.get(i)?;
    if tok.kind != TokenKind::Ident {
        return None;
    }
    let name = tok.text(src);
    let key = BLOCK_OPENERS
        .iter()
        .find(|k| name.eq_ignore_ascii_case(k))?
        .to_string();
    let mut j = i + 1;
    if tokens.get(j).is_some_and(|t| t.kind == TokenKind::LParen) {
        j = skip_balanced_tokens(tokens, j, TokenKind::LParen, TokenKind::RParen);
    }
    let semi = tokens.get(j)?;
    if semi.kind != TokenKind::Semi {
        return None;
    }
    Some((
        key,
        Span {
            start: tok.span.start,
            end: semi.span.end,
        },
        j,
    ))
}

fn is_end_semi(tokens: &[Token], src: &str, i: usize) -> bool {
    tokens[i].kind == TokenKind::Ident
        && tokens[i].text(src).eq_ignore_ascii_case("end")
        && tokens.get(i + 1).is_some_and(|t| t.kind == TokenKind::Semi)
}

fn complete_block_ranges(tokens: &[Token], src: &str) -> Vec<Span> {
    let mut ranges = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((_, span, semi_i)) = opener_at(tokens, src, i) {
            let mut j = semi_i + 1;
            while j < tokens.len() {
                if is_end_semi(tokens, src, j) {
                    ranges.push(Span {
                        start: span.start,
                        end: tokens[j + 1].span.end,
                    });
                    break;
                }
                if opener_at(tokens, src, j).is_some() {
                    break;
                }
                j += 1;
            }
            i = semi_i + 1;
            continue;
        }
        i += 1;
    }
    ranges
}

fn inside_span(offset: u32, ranges: &[Span]) -> bool {
    ranges.iter().any(|r| offset >= r.start && offset < r.end)
}

fn at_line_start_ident(src: &str, span: Span) -> bool {
    let start = span.start as usize;
    let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    src[line_start..start]
        .chars()
        .all(|c| c == ' ' || c == '\t')
}

fn trailing_code_line(tokens: &[Token], src: &str) -> Option<u32> {
    for tok in tokens {
        if ident_in(tok, src, TERMINAL_COMMANDS) {
            let line_start = src[..tok.span.start as usize]
                .bytes()
                .filter(|&b| b == b'\n')
                .count() as u32;
            return Some(line_start);
        }
    }
    None
}

fn leading_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if i == 0 {
            if !(c.is_ascii_alphabetic() || c == '_') {
                return None;
            }
            end = i + c.len_utf8();
        } else if c.is_ascii_alphanumeric() || c == '_' {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end == 0 {
        None
    } else {
        Some(&s[..end])
    }
}

fn strip_line_comment(line: &str) -> &str {
    if let Some(i) = line.find("//") {
        return line[..i].trim_end();
    }
    if let Some(i) = line.find('%') {
        return line[..i].trim_end();
    }
    line.trim_end()
}

fn looks_like_matlab(rhs: &str) -> bool {
    rhs.contains('\'')
        || rhs.contains('"')
        || rhs.contains('[')
        || rhs.contains(']')
        || rhs.contains('{')
        || rhs.contains('}')
        || rhs
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .any(|w| matches!(w, "M_" | "oo_" | "options_"))
}

fn line_of(src: &str, byte: u32) -> (u32, &str) {
    let byte = byte.min(src.len() as u32) as usize;
    let start = src[..byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[byte..]
        .find('\n')
        .map(|i| byte + i)
        .unwrap_or(src.len());
    let line_no = src[..start].bytes().filter(|&b| b == b'\n').count() as u32;
    (line_no, &src[start..end])
}

fn join_lexemes(src: &str, tokens: &[Token]) -> String {
    let mut out = String::new();
    let mut prev: Option<TokenKind> = None;
    for tok in tokens {
        if tok.kind == TokenKind::Eof {
            continue;
        }
        let piece = tok.text(src);
        if piece.is_empty() {
            continue;
        }
        if let Some(p) = prev {
            if needs_space(p, tok.kind) {
                out.push(' ');
            }
        }
        out.push_str(piece);
        prev = Some(tok.kind);
    }
    out
}

fn tight_right(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen
            | TokenKind::LBrack
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
            | TokenKind::Hash
    )
}

fn tight_left(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::LBrack
            | TokenKind::RBrack
            | TokenKind::Comma
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Caret
    )
}

fn needs_space(prev: TokenKind, next: TokenKind) -> bool {
    !tight_right(prev) && !tight_left(next)
}

fn equation_from_statement(raw: &str, span: Span) -> Option<Equation> {
    let (name, rest) = strip_leading_tags(raw);
    let text = collapse_ws(&rest);
    if text.is_empty() {
        return None;
    }
    if !text.chars().any(|c| c.is_ascii_alphabetic()) && !looks_like_numeric_equation(&text) {
        return None;
    }
    let (lhs, rhs) = match split_eq(&text) {
        Some((l, r)) => (l.to_string(), r.to_string()),
        None => (String::new(), String::new()),
    };
    Some(Equation {
        text,
        name,
        span,
        lhs,
        rhs,
        lhs_expr: None,
        rhs_expr: None,
        is_local: false,
        model_local: false,
        static_tag: false,
        dynamic_tag: false,
        tags: Vec::new(),
    })
}

fn strip_leading_tags(raw: &str) -> (String, String) {
    let mut s = raw.trim_start();
    let mut name = String::new();
    while s.starts_with('[') {
        let Some(end) = tag_end(s) else {
            break;
        };
        let tag = &s[..end];
        if name.is_empty() {
            if let Some(n) = tag_name_attr(tag) {
                name = n;
            }
        }
        s = s[end..].trim_start();
    }
    (name, s.to_string())
}

fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == ']' {
            return Some(i + 1);
        }
    }
    None
}

fn tag_name_attr(tag: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let key = lower.find("name")?;
    let after = &tag[key + 4..];
    let after = after.trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    let quote = after.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &after[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

fn split_eq(text: &str) -> Option<(&str, &str)> {
    // First `=` also matches `==`/`!=`/`<=`/`>=`; lhs/rhs strings are P-core
    // display slices, not comparison-aware. Do not use this for expr split.
    let idx = text.find('=')?;
    Some((text[..idx].trim(), text[idx + 1..].trim()))
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn looks_like_numeric_equation(text: &str) -> bool {
    let Some((lhs, rhs)) = split_eq(text) else {
        return false;
    };
    fn numericish(s: &str) -> bool {
        s.chars().all(|c| {
            c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | '*' | '/' | '(' | ')' | ' ')
        })
    }
    numericish(lhs) && numericish(rhs)
}

fn is_integer_lexeme(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_builtin_function(name: &str) -> bool {
    const BUILTINS: &[&str] = &[
        "exp",
        "log",
        "ln",
        "log10",
        "sqrt",
        "cbrt",
        "sign",
        "abs",
        "sin",
        "cos",
        "tan",
        "asin",
        "acos",
        "atan",
        "sinh",
        "cosh",
        "tanh",
        "asinh",
        "acosh",
        "atanh",
        "max",
        "min",
        "normcdf",
        "normpdf",
        "erf",
        "erfc",
        "log2",
        "floor",
        "ceil",
        "round",
        "norminv",
        "logncdf",
        "pac_expectation",
        "diff",
        "adl",
    ];
    BUILTINS.iter().any(|b| name.eq_ignore_ascii_case(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn parse_lhs(src: &str) -> (crate::model::Model, ExprId) {
        let model = parse(&format!("model;\n{src};\nend;\n"));
        assert_eq!(
            model.equations.len(),
            1,
            "expected one equation for {src:?}"
        );
        let id = model.equations[0].lhs_expr.expect("lhs_expr");
        (model, id)
    }

    fn kind(model: &crate::model::Model, id: ExprId) -> &ExprKind {
        &model.exprs.get(id).kind
    }

    fn name(model: &crate::model::Model, n: Name) -> &str {
        model.name(n)
    }

    #[test]
    fn timing_c_plus_one() {
        let (m, id) = parse_lhs("c(+1)");
        match kind(&m, id) {
            ExprKind::Ident {
                name: n, timing, ..
            } => {
                assert_eq!(name(&m, *n), "c");
                assert_eq!(*timing, 1);
            }
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn timing_c_one_is_lead() {
        let (m, id) = parse_lhs("c(1)");
        match kind(&m, id) {
            ExprKind::Ident {
                name: n, timing, ..
            } => {
                assert_eq!(name(&m, *n), "c");
                assert_eq!(*timing, 1);
            }
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn timing_k_minus_one() {
        let (m, id) = parse_lhs("k(-1)");
        match kind(&m, id) {
            ExprKind::Ident {
                name: n, timing, ..
            } => {
                assert_eq!(name(&m, *n), "k");
                assert_eq!(*timing, -1);
            }
            other => panic!("expected Ident, got {other:?}"),
        }
    }

    #[test]
    fn call_log_y() {
        let (m, id) = parse_lhs("log(y)");
        match kind(&m, id) {
            ExprKind::Call { callee, args } => {
                assert_eq!(name(&m, *callee), "log");
                assert_eq!(args.len(), 1);
                match kind(&m, args[0]) {
                    ExprKind::Ident {
                        name: n, timing, ..
                    } => {
                        assert_eq!(name(&m, *n), "y");
                        assert_eq!(*timing, 0);
                    }
                    other => panic!("expected Ident y, got {other:?}"),
                }
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn log_one_is_call_not_timed_log() {
        let (m, id) = parse_lhs("log(1)");
        match kind(&m, id) {
            ExprKind::Call { callee, args } => {
                assert_eq!(name(&m, *callee), "log");
                assert_eq!(args.len(), 1);
                assert!(matches!(kind(&m, args[0]), ExprKind::Number));
            }
            other => panic!("expected Call, got {other:?}"),
        }
    }

    #[test]
    fn mul_pow_timing_exp_z_k_lag() {
        let (m, id) = parse_lhs("exp(z)*k(-1)^alppha");
        match kind(&m, id) {
            ExprKind::Binary {
                op: BinOp::Mul,
                lhs,
                rhs,
            } => {
                match kind(&m, *lhs) {
                    ExprKind::Call { callee, args } => {
                        assert_eq!(name(&m, *callee), "exp");
                        assert_eq!(args.len(), 1);
                        match kind(&m, args[0]) {
                            ExprKind::Ident {
                                name: n, timing, ..
                            } => {
                                assert_eq!(name(&m, *n), "z");
                                assert_eq!(*timing, 0);
                            }
                            other => panic!("expected Ident z, got {other:?}"),
                        }
                    }
                    other => panic!("expected Call exp, got {other:?}"),
                }
                match kind(&m, *rhs) {
                    ExprKind::Binary {
                        op: BinOp::Pow,
                        lhs: k,
                        rhs: a,
                    } => {
                        match kind(&m, *k) {
                            ExprKind::Ident {
                                name: n, timing, ..
                            } => {
                                assert_eq!(name(&m, *n), "k");
                                assert_eq!(*timing, -1);
                            }
                            other => panic!("expected Ident k, got {other:?}"),
                        }
                        match kind(&m, *a) {
                            ExprKind::Ident {
                                name: n, timing, ..
                            } => {
                                assert_eq!(name(&m, *n), "alppha");
                                assert_eq!(*timing, 0);
                            }
                            other => panic!("expected Ident alppha, got {other:?}"),
                        }
                    }
                    other => panic!("expected Pow, got {other:?}"),
                }
            }
            other => panic!("expected Mul, got {other:?}"),
        }
    }

    #[test]
    fn steady_state_and_expectation() {
        let (m, id) = parse_lhs("STEADY_STATE(y(+1))");
        match kind(&m, id) {
            ExprKind::SteadyState { arg } => match kind(&m, *arg) {
                ExprKind::Ident {
                    name: n, timing, ..
                } => {
                    assert_eq!(name(&m, *n), "y");
                    assert_eq!(*timing, 1);
                }
                other => panic!("expected Ident y(+1), got {other:?}"),
            },
            other => panic!("expected SteadyState, got {other:?}"),
        }

        let (m, id) = parse_lhs("EXPECTATION(-1)(x(+1))");
        match kind(&m, id) {
            ExprKind::Expectation { shift, arg } => {
                assert_eq!(*shift, -1);
                match kind(&m, *arg) {
                    ExprKind::Ident {
                        name: n, timing, ..
                    } => {
                        assert_eq!(name(&m, *n), "x");
                        assert_eq!(*timing, 1);
                    }
                    other => panic!("expected Ident x(+1), got {other:?}"),
                }
            }
            other => panic!("expected Expectation, got {other:?}"),
        }
    }

    #[test]
    fn comments_and_strings_are_not_ident_nodes() {
        let (m, id) = parse_lhs("a + /* sneaky_ident */ b");
        match kind(&m, id) {
            ExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                assert!(matches!(kind(&m, *lhs), ExprKind::Ident { .. }));
                assert!(matches!(kind(&m, *rhs), ExprKind::Ident { .. }));
            }
            other => panic!("expected Add, got {other:?}"),
        }
        let refs: Vec<_> = m
            .exprs
            .walk_idents(id)
            .map(|r| m.name(r.name).to_string())
            .collect();
        assert_eq!(refs, vec!["a", "b"]);

        let (m, id) = parse_lhs("a + 'sneaky_ident'");
        match kind(&m, id) {
            ExprKind::Binary {
                op: BinOp::Add,
                lhs,
                rhs,
            } => {
                assert!(matches!(kind(&m, *lhs), ExprKind::Ident { .. }));
                assert!(matches!(kind(&m, *rhs), ExprKind::String));
            }
            other => panic!("expected Add, got {other:?}"),
        }
        let refs: Vec<_> = m
            .exprs
            .walk_idents(id)
            .map(|r| m.name(r.name).to_string())
            .collect();
        assert_eq!(refs, vec!["a"]);
    }

    #[test]
    fn endval_block_fills_entries() {
        let model = parse("endval; x = 1; end;");
        assert_eq!(model.endval.len(), 1);
        assert!(model.endval_block.is_some());
        assert_eq!(model.name(model.endval[0].name), "x");
        assert_eq!(model.endval[0].expression, "1");
    }

    #[test]
    fn static_dynamic_tags_set_and_text_drops_brackets() {
        let model = parse("model; [static] y = 1; [dynamic] y = 1; end;");
        assert_eq!(model.equations.len(), 2);
        assert_eq!(model.equations[0].text, "y = 1");
        assert_eq!(model.equations[0].tags, vec!["static".to_string()]);
        assert!(model.equations[0].static_tag);
        assert!(!model.equations[0].dynamic_tag);
        assert_eq!(model.equations[1].text, "y = 1");
        assert_eq!(model.equations[1].tags, vec!["dynamic".to_string()]);
        assert!(model.equations[1].dynamic_tag);
        assert!(!model.equations[1].static_tag);
    }
}
