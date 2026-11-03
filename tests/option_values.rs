//! Option value help follows written option ownership in both activity states.
use dygnosis::server::new_service;
use dygnosis::span::LineIndex;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn document(marked: &str) -> (String, Position) {
    let byte = marked.find('|').unwrap();
    let text = marked.replacen('|', "", 1);
    let pos = LineIndex::new(&text).position_utf16(&text, byte as u32);
    (text, Position::new(pos.line, pos.character))
}

async fn hover(marked: &str) -> Option<Hover> {
    let (text, position) = document(marked);
    let uri = Url::parse("file:///C:/option-values/model.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text,
            },
        })
        .await;
    service
        .inner()
        .hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
}

async fn complete(marked: &str) -> Vec<CompletionItem> {
    let (text, position) = document(marked);
    let uri = Url::parse("file:///C:/option-values/model.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text,
            },
        })
        .await;
    match service
        .inner()
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
    {
        Some(CompletionResponse::Array(items)) => items,
        Some(CompletionResponse::List(list)) => list.items,
        None => vec![],
    }
}

fn markdown(hover: Hover) -> String {
    match hover.contents {
        HoverContents::Markup(value) => value.value,
        other => panic!("Expected Markdown, got {other:?}"),
    }
}

#[tokio::test]
async fn fixed_literals_keep_explanations_without_executed_model_facts() {
    // Guards incorrect fallback help and a hover range that includes the option key.
    for (source, meaning) in [
        ("pac_model(kind=d|l);", "levels"),
        ("stoch_simul(graph_format=(eps,p|df));", "PDF"),
        ("steady(solve_algo=|1);", "line search"),
        (
            "estimation(posterior_sampling_method='random_walk_|metropolis_hastings');",
            "Metropolis",
        ),
    ] {
        for text in [source.to_owned(), format!("@#if 0\n{source}\n@#endif\n")] {
            let value_hover = hover(&text)
                .await
                .unwrap_or_else(|| panic!("No hover: {text}"));
            if source.starts_with("pac_model") {
                let (_, cursor) = document(&text);
                assert_eq!(
                    value_hover.range,
                    Some(Range::new(
                        Position::new(cursor.line, cursor.character - 1),
                        Position::new(cursor.line, cursor.character + 1),
                    ))
                );
            }
            let result = markdown(value_hover);
            assert!(result.contains(meaning), "{text}: {result}");
            assert!(!result.contains("source is inactive"), "{result}");
        }
    }
}

#[tokio::test]
async fn completion_explains_values_and_handles_incomplete_slots() {
    // Guards missing helper-derived choices, incomplete header recovery and missing documentation.
    for text in [
        "pac_model(kind=|);",
        "pac_model(kind=|",
        "@#if 0\npac_model(kind=|);\n@#endif",
    ] {
        let items = complete(text).await;
        let dl = items
            .iter()
            .find(|item| item.label == "dl")
            .unwrap_or_else(|| panic!("No dl: {text}"));
        assert_eq!(dl.kind, Some(CompletionItemKind::ENUM_MEMBER));
        assert!(format!("{:?}", dl.documentation).contains("levels"));
    }
    let items = complete("stoch_simul(graph_format=(eps,|));").await;
    assert!(items.iter().any(|item| item.label == "pdf"));
}

#[tokio::test]
async fn literal_context_does_not_leak_into_expressions_or_wrong_forms() {
    // Guards enum help on expression operands, invalid quoting and unrelated native text.
    for text in [
        "steady(solve_algo=|1+0);",
        "pac_model(kind='d|l');",
        "stoch_simul(irf=foo(p|df));",
        "verbatim;\npac_model(kind=d|l);\nend;",
        "// pac_model(kind=d|l);",
        "steady(solve_algo=|1.0);",
        "steady(solve_algo=+|1);",
        "markov_switching(chain=1,number_of_regimes=2,duration=i|nf);",
        "markov_switching(chain=1,number_of_regimes=2,duration=[+|]);",
        "method_of_moments(weighting_matrix='|DIAGONAL');",
    ] {
        assert!(hover(text).await.is_none(), "{text}");
    }
}

#[tokio::test]
async fn quoted_completion_replaces_one_literal_and_preserves_open_names() {
    // Guards losing an unfinished quoted prefix or adding a second pair of quotes.
    for source in [
        "estimation(posterior_sampling_method='random_|walk_metropolis_hastings');",
        "estimation(posterior_sampling_method='random_|",
    ] {
        let items = complete(source).await;
        let choice = items
            .iter()
            .find(|item| item.label == "random_walk_metropolis_hastings")
            .unwrap_or_else(|| panic!("{source}"));
        let Some(CompletionTextEdit::Edit(edit)) = &choice.text_edit else {
            panic!("Missing literal edit");
        };
        assert_eq!(edit.new_text, "'random_walk_metropolis_hastings'");
        let (text, _) = document(source);
        let index = LineIndex::new(&text);
        let start = index.offset_utf16(
            &text,
            dygnosis::span::Position {
                line: edit.range.start.line,
                character: edit.range.start.character,
            },
        );
        let end = index.offset_utf16(
            &text,
            dygnosis::span::Position {
                line: edit.range.end.line,
                character: edit.range.end.character,
            },
        );
        assert!(text[start as usize..end as usize].starts_with('\''));
    }
    let source = "parameters dl; dl=.99; pac_model(discount=d|l);";
    let text = markdown(hover(source).await.unwrap());
    assert!(text.to_lowercase().contains("parameter"), "{text}");
    assert!(!text.contains("kind = dl"));
}

#[tokio::test]
async fn source_ownership_stays_literal_across_branches_comments_and_nested_calls() {
    // Guards a fixed value being inferred through a generated option or a different macro path.
    for text in [
        "@#define KEY=\"kind\"\npac_model(@{KEY}=d|l);",
        "pac_model(\n@#if 1\nkind=\n@#else\nkind=\n@#endif\nd|l);",
    ] {
        if let Some(value) = hover(text).await {
            assert!(!markdown(value).contains("target appears"), "{text}");
        }
    }
    for text in [
        "pac_model(kind=/* co|mment */dl);",
        "pac_model(kind=// co|mment\ndl);",
    ] {
        assert!(
            !complete(text)
                .await
                .iter()
                .any(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            "{text}"
        );
    }
    let signed = markdown(
        hover("identification(analytic_derivation_mode=-|1);")
            .await
            .unwrap(),
    );
    assert!(signed.contains("analytic_derivation_mode = -1"), "{signed}");
    let list = complete("method_of_moments(weighting_matrix=['DI|AGONAL']);").await;
    assert!(
        list.iter()
            .any(|item| item.label.eq_ignore_ascii_case("DIAGONAL"))
    );
    let infinity = markdown(
        hover("markov_switching(chain=1,number_of_regimes=2,duration=[-i|nf]);")
            .await
            .unwrap(),
    );
    assert!(
        infinity.to_lowercase().contains("negative infinity"),
        "{infinity}"
    );
    for text in [
        "markov_switching(chain=1,number_of_regimes=2,duration=[inf - i|nf]);",
        "identification(analytic_derivation_mode=- |1);",
        "parameters p; p.prior(shape=b|eta);",
        "var y c; corr(y,c).prior(shape=b|eta);",
    ] {
        assert!(hover(text).await.is_some(), "{text}");
    }
    let spaced =
        complete("method_of_moments(weighting_matrix=['DIAGONAL' '|IDENTITY_MATRIX']);").await;
    assert!(spaced.iter().any(|item| item.label == "IDENTITY_MATRIX"));
    // A whitespace position after a completed literal must not receive an edit that excludes the cursor.
    assert!(
        !complete("pac_model(kind=dl |);")
            .await
            .iter()
            .any(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
    );
}

#[tokio::test]
async fn open_option_symbols_keep_definition_and_rename() {
    // Guards fixed-value completion taking ownership of an ordinary option operand.
    let text = "parameters beta; beta=.99; var y; model; [name='aux'] y=0; end;\n\
                var_model(model_name=vm,eqtags=['aux']);\n\
                pac_model(model_name=pm,auxiliary_model_name=vm,kind=dl,discount=beta);";
    let uri = Url::parse("file:///C:/option-values/names.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.into(),
            },
        })
        .await;
    let index = LineIndex::new(text);
    let byte = text.rfind("beta").unwrap() as u32 + 1;
    let pos = index.position_utf16(text, byte);
    let target = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position::new(pos.line, pos.character),
    };
    let definition = service
        .inner()
        .goto_definition(GotoDefinitionParams {
            text_document_position_params: target.clone(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap();
    assert!(definition.is_some());
    let edit = service
        .inner()
        .rename(RenameParams {
            text_document_position: target,
            new_name: "betta".into(),
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .expect("ordinary option operand remains editable");
    let Some(DocumentChanges::Edits(changes)) = edit.document_changes else {
        panic!("ordinary rename returns versioned text edits");
    };
    let edits = changes
        .iter()
        .flat_map(|change| &change.edits)
        .collect::<Vec<_>>();
    assert_eq!(edits.len(), 3, "{edits:?}");
    let choice_byte = text.find("kind=dl").unwrap() + "kind=".len();
    let choice = index.position_utf16(text, choice_byte as u32);
    assert!(edits.iter().all(
        |edit| matches!(edit, OneOf::Left(edit) if edit.new_text == "betta"
                && edit.range.start != Position::new(choice.line, choice.character))
    ));
}
