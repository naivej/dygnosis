use super::{Site, SourceInventory, SourceOptionValue, Token, TokenKind, literal_span, syntax};
use crate::engine::catalog::syntax::{SyntaxRecord, SyntaxSelector};
use crate::span::Span;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
};

const SAMPLER_OPTIONS: &str = "posterior_sampler_options";
const TARB: &str = "tailored_random_block_metropolis_hastings";

#[derive(Clone, Copy)]
struct Element<'a> {
    tokens: &'a [Token],
    start: u32,
    end: u32,
}

struct Elements<'a> {
    elements: Vec<Element<'a>>,
    closed: bool,
    consumed: usize,
}

/// Split only at commas in the current list. Tokens inside strings are opaque.
fn elements(tokens: &[Token], mut start: u32) -> Option<Elements<'_>> {
    if tokens.len() > 8192 {
        return None;
    }
    let mut elements = Vec::new();
    let mut begin = 0;
    let mut stack = Vec::new();
    for (at, token) in tokens.iter().enumerate() {
        if at % 128 == 0 && crate::engine::control::cancelled() {
            return None;
        }
        if stack.is_empty()
            && matches!(
                token.kind,
                TokenKind::Comma | TokenKind::RParen | TokenKind::Semi | TokenKind::Eof
            )
        {
            elements.push(Element {
                tokens: &tokens[begin..at],
                start,
                end: token.span.start,
            });
            if token.kind != TokenKind::Comma {
                return Some(Elements {
                    elements,
                    closed: token.kind == TokenKind::RParen,
                    consumed: at + 1,
                });
            }
            begin = at + 1;
            start = token.span.end;
            continue;
        }
        match token.kind {
            TokenKind::LParen | TokenKind::LBrack if stack.len() < 32 => stack.push(token.kind),
            TokenKind::RParen if stack.pop() == Some(TokenKind::LParen) => {}
            TokenKind::RBrack if stack.pop() == Some(TokenKind::LBrack) => {}
            TokenKind::LParen
            | TokenKind::LBrack
            | TokenKind::RParen
            | TokenKind::RBrack
            | TokenKind::Semi
            | TokenKind::Eof => return None,
            _ => {}
        }
    }
    elements.push(Element {
        tokens: &tokens[begin..],
        start,
        end: tokens
            .last()
            .map_or(start, |token| token.span.end)
            .max(start),
    });
    Some(Elements {
        elements,
        closed: false,
        consumed: tokens.len(),
    })
}

fn list(tokens: &[Token]) -> Option<Elements<'_>> {
    let (opener, rest) = tokens.split_first()?;
    if opener.kind != TokenKind::LParen {
        return None;
    }
    let list = elements(rest, opener.span.end)?;
    (list.consumed == rest.len()).then_some(list)
}

fn key<'a>(tokens: &[Token], text: &'a str) -> Option<&'a str> {
    let [token] = tokens else { return None };
    if token.kind != TokenKind::String {
        return None;
    }
    let key = text
        .get(token.span.start as usize..token.span.end as usize)?
        .strip_prefix('\'')?
        .strip_suffix('\'')?;
    (!key.contains('.')).then_some(key)
}

/// The retained lexer skips punctuation such as colons. Check the gaps at
/// the selected literal so an ignored character cannot change its role.
fn after_trivia(text: &str) -> Option<&str> {
    if text.len() > 8192 {
        return None;
    }
    let start = crate::engine::language::native_line::initial_content_start(text, 0);
    text.get(start..)
}
fn trivia(text: &str) -> bool {
    after_trivia(text) == Some("")
}

fn token_gap<'a>(text: &'a str, left: &Token, right: &Token) -> &'a str {
    &text[left.span.end as usize..right.span.start as usize]
}

impl Element<'_> {
    fn literal_gaps(self, text: &str) -> bool {
        let (Some(first), Some(last)) = (self.tokens.first(), self.tokens.last()) else {
            return true;
        };
        trivia(&text[self.start as usize..first.span.start as usize])
            && trivia(&text[last.span.end as usize..self.end as usize])
    }
}

#[derive(Default)]
struct Facts<'a>(BTreeMap<&'a str, Option<&'a [Token]>>);

impl<'a> Facts<'a> {
    fn add(&mut self, name: &'a str, value: Option<&'a [Token]>) {
        self.0
            .entry(name)
            .and_modify(|old| *old = None)
            .or_insert(value);
    }

    fn get(&self, name: &str) -> Option<&'a [Token]> {
        self.0.get(name).copied().flatten()
    }

    fn contains(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
}

pub(super) struct Context<'a> {
    command: &'a str,
    text: &'a str,
    options: Facts<'a>,
    sampler: Facts<'a>,
    sampler_closed: bool,
    header_closed: bool,
    key_site: bool,
    optimizer_steps: Option<Vec<(i32, i32)>>,
}

impl Context<'_> {
    pub(super) fn key_site(&self) -> bool {
        self.key_site
    }

    fn selector_tokens(&self, selector: &SyntaxSelector) -> Option<&[Token]> {
        match selector.scope.as_deref() {
            Some(SAMPLER_OPTIONS) => self.sampler.get(&selector.option),
            None => self.options.get(&selector.option),
            _ => None,
        }
    }

    pub(super) fn written_selector(&self, selector: &SyntaxSelector) -> Option<&str> {
        let [token] = self.selector_tokens(selector)? else {
            return None;
        };
        Some(token.text(self.text))
    }

    fn compare(&self, selector: &SyntaxSelector) -> Option<bool> {
        if selector.option == "posterior_sampling_method" {
            return self.sampler_is(&selector.value);
        }
        let tokens = self.selector_tokens(selector)?;
        if selector.option == "load_mh_file" {
            return tokens.is_empty().then_some(selector.value == "true");
        }
        if tokens.is_empty() {
            return Some(selector.value == "true");
        }
        match selector.option.as_str() {
            "proposal_distribution" if !matches!(tokens, [token] if token.kind == TokenKind::String) =>
            {
                return None;
            }
            "filter_algorithm" | "proposal_approximation" | "distribution_approximation" if !matches!(tokens, [token] if token.kind == TokenKind::Ident) =>
            {
                return None;
            }
            _ => {}
        }
        if selector.scope.is_some()
            && let Ok(expected) = selector.value.parse::<i32>()
        {
            return nested_number_matches(tokens, self.text, expected);
        }
        scalar_matches(tokens, self.text, &selector.value)
    }

    fn sampler_name(&self) -> Option<&str> {
        let [token] = self.options.get("posterior_sampling_method")? else {
            return None;
        };
        if token.kind != TokenKind::String {
            return None;
        }
        token.text(self.text).strip_prefix('\'')?.strip_suffix('\'')
    }

    fn sampler_is(&self, expected: &str) -> Option<bool> {
        self.sampler_name().map(|name| name == expected)
    }

    fn optimizer(&self, value: &str) -> Option<bool> {
        let primary = self.options.get("mode_compute");
        let selected = primary.and_then(|tokens| scalar_matches(tokens, self.text, value));
        if self.command.eq_ignore_ascii_case("estimation") {
            if primary.and_then(|tokens| scalar_matches(tokens, self.text, "0")) == Some(true)
                || self
                    .options
                    .get("mh_posterior_mode_estimation")
                    .is_some_and(<[Token]>::is_empty)
            {
                return Some(false);
            }
            let primary = primary?;
            if integer(primary, self.text).is_none()
                && !matches!(primary, [token] if token.kind == TokenKind::Ident)
                || self.options.contains("mh_posterior_mode_estimation")
            {
                return None;
            }
            if self.options.contains("posterior_sampling_method") {
                match self.sampler_name()? {
                    "hssmc" | "dime_mcmc" | "online" | "dsmh" => return Some(false),
                    "random_walk_metropolis_hastings"
                    | "independent_metropolis_hastings"
                    | TARB
                    | "slice" => {}
                    _ => return None,
                }
            }
        }
        if selected == Some(true) {
            return selected;
        }
        if let Some(steps) = &self.optimizer_steps {
            let value = value.parse::<i32>().ok()?;
            return Some(
                steps
                    .iter()
                    .any(|&(first, last)| (first..=last).contains(&value)),
            );
        }
        if self.options.contains("additional_optimizer_steps") {
            return None;
        }
        selected
    }

    fn inherited_optimizer(&self, value: &str) -> Option<bool> {
        if !self.header_closed || !self.sampler_closed || self.sampler.contains("optim") {
            return None;
        }
        if !self.sampler_is(TARB)? {
            return Some(false);
        }
        nested_number_matches(
            self.sampler.get("mode_compute")?,
            self.text,
            value.parse().ok()?,
        )
    }

    pub(super) fn applies(&self, record: &SyntaxRecord) -> bool {
        if record.applicability.is_empty() {
            return record
                .selector
                .as_ref()
                .is_none_or(|selector| self.condition(record, selector));
        }
        record
            .applicability
            .iter()
            .any(|all| all.iter().all(|selector| self.condition(record, selector)))
    }

    fn condition(&self, record: &SyntaxRecord, selector: &SyntaxSelector) -> bool {
        if selector.scope.as_deref() == Some("optimizer_dispatch") {
            return self.optimizer(&selector.value) == Some(true)
                || record.slot.as_deref() != Some("optim.verbosity")
                    && self.inherited_optimizer(&selector.value) == Some(true);
        }
        self.compare(selector) == Some(true)
    }
}

pub(super) fn setting_help(record: &SyntaxRecord) -> String {
    if record.applicability_text.is_empty() {
        record.summary.clone()
    } else {
        format!("{}\n\n{}", record.summary, record.applicability_text)
    }
}
fn integer(tokens: &[Token], text: &str) -> Option<i64> {
    let [number] = tokens else { return None };
    let digits = number.text(text);
    (number.kind == TokenKind::Number && digits.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
}

fn scalar_matches(tokens: &[Token], text: &str, expected: &str) -> Option<bool> {
    if let Ok(expected) = expected.parse::<i64>() {
        return Some(integer(tokens, text)? == expected);
    }
    let [token] = tokens else { return None };
    let actual = token.text(text);
    match token.kind {
        TokenKind::Ident => Some(actual.eq_ignore_ascii_case(expected)),
        TokenKind::String => Some(
            actual
                .strip_prefix('\'')?
                .strip_suffix('\'')?
                .eq_ignore_ascii_case(expected),
        ),
        _ => None,
    }
}

fn nested_number_matches(tokens: &[Token], text: &str, expected: i32) -> Option<bool> {
    let (sign, number) = match tokens {
        [number] => (1.0, number),
        [sign, number] if trivia(token_gap(text, sign, number)) => match sign.kind {
            TokenKind::Plus => (1.0, number),
            TokenKind::Minus => (-1.0, number),
            _ => return None,
        },
        _ => return None,
    };
    if number.kind != TokenKind::Number {
        return None;
    }
    let actual: f64 = number.text(text).parse().ok()?;
    actual
        .is_finite()
        .then_some(sign * actual == f64::from(expected))
}

fn optimizer_steps(tokens: &[Token], text: &str) -> Option<Vec<(i32, i32)>> {
    let [open, inner @ .., close] = tokens else {
        return None;
    };
    if open.kind != TokenKind::LBrack || close.kind != TokenKind::RBrack {
        return None;
    }
    let number = |token: &Token| {
        (token.kind == TokenKind::Number)
            .then(|| token.text(text).parse::<i32>().ok())
            .flatten()
    };
    let mut steps = Vec::new();
    let gap = |left: &Token, right: &Token| after_trivia(token_gap(text, left, right));
    let first = inner.first()?;
    if !gap(open, first)?.is_empty() {
        return None;
    }
    let mut at = usize::from(first.kind == TokenKind::Comma);
    if at == 1 && !gap(first, inner.get(at)?)?.is_empty() {
        return None;
    }
    while at < inner.len() {
        if at % 128 == 0 && crate::engine::control::cancelled() {
            return None;
        }
        let first = number(&inner[at])?;
        at += 1;
        let last = match gap(&inner[at - 1], inner.get(at).unwrap_or(close))? {
            "" => first,
            tail if tail.strip_prefix(':').is_some_and(trivia) => {
                let last = number(inner.get(at)?)?;
                at += 1;
                if !gap(&inner[at - 1], inner.get(at).unwrap_or(close))?.is_empty() {
                    return None;
                }
                last
            }
            _ => return None,
        };
        steps.push((first, last));
        if let Some(comma) = inner.get(at).filter(|token| token.kind == TokenKind::Comma) {
            at += 1;
            if !gap(comma, inner.get(at).unwrap_or(close))?.is_empty() {
                return None;
            }
        }
    }
    (!steps.is_empty()).then_some(steps)
}

fn complete_pairs(elements: &Elements<'_>, text: &str) -> bool {
    let (pairs, remainder) = elements.elements.as_chunks::<2>();
    elements.closed
        && remainder.is_empty()
        && pairs.iter().all(|pair| {
            key(pair[0].tokens, text).is_some()
                && !pair[1].tokens.is_empty()
                && pair[0].literal_gaps(text)
                && pair[1].literal_gaps(text)
        })
}

fn find_pair<'a>(
    list: &Elements<'a>,
    text: &'a str,
    byte: u32,
    completion: bool,
) -> Option<(&'a str, Element<'a>, bool)> {
    for pair in list.elements.as_chunks::<2>().0 {
        let name = key(pair[0].tokens, text)?;
        if !pair[0].literal_gaps(text) {
            return None;
        }
        let key_span = pair[0].tokens[0].span;
        if !completion && key_span.start <= byte && byte < key_span.end {
            return Some((name, pair[0], true));
        }
        let value = pair[1];
        let end = value.tokens.last().map_or(byte, |token| token.span.end);
        if value.start <= byte && byte <= end {
            return Some((name, value, false));
        }
        if byte < value.start {
            return None;
        }
    }
    None
}

pub(super) fn site<'a>(
    inventory: &SourceInventory,
    text: &'a str,
    byte: u32,
    completion: bool,
    source: &SourceOptionValue<'a>,
) -> Option<Site<'a>> {
    let root = if source.option.bytes().any(|byte| byte.is_ascii_uppercase()) {
        Cow::Owned(source.option.to_ascii_lowercase())
    } else {
        Cow::Borrowed(source.option)
    };
    syntax::written_slot(Some(source.command), &root)?;
    let opener = source.tokens.first()?;
    if !trivia(&text[source.start as usize..opener.span.start as usize])
        || !trivia(&text[source.key_to_equal.start as usize..source.key_to_equal.end as usize])
    {
        return None;
    }
    let outer = list(source.tokens)?;
    let (name, mut value, mut key_site) = find_pair(&outer, text, byte, completion)?;
    let mut path = format!("{root}.{name}");
    if !key_site
        && value
            .tokens
            .first()
            .is_some_and(|token| token.kind == TokenKind::LParen)
    {
        if !value.literal_gaps(text) {
            return None;
        }
        // Only catalogued pair-list paths can own a second level.
        let container = syntax::written_slot(Some(source.command), &path)?;
        if !container.settings.iter().any(|row| {
            row.value_domain.as_ref().is_some_and(|domain| {
                domain
                    .open_rules
                    .iter()
                    .any(|rule| rule == "name_value_pairs")
            })
        }) {
            return None;
        }
        let inner = list(value.tokens)?;
        let (name, inner_value, inner_key) = find_pair(&inner, text, byte, completion)?;
        path.push('.');
        path.push_str(name);
        value = inner_value;
        key_site = inner_key;
    }
    let slot = syntax::written_slot(Some(source.command), &path)?;
    let path = slot.settings.first()?.slot.as_deref()?;
    let span = if key_site {
        value.tokens[0].span
    } else {
        if !value.literal_gaps(text)
            || !completion
                && matches!(value.tokens, [token] if token.kind == TokenKind::Ident && !matches!(token.text(text), "true" | "false"))
        {
            return None;
        }
        literal_span(value.tokens, text, value.start, byte, completion)?
    };
    let literal = if let [sign, number] = value.tokens
        && matches!(sign.kind, TokenKind::Plus | TokenKind::Minus)
    {
        Cow::Owned(format!("{}{}", sign.text(text), number.text(text)))
    } else {
        Cow::Borrowed(text.get(span.start as usize..span.end as usize)?)
    };
    let anchored = |end: u32| {
        inventory.literal_value_anchor(
            Span {
                start: source.anchor_start,
                end: end.max(byte),
            },
            byte,
        )
    };
    let mut context = Context {
        command: source.command,
        text,
        options: Facts::default(),
        sampler: Facts::default(),
        sampler_closed: false,
        header_closed: false,
        key_site,
        optimizer_steps: None,
    };
    let mut command_selectors = BTreeSet::new();
    let mut sampler_selectors = BTreeSet::new();
    let mut optimizer_dispatch = false;
    for row in slot
        .settings
        .iter()
        .copied()
        .chain(syntax::slot_values(Some(source.command), path))
    {
        let selectors = row.applicability.iter().flatten();
        for selector in selectors {
            match selector.scope.as_deref() {
                Some(SAMPLER_OPTIONS) => {
                    sampler_selectors.insert(selector.option.as_str());
                }
                Some("optimizer_dispatch") => optimizer_dispatch = true,
                None => {
                    command_selectors.insert(selector.option.as_str());
                }
                _ => {}
            }
        }
    }
    if optimizer_dispatch {
        command_selectors.extend([
            "mode_compute",
            "additional_optimizer_steps",
            "posterior_sampling_method",
            SAMPLER_OPTIONS,
            "mh_posterior_mode_estimation",
        ]);
        sampler_selectors.extend(["mode_compute", "optim"]);
    }
    if let Some(header) = elements(source.header, source.header_start)
        && source
            .header
            .get(header.consumed.saturating_sub(1))
            .is_some_and(|token| anchored(token.span.end))
    {
        context.header_closed = header.closed;
        for element in header.elements {
            let Some((key, rest)) = element.tokens.split_first() else {
                continue;
            };
            if key.kind != TokenKind::Ident {
                continue;
            }
            let spelling = key.text(text);
            let normalized = command_selectors
                .iter()
                .copied()
                .find(|known| spelling.eq_ignore_ascii_case(known));
            let Some(name) = normalized else { continue };
            let value = element
                .literal_gaps(text)
                .then(|| match rest {
                    [] => Some(rest),
                    [equal, values @ ..]
                        if equal.kind == TokenKind::Eq
                            && !values.is_empty()
                            && trivia(token_gap(text, key, equal))
                            && (!matches!(
                                name,
                                "mode_compute"
                                    | "opt_algo"
                                    | "solve_algo"
                                    | "nonlinear_filter_initialization"
                            ) || matches!(values, [_]))
                            && trivia(token_gap(text, equal, &values[0])) =>
                    {
                        Some(values)
                    }
                    _ => None,
                })
                .flatten();
            context.options.add(name, value);
        }
    }
    if optimizer_dispatch {
        context.optimizer_steps = context
            .options
            .get("additional_optimizer_steps")
            .and_then(|tokens| optimizer_steps(tokens, text));
    }
    let sampler = if root == SAMPLER_OPTIONS {
        Some(outer)
    } else {
        context.options.get(SAMPLER_OPTIONS).and_then(list)
    };
    if let Some(sampler) = sampler {
        context.sampler_closed = complete_pairs(&sampler, text);
        for pair in sampler.elements.as_chunks::<2>().0 {
            let Some(name) = key(pair[0].tokens, text) else {
                break;
            };
            if !sampler_selectors.contains(name) {
                continue;
            }
            let value = pair[1].tokens;
            context.sampler.add(
                name,
                value
                    .last()
                    .filter(|token| {
                        pair[0].literal_gaps(text)
                            && pair[1].literal_gaps(text)
                            && anchored(token.span.end)
                    })
                    .map(|_| value),
            );
        }
    }
    Some(Site {
        parent: Some(source.command),
        slot: path,
        span,
        literal,
        nested: Some(context),
        closed: false,
    })
}
