//! Compatible written tokens shared by one editor projection.
use super::*;
use crate::engine::language::lexer::Token;
use std::borrow::Cow;

#[cfg(test)]
thread_local! {
    pub(super) static COLOR_TIMING_ANALYSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static SELECTION_TOKEN_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn selection_tokens(text: &str) -> Vec<Token> {
    #[cfg(test)]
    SELECTION_TOKEN_SCANS.with(|scans| scans.set(scans.get() + 1));
    // The structure inventory lexes inert macro placeholders, which can join
    // adjacent written identifiers. Raw selection requires the original lexer.
    tokenize(text)
}

pub(super) fn color_tokens<'a>(text: &str, retained: Option<&'a [Token]>) -> Cow<'a, [Token]> {
    retained
        .map(Cow::Borrowed)
        .unwrap_or_else(|| Cow::Owned(tokenize(text)))
}

pub(super) fn color_timing(
    model: &Model,
    mapping: &SemanticMapping,
) -> HashMap<String, crate::engine::analysis::timing::TimingInfo> {
    if mapping.forward == 0 && mapping.predetermined == 0 {
        return HashMap::new();
    }
    #[cfg(test)]
    COLOR_TIMING_ANALYSES.with(|analyses| analyses.set(analyses.get() + 1));
    classify_variable_timing(model)
}
