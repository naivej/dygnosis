//! Written-source presentation over the shared inventory and availability trace.

use super::*;
use crate::engine::analysis::source_inventory::{SourceItem, SourceItemKind};
use crate::engine::analysis_configuration::{
    SourceAvailability, SourceFileSelection, SourceFileStructure, SourceStructureQuery,
};
use crate::engine::workspace::include_resolver::normalize_uri;

pub(super) fn file(inner: &mut Inner, document: &Url) -> Option<SourceFileStructure> {
    let roots = inner.context_roots(document);
    if roots.len() != 1 {
        return None;
    }
    let root = &roots[0];
    inner.prepare_root(root);
    let key = normalize_uri(document.as_str());
    inner
        .workspace
        .source_structure(
            root.as_str(),
            &SourceStructureQuery {
                files: vec![if root == document {
                    SourceFileSelection::Root
                } else {
                    SourceFileSelection::File(key.clone())
                }],
            },
        )
        .ok()?
        .files
        .into_iter()
        .find(|file| {
            file.file.as_deref() == Some(key.as_str())
                || (file.file.is_none() && document.scheme() == "untitled" && root == document)
        })
}

pub(super) fn availability_label(state: SourceAvailability) -> &'static str {
    match state {
        SourceAvailability::Active => "active",
        SourceAvailability::Inactive => "inactive",
        SourceAvailability::Mixed => "mixed",
        SourceAvailability::Unknown => "unknown",
    }
}

/// Semantic facts use the root's selected configuration; written source remains unchanged.
pub(super) fn label(inner: &Inner, document: &Url, hover: &mut Hover) {
    let roots = inner.context_roots(document);
    if roots.len() != 1 {
        return;
    }
    let choices = inner
        .workspace
        .root_analysis_configuration(roots[0].as_str());
    let Some(name) = choices.name.as_deref().filter(|name| {
        !matches!(
            *name,
            "Analysis preview"
                | "Analysis preview (modified)"
                | "Macro inputs"
                | "Macro inputs (modified)"
        )
    }) else {
        return;
    };
    let detail = format!(
        "\n\nAnalysis: {name}{}",
        if choices.is_preview() {
            " (preview)"
        } else {
            ""
        }
    );
    match &mut hover.contents {
        HoverContents::Markup(markup) => markup.value.push_str(&detail),
        HoverContents::Scalar(MarkedString::String(value)) => value.push_str(&detail),
        HoverContents::Array(values) => values.push(MarkedString::String(detail)),
        HoverContents::Scalar(value) => {
            hover.contents = HoverContents::Array(vec![value.clone(), MarkedString::String(detail)])
        }
    }
}

/// Resolve literal written include links without acquiring any dormant body.
pub(super) fn literal_links(
    inner: &mut Inner,
    document: &Url,
    text: &str,
    index: &LineIndex,
) -> Vec<DocumentLink> {
    let roots = inner.context_roots(document);
    if roots.len() != 1 {
        return Vec::new();
    }
    let root = &roots[0];
    inner.prepare_root(root);
    let Some(inventory) = inner.workspace.source_inventory(document.as_str()) else {
        return Vec::new();
    };
    let including = normalize_uri(document.as_str());
    let Ok(mut lookup) = inner.workspace.source_include_lookup(root.as_str()) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for token in inventory
        .macro_tokens
        .iter()
        .filter(|token| token.kind == TokenKind::MacroDir && !token.comment_context)
    {
        let Some(literal) =
            crate::engine::language::parser::literal_include_filename(token.text(text))
        else {
            continue;
        };
        let target = match inner
            .workspace
            .source_include_target(&mut lookup, &including, &literal)
        {
            Ok(Some(target)) => target,
            Ok(None) => continue,
            Err(_) => break,
        };
        let Some(uri) = file_url_from_path_key(&target).or_else(|| Url::parse(&target).ok()) else {
            continue;
        };
        links.push(DocumentLink {
            range: span_range(index, text, token.span),
            target: Some(uri),
            tooltip: Some(format!("Open Source: {literal}")),
            data: Some(json!({"dygnosis_source_include":true,"root_uri":root})),
        });
    }
    links
}

pub(super) fn hover(inner: &mut Inner, document: &Url, byte: u32) -> Option<Hover> {
    let file = file(inner, document)?;
    let item = file
        .inventory
        .items
        .iter()
        .filter(|item| item.span.start <= byte && byte < item.span.end)
        .min_by_key(|item| item.span.end - item.span.start)?;
    if syntax_intel::display_equation_tag_at(&file.inventory, &file.text, item, byte) {
        return None;
    }
    let availability = file
        .availability
        .iter()
        .find(|row| row.source_id == item.source_id);
    let control = matches!(
        item.kind,
        SourceItemKind::Conditional | SourceItemKind::Branch
    ) && item.selection.start <= byte
        && byte < item.selection.end;
    let state = availability.map_or(SourceAvailability::Unknown, |row| {
        if control {
            row.control_availability
        } else {
            row.availability
        }
    });
    let root = inner.context_roots(document).into_iter().next()?;
    let choices = inner.workspace.root_analysis_configuration(root.as_str());
    let overridden = choices.overrides.iter().find_map(|choice| {
        file.sites
            .iter()
            .find(|site| {
                &site.target == choice.target()
                    && site.directive_span.start <= byte
                    && byte < site.directive_span.end
            })
            .map(|site| (choice, site))
    });
    if state == SourceAvailability::Active && overridden.is_none() && !item.template && !control {
        return None;
    }
    if !control
        && overridden.is_none()
        && let Some((word, span)) = syntax_intel::candidate_word_at(&file.text, byte)
        && let Some(context) =
            syntax_intel::source_context(&file.inventory, &file.text, byte, false)
        && let Some(description) = syntax_intel::written_syntax_help(
            &file.inventory,
            &file.text,
            byte,
            word,
            span,
            &context,
        )
    {
        let index = inner
            .workspace
            .source_document(root.as_str(), &normalize_uri(document.as_str()))
            .map(|source| source.source_index())
            .unwrap_or_else(|| Arc::new(LineIndex::new(&file.text)));
        return Some(markdown_hover(
            description,
            Some(span_range(&index, &file.text, span)),
        ));
    }
    // A containing statement supplies availability, not the hovered name.
    // Reuse its retained lexical tokens for a precise inner token and range.
    let (name, selection, option) = if let Some(span) = file.inventory.ordinary_name_use(item, byte)
    {
        (
            &file.text[span.start as usize..span.end as usize],
            span,
            None,
        )
    } else if item.selection.start <= byte && byte < item.selection.end {
        (
            item.name.as_deref().unwrap_or("Written source"),
            item.selection,
            None,
        )
    } else if let Some(site) = file
        .inventory
        .named_sites
        .iter()
        .find(|site| site.span.start <= byte && byte < site.span.end)
    {
        (
            file.text[site.span.start as usize..site.span.end as usize].trim_matches(['\'', '"']),
            site.span,
            None,
        )
    } else if item.kind == SourceItemKind::MacroDefinition && overridden.is_some() {
        // A preview receipt describes the whole written directive, including
        // its keyword and replacement expression outside the defined name.
        (
            item.name.as_deref().unwrap_or("Written source"),
            item.span,
            None,
        )
    } else {
        let tokens = &file.inventory.lexical_tokens;
        let at = tokens.partition_point(|token| token.span.end <= byte);
        let token = tokens.get(at).filter(|token| {
            token.kind == TokenKind::Ident
                && token.span.start <= byte
                && byte < token.span.end
                && item.span.start <= token.span.start
                && token.span.end <= item.span.end
        })?;
        let name = token.text(&file.text);
        let option = if item.kind == SourceItemKind::Command
            && tokens
                .get(at + 1)
                .is_some_and(|next| next.kind == TokenKind::Eq)
        {
            item.name.as_deref().and_then(|command| {
                syntax_intel::options(command)
                    .find(|option| option.name.eq_ignore_ascii_case(name))
                    .map(|option| (command, option))
            })
        } else {
            None
        };
        (name, token.span, option)
    };
    let declaration_lookup = ((state == SourceAvailability::Inactive
        || state == SourceAvailability::Unknown && root != *document)
        && option.is_none())
    .then(|| {
        inner.workspace.written_declaration_at_use(
            root.as_str(),
            document.as_str(),
            &file,
            item,
            byte,
        )
    });
    let use_declaration = declaration_lookup
        .as_ref()
        .and_then(|lookup| lookup.declaration.as_ref());
    let mut message = if control || overridden.is_some() {
        format!(
            "**{name}** · {}",
            if state == SourceAvailability::Mixed {
                "active in some expansions"
            } else {
                availability_label(state)
            }
        )
    } else if item.kind == SourceItemKind::Declaration && selection == item.selection {
        declaration_description(inner, document, name, item)
    } else if let Some(declaration) = use_declaration {
        let mut description = if let Some(rhs) = declaration.item.local_definition {
            let text = if declaration.file == normalize_uri(document.as_str()) {
                Some(file.text.as_str())
            } else {
                declaration_lookup
                    .as_ref()
                    .and_then(|lookup| {
                        lookup
                            .sources
                            .iter()
                            .find(|source| source.file == declaration.file)
                    })
                    .map(|source| source.text.as_str())
            };
            format!(
                "**Written model-local variable** {}\n\nExpression: {}",
                names::code(name),
                names::code(
                    text.and_then(|text| text.get(rhs.start as usize..rhs.end as usize))
                        .unwrap_or_default()
                )
            )
        } else {
            declaration_description(
                inner,
                document,
                declaration.item.name.as_deref().unwrap_or(name),
                &declaration.item,
            )
        };
        if declaration.file != normalize_uri(document.as_str()) {
            let root_file = normalize_uri(root.as_str());
            let path = std::path::Path::new(&declaration.file);
            let display = std::path::Path::new(&root_file)
                .parent()
                .and_then(|parent| path.strip_prefix(parent).ok())
                .unwrap_or(path)
                .to_string_lossy();
            let location = format!("{}:{}", display.replace('\\', "/"), declaration.line);
            if let Some(url) = file_url_from_path_key(&declaration.file) {
                description.push_str(&format!(
                    "\n\nDeclared in: [{}]({url}#L{}).",
                    names::literal(&location),
                    declaration.line
                ));
            } else {
                description.push_str(&format!("\n\nDeclared in: {}.", names::code(&location)));
            }
        }
        description
    } else if selection == item.selection
        && let Some(rhs) = item.local_definition
    {
        let expression = file
            .text
            .get(rhs.start as usize..rhs.end as usize)
            .unwrap_or_default();
        format!(
            "**Written model-local variable** {}\n\nExpression: {}",
            names::code(name),
            names::code(expression)
        )
    } else {
        let reason = match state {
            SourceAvailability::Inactive => {
                "This source is inactive in the selected analysis, so model information is unavailable."
            }
            SourceAvailability::Mixed => {
                "This source has different activity across expansions, so model information is unavailable."
            }
            SourceAvailability::Unknown => {
                "Source activity is unknown, so model information is unavailable."
            }
            SourceAvailability::Active => {
                "Model information is unavailable for this source template."
            }
        };
        format!("**{name}**\n\n{reason}")
    };
    if let Some(cause) = declaration_lookup
        .as_ref()
        .and_then(|lookup| lookup.cause.as_ref())
    {
        message.push_str("\n\n");
        message.push_str(cause);
    }
    if state == SourceAvailability::Unknown
        && let Some(cause) = availability.and_then(|row| {
            if control {
                row.control_cause.as_ref()
            } else {
                row.cause.as_ref()
            }
        })
    {
        message.push_str("\n\n");
        message.push_str(&cause.message);
    }
    if let Some((command, option)) = option {
        message.push_str(&format!(
            "\n\n**`{command}` option**: `{name}`\n\n{}",
            option.summary
        ));
    }
    if let Some((choice, site)) = overridden {
        use crate::engine::analysis_configuration::AnalysisOverride;
        let value = match choice {
            AnalysisOverride::Definition { expression, .. } => {
                format!("Replacement expression: `{expression}`")
            }
            AnalysisOverride::SkipDefinition { .. } => {
                "Skip this definition; keep any earlier value.".into()
            }
            AnalysisOverride::Branch { branch_id, .. } => {
                if let Some(branch_id) = branch_id {
                    site.branches
                        .iter()
                        .find(|branch| &branch.branch_id == branch_id)
                        .map_or_else(
                            || "The selected branch is unavailable in this source.".into(),
                            |branch| format!("Branch preview: `{}`", branch.expected_text),
                        )
                } else {
                    "Skip this conditional for analysis.".into()
                }
            }
        };
        message.push_str(&format!("\n\n{value}"));
        if let Ok(metadata) = inner.workspace.analysis_metadata(root.as_str())
            && let Some(receipt) = metadata
                .overrides
                .iter()
                .find(|receipt| receipt.operation.target() == choice.target())
            && (receipt.application
                != crate::engine::analysis_configuration::OverrideApplication::Applied
                || receipt.cause.is_some())
        {
            let application = match receipt.application {
                crate::engine::analysis_configuration::OverrideApplication::Applied => "applied",
                crate::engine::analysis_configuration::OverrideApplication::NotReached => {
                    "not reached"
                }
                crate::engine::analysis_configuration::OverrideApplication::Unknown => "unknown",
            };
            message.push_str(&format!("\n\nApplication: {application}."));
            if let Some(cause) = &receipt.cause {
                message.push(' ');
                message.push_str(cause);
            }
        }
    }
    let index = inner
        .workspace
        .source_document(root.as_str(), &normalize_uri(document.as_str()))
        .map(|source| source.source_index())
        .unwrap_or_else(|| Arc::new(LineIndex::new(&file.text)));
    let mut hover = markdown_hover(message, Some(span_range(&index, &file.text, selection)));
    label(inner, document, &mut hover);
    Some(hover)
}

fn declaration_description(inner: &Inner, document: &Url, name: &str, item: &SourceItem) -> String {
    let kind = item.written_kind.as_deref().unwrap_or("declaration");
    let mut parts = vec![
        names::declaration_heading(name, kind, true)
            .or_else(|| names::typed_heading(name, kind, true))
            .unwrap_or_else(|| {
                format!(
                    "**Written declaration**: {} ({})",
                    names::code(name),
                    names::code(kind)
                )
            }),
    ];
    if let Some(namespace) = &item.source_namespace {
        parts.push(format!("Owner: {}", names::code(namespace)));
    }
    if names::typed_label(kind).is_some()
        && let Some(record) = crate::engine::catalog::syntax::records()
            .iter()
            .find(|record| {
                record.name == kind
                    && matches!(record.kind.as_str(), "command" | "block" | "declaration")
            })
    {
        parts.push(record.summary.clone());
    }
    if let Some(metadata) = &item.declaration_metadata {
        let settings = inner.presentation_for(document);
        names::append_name_metadata(
            &mut parts,
            metadata.long_name.as_deref(),
            metadata.tex_name.as_deref(),
            &settings.name_details,
        );
    }
    parts.join("\n\n")
}

pub(super) fn candidates(inner: &mut Inner, arguments: &[Value]) -> Value {
    let Some(params) = arguments
        .first()
        .and_then(|value| serde_json::from_value::<TextDocumentPositionParams>(value.clone()).ok())
    else {
        return json!({"code":"INVALID_ARGUMENTS","error":"Source candidates require a document URI and cursor position."});
    };
    let document = &params.text_document.uri;
    let Some(doc) = inner.document(document) else {
        return json!({"code":"DOCUMENT_NOT_OPEN","error":"Open the written source before finding source candidates."});
    };
    let text = doc.text.clone();
    let version = doc.version;
    let index = LineIndex::new(&text);
    let byte = index.offset_utf16(&text, span_pos(params.position));
    let Some((name, _)) = syntax_intel::word_at(&text, byte) else {
        return json!({"code":"SOURCE_CANDIDATE_UNAVAILABLE","error":"Place the cursor on a written name to find source candidates."});
    };
    let Some(inventory) = inner.workspace.source_inventory(document.as_str()) else {
        return json!({"code":"SOURCE_CANDIDATE_UNAVAILABLE","error":"Written source information is unavailable for this document."});
    };
    let projected = file(inner, document);
    let rows: Vec<_> = inventory.compatible_candidates(byte).filter(|item| item.name.as_deref() == Some(name.as_str()))
        .map(|item| {
            let state = projected.as_ref().and_then(|file| file.availability.iter().find(|row| row.source_id == item.source_id))
                .map_or(SourceAvailability::Unknown, |row| row.availability);
            json!({"source_id":item.source_id,"name":name,"label":"source candidate · unverified","kind":item.written_kind,"availability":availability_label(state),
                "location":{"uri":document,"range":span_range(&index,&text,item.selection)}})
        }).collect();
    json!({"schema_version":1,"document_uri":document,"document_version":version,"source_revision":inventory.source_revision,
        "candidates":rows,"message":if rows.is_empty() {format!("No declaration of {name} was found in {}.{}", document.path().rsplit('/').next().unwrap_or("this file"), inventory.coverage_cause.as_ref().map(|cause|format!(" {cause}")).unwrap_or_default())} else {"Choose a source declaration; this does not establish references.".into()}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn active_and_written_declaration_metadata_share_style_and_preferences() {
        // Guards style drift, lost Markdown escaping or settings, and selected
        // model values/timing leaking into written-source descriptions.
        let root = Url::parse("untitled:hover-style.mod").unwrap();
        for (kind, role) in [
            ("var", "Endogenous variable"),
            ("parameters", "Parameter"),
            ("varexo", "Exogenous variable"),
            ("varexo_det", "Deterministic exogenous variable"),
        ] {
            for active in [0, 1] {
                let calibration = if kind == "parameters" {
                    "subject=0.875;\n"
                } else {
                    ""
                };
                let text = format!(
                    "var y;\n{kind} subject $S`label$ (long_name='Capital *[gross]*');\n{calibration}@#if {active}\nmodel;y=subject;end;\n@#endif\n"
                );
                let (service, _socket) = new_service();
                let server = service.inner();
                server.upsert(root.clone(), text.clone(), 1);
                let byte = text.find("subject;end").unwrap();
                let pos = TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root.clone() },
                    position: span_range(&LineIndex::new(&text), &text, Span::new(byte, byte))
                        .start,
                };
                for (long_name, tex) in [(true, true), (true, false), (false, true), (false, false)]
                {
                    server
                        .lock_inner()
                        .settings
                        .apply(&json!({"nameDetails":{"longName":long_name,"tex":tex}}));
                    let result = server.hover_at(&pos).unwrap();
                    let HoverContents::Markup(markup) = result.contents else {
                        panic!("markdown hover")
                    };
                    let heading = if active == 1 {
                        format!("**{role}**: `subject`")
                    } else {
                        format!("**Written {}**: `subject`", role.to_ascii_lowercase())
                    };
                    assert!(
                        markup.value.starts_with(&heading),
                        "{kind}/{active}: {}",
                        markup.value
                    );
                    assert_eq!(
                        markup.value.contains(r"Capital \*\[gross\]\*"),
                        long_name,
                        "{}",
                        markup.value
                    );
                    assert_eq!(
                        markup.value.contains("TeX: ``S`label``"),
                        tex,
                        "{}",
                        markup.value
                    );
                    assert!(
                        !markup.value.contains("Long name:")
                            && !markup.value.contains("TeX label:"),
                        "{}",
                        markup.value
                    );
                    if active == 0 {
                        assert!(
                            !markup.value.contains("Value:")
                                && !markup.value.contains("Timing:")
                                && !markup.value.contains("0.875"),
                            "{}",
                            markup.value
                        );
                    } else if kind == "parameters" {
                        assert!(markup.value.contains("Value: `0.875`"), "{}", markup.value);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn active_and_written_model_local_definitions_share_expression_style() {
        // Guards source definition hover keeping the old name-first layout
        // while the active definition uses a role heading and Expression label.
        let root = Url::parse("untitled:local-hover-style.mod").unwrap();
        for active in [0, 1] {
            let text = format!("var y;\n@#if {active}\nmodel;#temp=log(1);y=temp;end;\n@#endif\n");
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text.clone(), 1);
            let byte = text.find("temp=log").unwrap();
            let result = server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root.clone() },
                    position: span_range(&LineIndex::new(&text), &text, Span::new(byte, byte))
                        .start,
                })
                .unwrap();
            let HoverContents::Markup(markup) = result.contents else {
                panic!("markdown hover")
            };
            let heading = if active == 1 {
                "**Model-local variable** `temp`"
            } else {
                "**Written model-local variable** `temp`"
            };
            assert!(
                markup.value.starts_with(heading) && markup.value.contains("Expression: `log(1)`"),
                "{}",
                markup.value
            );
            assert!(
                !markup.value.contains("Value:") && !markup.value.contains("Timing:"),
                "{}",
                markup.value
            );
        }
    }

    struct IncludeFiles(std::path::PathBuf);
    impl IncludeFiles {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "dygnosis-hover-includes-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn root(&self) -> Url {
            Url::from_file_path(self.0.join("main.mod")).unwrap()
        }
    }
    impl Drop for IncludeFiles {
        fn drop(&mut self) {
            for name in ["decl.inc", "nested.inc"] {
                let _ = std::fs::remove_file(self.0.join(name));
            }
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[tokio::test]
    async fn inactive_include_hover_reads_nested_declarations_without_executing_them() {
        // Guards unopened declaration includes, unknown variable timing parsed
        // as a generic call, stale disk metadata, and new semantic dependencies.
        use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
        let files = IncludeFiles::new();
        let root = files.root();
        let child = Url::from_file_path(files.0.join("decl.inc")).unwrap();
        std::fs::write(
            files.0.join("decl.inc"),
            "@#include \"nested.inc\"\nparameters rho $R$ (long_name='Persistence');\n",
        )
        .unwrap();
        std::fs::write(
            files.0.join("nested.inc"),
            "var capital $K$ (long_name='Capital');\n",
        )
        .unwrap();
        let text = "var live;model;live=0;end;\n@#if 0\n@#include \"decl.inc\"\nmodel;capital=capital(-1)+rho;end;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        let revision = server
            .lock_inner()
            .workspace
            .input_revision(root.as_str())
            .unwrap();
        let at = |character| TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: Position::new(3, character),
        };
        let capital = serde_json::to_string(&server.hover_at(&at(14)).unwrap()).unwrap();
        assert!(
            capital.contains("**Written ")
                && capital.contains("Capital")
                && capital.contains("nested.inc#L1"),
            "{capital}"
        );
        let parameter = serde_json::to_string(&server.hover_at(&at(26)).unwrap()).unwrap();
        assert!(
            parameter.contains("Persistence") && parameter.contains("decl.inc#L2"),
            "{parameter}"
        );
        let builds = PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
        assert_eq!(
            serde_json::to_string(&server.hover_at(&at(14)).unwrap()).unwrap(),
            capital
        );
        assert_eq!(
            PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
            builds,
            "warm includes reuse the existing source inventories"
        );
        std::fs::write(
            files.0.join("nested.inc"),
            "var capital $K$ (long_name='Updated');\n",
        )
        .unwrap();
        let updated = serde_json::to_string(&server.hover_at(&at(14)).unwrap()).unwrap();
        assert!(
            updated.contains("Updated") && !updated.contains("Capital"),
            "{updated}"
        );
        let mut inner = server.lock_inner();
        assert_eq!(
            inner.workspace.input_revision(root.as_str()).unwrap(),
            revision
        );
        assert!(inner.workspace.owner_roots(child.as_str()).is_empty());
        assert!(inner.workspace.source_inventory(child.as_str()).is_none());
        assert!(
            inner
                .workspace
                .include_records(root.as_str())
                .unwrap()
                .resolved
                .is_empty()
        );
    }

    #[tokio::test]
    async fn inactive_include_hover_withholds_ambiguous_or_unchecked_candidates() {
        // Guards a local declaration hiding an include competitor, computed
        // and cyclic edges, and named operators mistaken for ordinary names.
        let root = Url::parse("file:///C:/dygnosis-hover-candidates/main.mod").unwrap();
        let child = Url::parse("file:///C:/dygnosis-hover-candidates/decl.inc").unwrap();
        for (body, include, declaration, expression, reason) in [
            (
                "var shared_name;",
                "\"decl.inc\"",
                "var shared_name;",
                "shared_name",
                "More than one",
            ),
            (
                "var shared_name;",
                "PATH",
                "var shared_name;",
                "shared_name",
                "computed include",
            ),
            (
                "@#include \"decl.inc\"",
                "\"decl.inc\"",
                "",
                "shared_name",
                "cycle",
            ),
            (
                "@#if FLAG\nvar shared_name;\n@#endif",
                "\"decl.inc\"",
                "",
                "shared_name",
                "another macro branch",
            ),
            (
                "external_function(name=shared_name);",
                "\"decl.inc\"",
                "var shared_name;",
                "shared_name(1)",
                "not supported by source lookup",
            ),
        ] {
            let (service, _socket) = new_service();
            let server = service.inner();
            server
                .lock_inner()
                .workspace
                .update_document(child.as_str(), body);
            let text = format!(
                "@#if 0\n{declaration}\n@#include {include}\nmodel;y={expression};end;\n@#endif\n"
            );
            server.upsert(root.clone(), text.clone(), 1);
            let result = hover(
                &mut server.lock_inner(),
                &root,
                text.rfind("shared_name").unwrap() as u32,
            )
            .unwrap();
            let value = serde_json::to_string(&result).unwrap();
            assert!(
                !value.contains("**Written ") && value.contains(reason),
                "{text}: {value}"
            );
        }
    }

    #[tokio::test]
    async fn dormant_editor_hover_uses_a_direct_parent_route_and_existing_inventories() {
        // Guards Unknown availability suppressing source facts in an opened
        // dormant include, and a new parse just to use a smaller hover allowance.
        use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
        let root = Url::parse("file:///C:/dygnosis-hover-parent/main.mod").unwrap();
        let child = Url::parse("file:///C:/dygnosis-hover-parent/decl.inc").unwrap();
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(),"parameters rho $R$ (long_name='Persistence');\n@#if 0\n@#include \"decl.inc\"\n@#endif\n".into(),1);
        server.upsert(
            child.clone(),
            "var local_name;\nmodel;local_name=rho;end;\n".into(),
            1,
        );
        {
            let mut inner = server.lock_inner();
            inner
                .document_contexts
                .insert(normalize_uri(child.as_str()), root.clone());
            inner.workspace.source_inventory(root.as_str()).unwrap();
            inner.workspace.source_inventory(child.as_str()).unwrap();
        }
        let builds = PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
        let result = server
            .hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: child.clone() },
                position: Position::new(1, 17),
            })
            .unwrap();
        let value = serde_json::to_string(&result).unwrap();
        assert!(
            value.contains("Persistence") && value.contains("main.mod#L1"),
            "{value}"
        );
        assert_eq!(
            PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
            builds,
            "existing complete inventories remain reusable under the hover cap"
        );
        server.upsert(
            root.clone(),
            "@#if 0\n@#include \"decl.inc\"\n@#include \"decl.inc\"\n@#endif\n".into(),
            2,
        );
        let result = server
            .hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: child },
                position: Position::new(1, 6),
            })
            .unwrap();
        let value = serde_json::to_string(&result).unwrap();
        assert!(
            value.contains("**Written ")
                && value.contains("Parent-file declarations were not checked"),
            "{value}"
        );
    }

    #[tokio::test]
    async fn include_hover_preserves_overlay_precedence_and_enforces_source_bounds() {
        // Guards a cached disk body replacing an open overlay, and an automatic
        // hover reading an oversized include or borrowing a sibling search path.
        let files = IncludeFiles::new();
        let root = files.root();
        let child = Url::from_file_path(files.0.join("decl.inc")).unwrap();
        std::fs::write(
            files.0.join("decl.inc"),
            "parameters rho (long_name='Disk');",
        )
        .unwrap();
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(
            root.clone(),
            "@#if 0\n@#include \"decl.inc\"\nmodel;y=rho;end;\n@#endif\n".into(),
            1,
        );
        let at = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: Position::new(2, 8),
        };
        assert!(
            serde_json::to_string(&server.hover_at(&at).unwrap())
                .unwrap()
                .contains("Disk")
        );
        server
            .lock_inner()
            .workspace
            .update_document(child.as_str(), "parameters rho (long_name='Overlay');");
        assert!(
            serde_json::to_string(&server.hover_at(&at).unwrap())
                .unwrap()
                .contains("Overlay")
        );
        server
            .lock_inner()
            .workspace
            .remove_document(child.as_str());
        assert!(
            serde_json::to_string(&server.hover_at(&at).unwrap())
                .unwrap()
                .contains("Disk")
        );
        std::fs::write(files.0.join("decl.inc"), " ".repeat(256 * 1024 + 1)).unwrap();
        let bounded = serde_json::to_string(&server.hover_at(&at).unwrap()).unwrap();
        assert!(
            bounded.contains("262144-byte source limit") && !bounded.contains("**Written "),
            "{bounded}"
        );

        let other = IncludeFiles::new();
        std::fs::write(other.0.join("nested.inc"), "parameters rho;").unwrap();
        let search = other.0.to_string_lossy().replace('\\', "/");
        let text = format!(
            "@#if 1\n@#includepath \"{search}\"\n@#else\n@#include \"nested.inc\"\nmodel;y=rho;end;\n@#endif\n"
        );
        server.upsert(root.clone(), text, 2);
        let value = serde_json::to_string(
            &server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root },
                    position: Position::new(4, 8),
                })
                .unwrap(),
        )
        .unwrap();
        assert!(
            !value.contains("**Written ")
                && value.contains("unavailable in the configured include paths"),
            "{value}"
        );
        let relative = format!(
            "../{}/nested.inc",
            other.0.file_name().unwrap().to_string_lossy()
        );
        server.upsert(at.text_document.uri.clone(),format!("@#if 0\n@#includepath \"{search}\"\n@#include \"{relative}\"\nmodel;y=rho;end;\n@#endif\n"),3);
        let value = serde_json::to_string(
            &server
                .hover_at(&TextDocumentPositionParams {
                    text_document: at.text_document,
                    position: Position::new(3, 8),
                })
                .unwrap(),
        )
        .unwrap();
        assert!(
            value.contains("**Written ") && value.contains("nested.inc#L1"),
            "direct relative includes do not depend on written search paths: {value}"
        );
    }

    #[tokio::test]
    async fn include_hover_bounds_nested_acquisition_before_a_later_declaration() {
        // Guards a deep literal chain consuming unbounded source work on hover.
        let root = Url::parse("file:///C:/dygnosis-hover-depth/main.mod").unwrap();
        let (service, _socket) = new_service();
        let server = service.inner();
        for index in 0..9 {
            let child = format!("file:///C:/dygnosis-hover-depth/child{index}.inc");
            let text = if index == 8 {
                "parameters rho;".into()
            } else {
                format!("@#include \"child{}.inc\"\n", index + 1)
            };
            server.lock_inner().workspace.update_document(&child, text);
        }
        server.upsert(
            root.clone(),
            "@#if 0\n@#include \"child0.inc\"\nmodel;y=rho;end;\n@#endif\n".into(),
            1,
        );
        let value = serde_json::to_string(
            &server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root },
                    position: Position::new(2, 8),
                })
                .unwrap(),
        )
        .unwrap();
        assert!(
            !value.contains("**Written ") && value.contains("8-level include limit"),
            "{value}"
        );
    }

    #[tokio::test]
    async fn inactive_include_hover_recovers_when_the_configured_work_allowance_is_raised() {
        // Guards an independent include-inventory ceiling ignoring the user's
        // larger root allowance, including reuse after an incomplete attempt.
        use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
        let root = Url::parse("file:///C:/dygnosis-hover-work/main.mod").unwrap();
        let child = Url::parse("file:///C:/dygnosis-hover-work/decl.inc").unwrap();
        let (service, _socket) = new_service();
        let server = service.inner();
        let rows = 700;
        server
            .lock_inner()
            .workspace
            .update_document(child.as_str(), "parameters filler;\n".repeat(rows));
        server.upsert(root.clone(),"parameters mu_cngdp;\nvar live;model;live=0;end;\n@#if 0\n@#include \"decl.inc\"\nmodel;live=mu_cngdp;end;\n@#endif\n".into(),1);
        let at = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root },
            position: Position::new(4, 11),
        };
        server
            .lock_inner()
            .settings
            .apply(&json!({"macroWorkLimit":200_000}));
        let low = serde_json::to_string(&server.hover_at(&at).unwrap()).unwrap();
        assert!(
            low.contains("work allowance"),
            "low configured allowance must reach the actual source mechanism: {low}"
        );
        for limit in [2_000_000, 20_000_000] {
            server
                .lock_inner()
                .settings
                .apply(&json!({"macroWorkLimit":limit}));
            let value = serde_json::to_string(&server.hover_at(&at).unwrap()).unwrap();
            assert!(
                value.contains("**Written parameter**") && !value.contains("work allowance"),
                "raising configured work to {limit} must release source inspection ({rows} rows): {value}"
            );
            let builds = PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
            assert_eq!(
                serde_json::to_string(&server.hover_at(&at).unwrap()).unwrap(),
                value
            );
            assert_eq!(
                PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
                builds,
                "successful written source facts must be reused on repeat hover"
            );
        }
        server
            .lock_inner()
            .settings
            .apply(&json!({"macroWorkLimit":200_000}));
        let lowered = serde_json::to_string(&server.hover_at(&at).unwrap()).unwrap();
        assert!(
            lowered.contains("work allowance") && !lowered.contains("**Written parameter**"),
            "a completed high-work inventory must not bypass a later lower allowance: {lowered}"
        );
    }

    #[tokio::test]
    async fn inactive_name_uses_show_written_declarations_without_model_facts() {
        // Guards inactive uses losing declaration metadata, borrowing selected
        // model values/timing, or rebuilding the retained source inventory.
        use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;

        let root = Url::parse("file:///C:/dygnosis-inactive-use/main.mod").unwrap();
        let text = "var capital $K$ (long_name='Capital');\nparameters rho $R$ (long_name='Persistence');\nrho=0.9;\nmodel;capital=rho*capital(-1);end;\n@#if 0\nvar hidden $H$ (long_name='Hidden variable');\nmodel;\n# temp = rho*hidden;\nhidden = capital(+2)+rho+temp;\nend;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        let inventory = file(&mut server.lock_inner(), &root).unwrap().inventory;
        let builds = PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
        for _ in 0..2 {
            for (line, character, name, kind, long_name, tex) in [
                (7, 9, "rho", "parameter", "Persistence", "R"),
                (
                    7,
                    13,
                    "hidden",
                    "endogenous variable",
                    "Hidden variable",
                    "H",
                ),
                (
                    8,
                    0,
                    "hidden",
                    "endogenous variable",
                    "Hidden variable",
                    "H",
                ),
                (8, 9, "capital", "endogenous variable", "Capital", "K"),
                (8, 21, "rho", "parameter", "Persistence", "R"),
            ] {
                let hover = server
                    .hover_at(&TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: root.clone() },
                        position: Position::new(line, character),
                    })
                    .unwrap();
                let HoverContents::Markup(markup) = hover.contents else {
                    panic!("markdown hover")
                };
                assert_eq!(
                    markup.value,
                    format!("**Written {kind}**: `{name}`\n\n{long_name}\n\nTeX: `{tex}`")
                );
                assert_eq!(
                    hover.range.unwrap(),
                    Range::new(
                        Position::new(line, character),
                        Position::new(line, character + name.len() as u32)
                    )
                );
            }
        }
        assert_eq!(PARSE_EXPANDED_BUILDS.with(|builds| builds.get()), builds);
        assert!(Arc::ptr_eq(
            &inventory,
            &file(&mut server.lock_inner(), &root).unwrap().inventory
        ));

        server
            .lock_inner()
            .settings
            .apply(&json!({"nameDetails":{"longName":false,"tex":false}}));
        let hover = server
            .hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root },
                position: Position::new(8, 9),
            })
            .unwrap();
        let HoverContents::Markup(markup) = hover.contents else {
            panic!("markdown hover")
        };
        assert_eq!(markup.value, "**Written endogenous variable**: `capital`");
    }

    #[tokio::test]
    async fn inactive_name_uses_do_not_guess_written_bindings() {
        // Guards false attribution from a sibling/nested branch, duplicate
        // declarations, model-local scopes, equation tags, or qualified names.
        let root = Url::parse("file:///C:/dygnosis-inactive-ambiguous/main.mod").unwrap();
        for (declarations, expression, resolves) in [
            ("var shared_name;\n", "shared_name", true),
            ("@#if 1\nvar shared_name;\n@#else\n", "shared_name", false),
            ("@#if 0\nvar shared_name;\n@#endif\n", "shared_name", false),
            ("var shared_name; var shared_name;\n", "shared_name", false),
            (
                "model;# shared_name=1;end;\nvar shared_name;\n",
                "shared_name",
                true,
            ),
            ("var shared_name;\n", "pkg.shared_name", false),
            ("var shared_name;\n", "shared_name.field", false),
            (
                "parameters shared_name;\n",
                "VAR_EXPECTATION(shared_name)",
                false,
            ),
            (
                "parameters shared_name;\n",
                "PAC_EXPECTATION(shared_name)",
                false,
            ),
            ("parameters shared_name;\n", "@{callee}(shared_name)", false),
        ] {
            let has_sibling = declarations.contains("@#else");
            let text = if has_sibling {
                format!("{declarations}model;y={expression};end;\n@#endif\n")
            } else {
                format!("@#if 0\n{declarations}model;y={expression};end;\n@#endif\n")
            };
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text.clone(), 1);
            let byte = text.rfind("shared_name").unwrap() as u32;
            let mut inner = server.lock_inner();
            if declarations.starts_with("model;#") {
                let inventory = file(&mut inner, &root).unwrap().inventory;
                assert!(
                    inventory
                        .items
                        .iter()
                        .any(|row| row.name.as_deref() == Some("shared_name")
                            && row.local_definition.is_some()),
                    "fixture must exercise the retained local definition"
                );
                assert!(
                    inventory
                        .compatible_candidates(byte)
                        .any(|row| row.name.as_deref() == Some("shared_name")),
                    "fixture must also retain a declaration candidate"
                );
            }
            let result = hover(&mut inner, &root, byte).unwrap();
            let HoverContents::Markup(markup) = result.contents else {
                panic!("markdown hover")
            };
            assert_eq!(
                markup.value.contains("**Written "),
                resolves,
                "{text}: {}",
                markup.value
            );
            if !resolves {
                assert!(markup.value.contains("inactive"), "{}", markup.value);
            }
        }

        let text = "@#if 0\nvar shared_name;\nmodel;[shared_name='label'] y=0;end;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        let mut inner = server.lock_inner();
        assert!(hover(&mut inner, &root, text.rfind("shared_name").unwrap() as u32).is_none());
    }

    #[tokio::test]
    async fn inactive_uses_keep_sibling_metadata_and_command_roles_separate() {
        // Guards active sibling metadata leaking into an inactive name, and
        // command option names being mistaken for trailing ordinary-name uses.
        let root = Url::parse("file:///C:/dygnosis-inactive-sibling/main.mod").unwrap();
        let text = "@#if 1\nparameters shared_name $A$ (long_name='Active parameter');\nshared_name=0.75;\nvar y;model;y=shared_name;end;\n@#else\nvar shared_name $B$ (long_name='Inactive variable');\nmodel;shared_name=shared_name(-2);end;\nstoch_simul(order=1) shared_name;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        for (line, character) in [(6, 6), (6, 18), (7, 21)] {
            let result = server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root.clone() },
                    position: Position::new(line, character),
                })
                .unwrap();
            let HoverContents::Markup(markup) = result.contents else {
                panic!("markdown hover")
            };
            assert_eq!(
                markup.value,
                "**Written endogenous variable**: `shared_name`\n\nInactive variable\n\nTeX: `B`"
            );
        }
        let result = server
            .hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root },
                position: Position::new(7, 13),
            })
            .unwrap();
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("**Written ")
        );
    }

    #[tokio::test]
    async fn inactive_hover_preserves_local_syntax_and_written_facts() {
        // Guards catalog help, exact macro token selection, and the boundary
        // between a written definition and an inactive use of the same name.
        let root = Url::parse("file:///C:/dygnosis-inactive-hover/main.mod").unwrap();
        let text = "@#if 0\nvar hidden $H$ (long_name='Hidden variable');\nvar_model(structural);\nmodel(linear);\n# temp = log(1);\nhidden = exp(temp);\nend;\n@#define n = length([1,2])\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        let hover = |line: u32, column: u32| {
            server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root.clone() },
                    position: Position::new(line, column),
                })
                .unwrap()
        };
        for (line, column, word, kind) in [
            (2, 10, "structural", "option"),
            (3, 6, "linear", "option"),
            (4, 9, "log", "function"),
            (5, 9, "exp", "function"),
            (7, 13, "length", "macro_function"),
        ] {
            let result = hover(line, column);
            let value = serde_json::to_string(&result).unwrap();
            let record = crate::engine::catalog::syntax::records()
                .iter()
                .find(|record| {
                    record.kind == kind
                        && record.name.eq_ignore_ascii_case(word)
                        && (kind != "option"
                            || record.parent.as_deref()
                                == Some(if word == "linear" {
                                    "model"
                                } else {
                                    "var_model"
                                }))
                })
                .unwrap();
            assert!(value.contains(&record.summary), "{word}: {value}");
            assert!(!value.contains("inactive"), "{word}: {value}");
            if word == "length" {
                assert_eq!(
                    result.range.unwrap(),
                    Range::new(Position::new(7, 13), Position::new(7, 19))
                );
            }
        }
        let declaration = serde_json::to_string(&hover(1, 5)).unwrap();
        assert!(
            declaration.contains("**Written ")
                && declaration.contains("Hidden variable")
                && declaration.contains('H'),
            "{declaration}"
        );
        let definition = serde_json::to_string(&hover(4, 3)).unwrap();
        assert!(definition.contains("log(1)"), "{definition}");
        let usage = serde_json::to_string(&hover(5, 13)).unwrap();
        assert!(
            usage.contains("**Written model-local variable**") && usage.contains("log(1)"),
            "{usage}"
        );
    }

    #[tokio::test]
    async fn inactive_body_keyword_uses_its_containing_block() {
        // Guards the body keyword being replaced by availability-only hover.
        let root = Url::parse("file:///C:/dygnosis-inactive-body/main.mod").unwrap();
        let text = "@#if 0\nshocks;\nvar e;\nstderr 1;\nend;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        let hover = server
            .hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root },
                position: Position::new(3, 1),
            })
            .unwrap();
        let value = serde_json::to_string(&hover).unwrap();
        let record = crate::engine::catalog::syntax::records()
            .iter()
            .find(|record| {
                record.kind == "body_keyword"
                    && record.name == "stderr"
                    && record.parent.as_deref() == Some("shocks")
            })
            .unwrap();
        assert!(value.contains(&record.summary), "{value}");
        assert!(!value.contains("inactive"), "{value}");
    }

    #[tokio::test]
    async fn written_macro_function_ignores_quoted_text() {
        // A prior string must not hide a later call, and a call-shaped string
        // must not receive function documentation.
        let root = Url::parse("file:///C:/dygnosis-macro-hover/main.mod").unwrap();
        let text = "@#define n = \"foo\" + length([1,2])\n@#define s = \"length([1,2])\"\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        let at = |line, character| TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: Position::new(line, character),
        };
        let call = server.syntax_hover_at(&at(0, 22)).unwrap();
        assert!(serde_json::to_string(&call).unwrap().contains("length"));
        assert!(server.syntax_hover_at(&at(1, 16)).is_none());
    }

    #[tokio::test]
    async fn written_catalog_help_does_not_claim_names_or_qualified_calls() {
        // Guards catalog names in declarations, option values and qualified
        // calls from being presented as built-in calls.
        let root = Url::parse("file:///C:/dygnosis-catalog-collisions/main.mod").unwrap();
        for (text, character) in [
            ("external_function(name=f);\n", 23),
            ("var log;\n", 4),
            ("var_model(model_name=log);\n", 21),
            ("model; y=pkg.log(1); end;\n", 14),
            ("@#define length(x) = x\n", 10),
        ] {
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text.into(), 1);
            let position = TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root.clone() },
                position: Position::new(0, character),
            };
            assert!(
                server.syntax_hover_at(&position).is_none(),
                "{text}:{character}"
            );
        }
    }

    #[tokio::test]
    async fn literal_browsing_batches_lookup_and_reuses_cached_macro_source() {
        // Guards one full document-key lookup and a fresh budget per literal,
        // or a second macro/written parse merely to prove a browsing relation.
        let root = Url::parse("file:///C:/dygnosis-source-batch/main.mod").unwrap();
        let child = Url::parse("file:///C:/dygnosis-source-batch/dormant.inc").unwrap();
        let text = format!(
            "var y;model;y=0;end;\n@#if 0\n{}@#endif\n",
            "@#include \"dormant.inc\"\n".repeat(100)
        );
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.clone(), 1);
        let mut inner = server.lock_inner();
        inner
            .workspace
            .update_document(child.as_str(), "@#error \"dormant\"\n");
        for id in 0..100 {
            inner.workspace.update_document(
                &format!("file:///C:/dygnosis-source-batch/file{id}.inc"),
                "// supplied sibling\n",
            );
        }
        inner.workspace.get_effective_model(root.as_str()).unwrap();
        let inventory = inner.workspace.source_inventory(root.as_str()).unwrap();
        let parses =
            crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
        crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.set(0));
        assert_eq!(
            literal_links(&mut inner, &root, &text, &LineIndex::new(&text)).len(),
            100
        );
        assert_eq!(
            crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.get()),
            1
        );
        assert!(Arc::ptr_eq(
            &inventory,
            &inner.workspace.source_inventory(root.as_str()).unwrap()
        ));
        crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.set(0));
        assert!(inner.source_browsing_relation(&root, &child));
        assert_eq!(
            crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.get()),
            1
        );
        assert_eq!(
            crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
            parses
        );
        assert!(
            inner
                .workspace
                .cached_source_inventory(child.as_str())
                .is_none()
        );
    }

    fn selected_preview(server: &Backend, root: &Url) {
        let mut inner = server.lock_inner();
        let source = inner
            .workspace
            .source_structure(root.as_str(), &SourceStructureQuery::default())
            .unwrap();
        let target = source
            .files
            .iter()
            .flat_map(|file| file.sites.iter())
            .find(|site| {
                site.kind == crate::engine::analysis_configuration::DirectiveKind::Definition
            })
            .unwrap()
            .target
            .clone();
        inner.workspace.set_root_analysis_configuration(root.as_str(),serde_json::from_value(json!({"name":"Alternative","overrides":[{"kind":"definition","target":target,"expression":"1"}]})).unwrap()).unwrap();
        inner.analysis_configuration_committed(root);
        inner.refresh_diagnostics(Some(root));
    }
    fn action_params(root: &Url) -> CodeActionParams {
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            range: Range::new(Position::new(0, 0), Position::new(1000, 0)),
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only: Some(vec![CodeActionKind::QUICKFIX]),
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn preview_disables_stored_fixes_and_metadata_actions_without_edits() {
        // Both stored E001 repairs and I208/I209 metadata actions must stay read-only in previews.
        let root = Url::parse("file:///C:/dygnosis-analysis-actions/main.mod").unwrap();
        for body in [
            "var y;\nmodel;\ny=0;\nend;\n",
            "var y;\nmodel;\ny=0\nend;\n",
        ] {
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), format!("@#define FLAG=0\n{body}"), 1);
            selected_preview(server, &root);
            let actions = server.quick_fixes(&action_params(&root)).unwrap();
            assert!(!actions.is_empty());
            for action in actions {
                let CodeActionOrCommand::CodeAction(action) = action else {
                    panic!("no executable command in preview")
                };
                assert!(action.edit.is_none());
                assert!(action.command.is_none());
                assert!(
                    action
                        .disabled
                        .unwrap()
                        .reason
                        .contains("ANALYSIS_PREVIEW_EDIT_UNSUPPORTED")
                );
            }
        }
    }

    #[tokio::test]
    async fn semantic_hover_names_the_selected_configuration_and_preview_rename_has_code() {
        // A semantic hover must not display facts under Source after a configured selection.
        let root = Url::parse("file:///C:/dygnosis-analysis-hover/main.mod").unwrap();
        let text =
            "@#define FLAG=0\nvar y;\nparameters rho;\nrho=0.5;\nmodel;\ny=rho*y(-1);\nend;\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        selected_preview(server, &root);
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: Position::new(5, 3),
        };
        let hover = server.hover_at(&pos).unwrap();
        let value = serde_json::to_value(hover).unwrap().to_string();
        assert!(value.contains("Analysis: Alternative (preview)"), "{value}");
        let response=server.execute("dynare/captureRename",&[json!({"textDocument":{"uri":root},"position":{"line":5,"character":3},"newName":"persistence"})]);
        assert_eq!(response["code"], "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED");
        assert!(response.get("edit").is_none());
    }

    #[tokio::test]
    async fn semantic_hover_omits_automatic_analysis_names() {
        // Guards generic preview labels hiding the useful symbol facts in every hover.
        let root = Url::parse("file:///C:/dygnosis-analysis-hover/main.mod").unwrap();
        let text =
            "@#define FLAG=0\nvar y;\nparameters rho;\nrho=0.5;\nmodel;\ny=rho*y(-1);\nend;\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        selected_preview(server, &root);
        for name in [
            None,
            Some("Analysis preview"),
            Some("Analysis preview (modified)"),
            Some("Macro inputs"),
            Some("Macro inputs (modified)"),
        ] {
            {
                let mut inner = server.lock_inner();
                let mut choices =
                    (*inner.workspace.root_analysis_configuration(root.as_str())).clone();
                choices.name = name.map(str::to_owned);
                inner
                    .workspace
                    .set_root_analysis_configuration(root.as_str(), choices)
                    .unwrap();
                inner.analysis_configuration_committed(&root);
            }
            let hover = server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root.clone() },
                    position: Position::new(5, 3),
                })
                .unwrap();
            let value = serde_json::to_value(hover).unwrap().to_string();
            assert!(value.contains("rho"), "{value}");
            assert!(!value.contains("Analysis:"), "{value}");
        }
    }

    #[tokio::test]
    async fn conditional_hover_reports_each_arm_under_an_else_preview() {
        // Guards the opener reporting directive execution and the selected else having no activity.
        let root = Url::parse("file:///C:/dygnosis-branch-hover/main.mod").unwrap();
        let text = "@#define FLAG=1\n@#if FLAG\nvar a;\n@#else\nvar b;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        {
            let mut inner = server.lock_inner();
            let source = inner
                .workspace
                .source_structure(root.as_str(), &SourceStructureQuery::default())
                .unwrap();
            let site = source.files[0]
                .sites
                .iter()
                .find(|site| {
                    site.kind == crate::engine::analysis_configuration::DirectiveKind::Conditional
                })
                .unwrap();
            let choices = serde_json::from_value(json!({"overrides":[{
                "kind":"branch","target":site.target,"branch_id":site.branches[1].branch_id
            }]}))
            .unwrap();
            inner
                .workspace
                .set_root_analysis_configuration(root.as_str(), choices)
                .unwrap();
            inner.analysis_configuration_committed(&root);
            let projected = file(&mut inner, &root).unwrap();
            let states: Vec<_> = projected
                .inventory
                .items
                .iter()
                .filter(|item| {
                    matches!(
                        item.kind,
                        SourceItemKind::Conditional | SourceItemKind::Branch
                    )
                })
                .map(|item| {
                    projected
                        .availability
                        .iter()
                        .find(|row| row.source_id == item.source_id)
                        .unwrap()
                        .control_availability
                })
                .collect();
            assert_eq!(
                states,
                [SourceAvailability::Inactive, SourceAvailability::Active]
            );
        }
        let mut actual = Vec::new();
        for (line, character) in [(1, 3), (1, 6), (3, 3)] {
            let hover = server.hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root.clone() },
                position: Position::new(line, character),
            });
            actual.push(hover.map(|hover| match hover.contents {
                HoverContents::Markup(markup) => {
                    markup.value.lines().next().unwrap_or_default().to_owned()
                }
                _ => panic!("markdown hover"),
            }));
        }
        assert!(
            actual[0]
                .as_ref()
                .is_some_and(|text| text.ends_with(" · inactive"))
                && actual[1]
                    .as_ref()
                    .is_some_and(|text| text.ends_with(" · inactive"))
                && actual[2]
                    .as_ref()
                    .is_some_and(|text| text.ends_with(" · active")),
            "if / else hovers: {actual:?}"
        );
    }

    #[tokio::test]
    async fn conditional_hover_tracks_source_and_each_preview_without_reanalysis() {
        // Guards stale arm labels after forcing, skipping or clearing a branch, and repeated parsing for hover.
        use crate::engine::analysis_configuration::{AnalysisConfiguration, AnalysisOverride};
        use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
        for (opener, newline) in ["@#if FLAG", "@#ifdef FLAG", "@#ifndef FLAG"]
            .into_iter()
            .flat_map(|opener| ["\n", "\r\n"].map(move |newline| (opener, newline)))
        {
            let root = Url::parse("file:///C:/dygnosis-branch-hover/main.mod").unwrap();
            let text = format!(
                "@#define FLAG=1\n{opener}\nvar a;\n@#elseif 0\nvar b;\n@#else\nvar c;\n@#endif\n"
            )
            .replace('\n', newline);
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text, 1);
            let (target, branches) = {
                let mut inner = server.lock_inner();
                let source = inner
                    .workspace
                    .source_structure(root.as_str(), &SourceStructureQuery::default())
                    .unwrap();
                let site = source.files[0]
                    .sites
                    .iter()
                    .find(|site| {
                        site.kind
                            == crate::engine::analysis_configuration::DirectiveKind::Conditional
                    })
                    .unwrap();
                (
                    site.target.clone(),
                    site.branches
                        .iter()
                        .map(|branch| branch.branch_id.clone())
                        .collect::<Vec<_>>(),
                )
            };
            let positions = [
                (1, 3, 0),
                (1, opener.find("FLAG").unwrap() as u32, 0),
                (3, 3, 1),
                (3, 9, 1),
                (5, 3, 2),
            ];
            let hover_at = |line, character| {
                let hover = server
                    .hover_at(&TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: root.clone() },
                        position: Position::new(line, character),
                    })
                    .unwrap();
                let HoverContents::Markup(markup) = hover.contents else {
                    panic!("markdown hover")
                };
                markup.value.lines().next().unwrap_or_default().to_owned()
            };
            for selected in [
                None,
                Some(Some(0)),
                Some(Some(1)),
                Some(Some(2)),
                Some(None),
                None,
            ] {
                let mut choices = AnalysisConfiguration::default();
                if let Some(index) = selected {
                    choices.overrides.push(AnalysisOverride::Branch {
                        target: target.clone(),
                        branch_id: index.map(|index| branches[index].clone()),
                    });
                }
                {
                    let mut inner = server.lock_inner();
                    inner
                        .workspace
                        .set_root_analysis_configuration(root.as_str(), choices)
                        .unwrap();
                    inner.analysis_configuration_committed(&root);
                }
                hover_at(1, 3);
                let before = PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
                let active =
                    selected.unwrap_or(Some(if opener == "@#ifndef FLAG" { 2 } else { 0 }));
                for (line, character, arm) in positions {
                    let actual = hover_at(line, character);
                    let expected = if active == Some(arm) {
                        "active"
                    } else {
                        "inactive"
                    };
                    assert!(
                        actual.ends_with(&format!(" · {expected}")),
                        "{opener} / {selected:?} / line {line}: {actual}"
                    );
                }
                assert_eq!(
                    PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
                    before,
                    "current branch hovers must reuse the selected analysis"
                );
            }
        }
    }

    #[tokio::test]
    async fn activity_hover_omits_source_copies_and_keeps_unknown_causes() {
        // Guards copying a visible statement into its hover or hiding why activity is unknown.
        for (condition, state) in [("0", "inactive"), ("MISSING", "unknown")] {
            let root = Url::parse("file:///C:/dygnosis-activity-hover/main.mod").unwrap();
            let text = format!("@#if {condition}\nvar hidden;\n@#endif\nvar y;model;y=0;end;\n");
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text.clone(), 1);
            let mut inner = server.lock_inner();
            let projected = file(&mut inner, &root).unwrap();
            let source_id = &projected
                .inventory
                .items
                .iter()
                .find(|item| item.name.as_deref() == Some("hidden"))
                .unwrap()
                .source_id;
            let cause = projected
                .availability
                .iter()
                .find(|row| &row.source_id == source_id)
                .and_then(|row| row.cause.as_ref())
                .map(|cause| cause.message.clone());
            let hover = hover(&mut inner, &root, text.find("hidden").unwrap() as u32).unwrap();
            let HoverContents::Markup(markup) = hover.contents else {
                panic!("markdown hover")
            };
            assert!(
                markup
                    .value
                    .starts_with("**Written endogenous variable**: `hidden`"),
                "{}",
                markup.value
            );
            assert!(!markup.value.contains("Written:"));
            assert!(!markup.value.contains("var hidden;"));
            assert!(
                !markup
                    .value
                    .contains("Model values, timing and equation numbers")
            );
            if state == "inactive" {
                assert_eq!(markup.value, "**Written endogenous variable**: `hidden`");
            } else {
                assert!(
                    markup
                        .value
                        .contains(cause.as_deref().expect("unknown cause")),
                    "unknown activity retains its cause: {}",
                    markup.value
                );
            }
        }
    }

    #[tokio::test]
    async fn dormant_custom_extension_include_link_resolves_without_loading_its_body() {
        // Literal Open Source must use selected-root lookup and typed metadata, without dormant execution.
        struct Temporary(std::path::PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                if let (Ok(parent), Ok(target)) =
                    (std::env::temp_dir().canonicalize(), self.0.canonicalize())
                    && target.parent() == Some(parent.as_path())
                    && target
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("dygnosis-source-link-"))
                {
                    let _ = std::fs::remove_dir_all(target);
                }
            }
        }
        let directory = Temporary(std::env::temp_dir().join(format!(
                "dygnosis-source-link-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir_all(&directory.0).unwrap();
        let child_path = directory.0.join("dormant.txt");
        std::fs::write(&child_path, "@#error \"This body must not execute\"\n").unwrap();
        let root = Url::from_file_path(directory.0.join("main.mod")).unwrap();
        // `temp_dir()` may use a symlinked path (for example /var on macOS),
        // while the document link uses the canonical filesystem path.
        let child = Url::from_file_path(child_path.canonicalize().unwrap()).unwrap();
        let text =
            "var y;model;y=0;end;\n@#if 0\n@#include \"dormant.txt\"\n@#include UNKNOWN\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        assert!(
            server
                .lock_inner()
                .workspace
                .get_source(child.as_str())
                .is_none()
        );
        let links = server.document_links(&root).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target.as_ref(), Some(&child));
        assert_eq!(
            links[0].data.as_ref().unwrap()["dygnosis_source_include"],
            true
        );
        assert_eq!(links[0].data.as_ref().unwrap()["root_uri"], root.as_str());
        let mut inner = server.lock_inner();
        assert!(inner.workspace.get_source(child.as_str()).is_none());
        assert!(inner.workspace.owner_roots(child.as_str()).is_empty());
        assert!(inner.workspace.includes_complete(root.as_str()));
    }
}
