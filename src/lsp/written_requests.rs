//! Source-only editor answers, independent of the mutable model lock.
use super::*;
use crate::engine::workspace::SourceDocument;

/// Some includes a proven empty result; None keeps model-aware providers eligible.
pub(super) fn hover(source: &SourceDocument, position: Position) -> Option<Option<Hover>> {
    use crate::engine::analysis::source_inventory::SourceItemKind;
    let text = source.source();
    let index = source.source_index();
    let byte = index.offset_utf16(text, span_pos(position));
    let inventory = source.inventory()?;
    if !syntax_intel::hover_may_need_model(&inventory, text, byte) {
        return Some(None);
    }
    let (word, span) = syntax_intel::candidate_word_at(text, byte)?;
    if !crate::engine::catalog::syntax::records()
        .iter()
        .any(|record| {
            record.name.eq_ignore_ascii_case(word)
                || record.kind == "macro_directive"
                    && record
                        .name
                        .trim_start_matches("@#")
                        .eq_ignore_ascii_case(word)
                || record
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(word))
        })
    {
        return None;
    }
    let range = Some(span_range(&index, text, span));
    if macro_directive_name_with_tokens(text, byte, &inventory.macro_tokens) == Some(span) {
        // Conditional headers need the selected arm's activity, including an active else.
        // Other macro keywords retain source-only syntax help.
        if ["if", "ifdef", "ifndef", "elseif", "else"]
            .iter()
            .any(|name| word.eq_ignore_ascii_case(name))
        {
            return None;
        }
        let result =
            crate::engine::catalog::syntax::query(&crate::engine::catalog::syntax::SyntaxQuery {
                name: Some(word.into()),
                kind: Some("macro_directive".into()),
                ..Default::default()
            });
        return result
            .records
            .first()
            .map(|record| Some(markdown_hover(syntax_intel::documentation(record), range)));
    }
    let context = syntax_intel::source_context(&inventory, text, byte, false)?;
    let item = inventory
        .items
        .iter()
        .filter(|item| {
            (matches!(item.kind, SourceItemKind::Block | SourceItemKind::Command)
                || item.kind == SourceItemKind::Declaration && item.span != item.selection)
                && !item.template
                && item.span.start <= byte
                && byte
                    < if item.kind == SourceItemKind::Block {
                        item.header_end
                    } else {
                        item.span.end
                    }
        })
        .min_by_key(|item| item.span.end - item.span.start)?;
    if item.selection != span {
        let at = inventory
            .lexical_tokens
            .partition_point(|token| token.span.end <= byte);
        let token = inventory.lexical_tokens.get(at)?;
        if token.span != span
            || !inventory
                .lexical_tokens
                .get(at + 1)
                .is_some_and(|token| token.kind == TokenKind::Eq)
        {
            return None;
        }
        let command = item.name.as_deref()?;
        let option =
            syntax_intel::options(command).find(|option| option.name.eq_ignore_ascii_case(word))?;
        let description = shocks_overwrite_doc(text, byte, command, &option.name, &option.summary);
        return Some(Some(markdown_hover(
            format!("**`{command}` option**: `{word}`\n\n{description}"),
            range,
        )));
    }
    if item
        .name
        .as_deref()
        .is_none_or(|name| !name.eq_ignore_ascii_case(word))
    {
        return None;
    }
    let matches: Vec<_> = syntax_intel::records(&context)
        .into_iter()
        .filter(|record| {
            record.name.eq_ignore_ascii_case(word)
                || record
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(word))
        })
        .map(syntax_intel::documentation)
        .collect();
    (!matches.is_empty()).then(|| {
        Some(markdown_hover(
            format!("**`{word}`**\n\n{}", matches.join("\n\n")),
            range,
        ))
    })
}

impl Backend {
    pub(super) fn written_source(&self, uri: &Url) -> Option<SourceDocument> {
        if let Some(source) = self.written_sources.lock().unwrap().get(uri).cloned() {
            return Some(source);
        }
        // Compatibility for library callers supplying Workspace inputs directly.
        let inner = self.lock_inner();
        inner.docs.get(uri)?;
        let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
        let source = inner.workspace.source_document(uri.as_str(), &file)?;
        self.written_sources
            .lock()
            .unwrap()
            .insert(uri.clone(), source.clone());
        Some(source)
    }
    pub(super) fn refresh_written_sources(&self, inner: &Inner) {
        let mut sources = self.written_sources.lock().unwrap();
        sources.retain(|uri, _| inner.docs.contains_key(uri));
        for uri in inner.docs.keys() {
            let file = crate::engine::workspace::include_resolver::normalize_uri(uri.as_str());
            if let Some(source) = inner.workspace.source_document(uri.as_str(), &file) {
                sources.insert(uri.clone(), source);
            } else {
                sources.remove(uri);
            }
        }
    }
    pub(super) fn written_colors_empty(&self, uri: &Url, range: Range) -> bool {
        let Some(source) = self.written_source(uri) else {
            return false;
        };
        let Some(inventory) = source.inventory() else {
            return false;
        };
        if !inventory.structure_complete() {
            return false;
        }
        let index = source.source_index();
        let start = index.offset_utf16(source.source(), span_pos(range.start));
        let end = index.offset_utf16(source.source(), span_pos(range.end));
        let intersects = |token: &crate::engine::language::lexer::Token| {
            token.span.start < end && start < token.span.end
        };
        // A prior macro can change lexical context, including by ending a
        // comment before this range. Written trivia is proof only before any
        // macro input that can affect the requested prefix.
        if inventory
            .lexical_tokens
            .iter()
            .any(|token| token.kind != TokenKind::Eof && intersects(token))
            || inventory
                .macro_tokens
                .iter()
                .take_while(|token| token.span.start < end)
                .any(|token| {
                    matches!(
                        token.kind,
                        TokenKind::MacroDir | TokenKind::MacroInterp | TokenKind::MacroPrefix
                    )
                })
        {
            return false;
        }
        // Includes can inherit comment or string context from another file.
        // Use the existing owner/pending-work proof; a file suffix is not proof
        // that the displayed file starts a fresh lexical context.
        let inner = match self.inner.try_lock() {
            Ok(inner) => inner,
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return false,
        };
        if !inner.source_has_root_context(uri) {
            return false;
        }
        self.written_sources
            .lock()
            .unwrap()
            .get(uri)
            .is_some_and(|current| current.same_source(&source))
    }

    pub(super) fn written_binding_may_exist(&self, pos: &TextDocumentPositionParams) -> bool {
        let Some(source) = self.written_source(&pos.text_document.uri) else {
            return true;
        };
        let Some(inventory) = source.inventory() else {
            return true;
        };
        let byte = source
            .source_index()
            .offset_utf16(source.source(), span_pos(pos.position));
        syntax_intel::hover_may_need_model(&inventory, source.source(), byte)
            || !self
                .written_sources
                .lock()
                .unwrap()
                .get(&pos.text_document.uri)
                .is_some_and(|current| current.same_source(&source))
    }
}
