//! Real LSP consumers of the shared syntax records and written macro targets.
use dygnosis::server::new_service;
use dygnosis::span::LineIndex;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

fn position(text: &str, byte: usize) -> Position {
    let position = LineIndex::new(text).position_utf16(text, byte as u32);
    Position::new(position.line, position.character)
}

async fn completions(marked: &str) -> Vec<String> {
    let byte = marked.find('|').unwrap();
    let text = marked.replacen('|', "", 1);
    let uri = Url::parse("file:///C:/syntax-intel/model.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.clone(),
            },
        })
        .await;
    match service
        .inner()
        .completion(CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position(&text, byte),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
    {
        Some(CompletionResponse::Array(items)) => {
            items.into_iter().map(|item| item.label).collect()
        }
        Some(CompletionResponse::List(list)) => {
            list.items.into_iter().map(|item| item.label).collect()
        }
        None => Vec::new(),
    }
}

#[tokio::test]
async fn completion_keeps_model_macro_option_and_native_contexts_separate() {
    // Guards a small hard-coded builtin list, macro/model mixing and option keys
    // displacing the symbols or finite values required at an argument position.
    let model = completions("var y; parameters p; model; y=|; end;").await;
    for name in [
        "y",
        "p",
        "diff",
        "log10",
        "atanh",
        "erfc",
        "STEADY_STATE",
        "SUM",
    ] {
        assert!(
            model.iter().any(|item| item == name),
            "missing {name}: {model:?}"
        );
    }
    assert!(!model.iter().any(|item| item == "floor"));
    let macros = completions("@#define transform(x) = x\nvar y; model; y=@{|}; end;").await;
    for name in ["floor", "length", "transform"] {
        assert!(macros.iter().any(|item| item == name));
    }
    assert!(!macros.iter().any(|item| item == "diff" || item == "y"));
    let values = completions("var y; parameters p; model; y=p; end; stoch_simul(dr=|);").await;
    assert!(values.iter().any(|item| item == "cycle_reduction"));
    assert!(!values.iter().any(|item| item == "irf"));
    assert!(completions("disp lo|g(y)\n").await.is_empty());
    assert!(completions("// lo|g(y)\n").await.is_empty());
    // Bare `std` is a reserved INITIAL lexer head in Dynare 7.2. An unknown
    // assignment head instead hands the whole line to native MATLAB.
    let native_std = completions("out=st|d(y);\n").await;
    assert!(
        native_std.is_empty(),
        "native std completions: {native_std:?}"
    );
    for source in ["initval; y=|; end;", "steady_state_model; y=|; end;"] {
        let ordinary = completions(source).await;
        assert!(ordinary.iter().any(|item| item == "log"));
        assert!(
            !ordinary
                .iter()
                .any(|item| matches!(item.as_str(), "diff" | "EXPECTATION" | "STEADY_STATE"))
        );
    }
    assert!(
        completions("pac_m|")
            .await
            .iter()
            .any(|item| item == "pac_model")
    );
    assert!(
        completions("var y; planner_objective log(|y);")
            .await
            .iter()
            .any(|item| item == "diff")
    );
}

#[tokio::test]
async fn included_expression_uses_its_proven_model_context() {
    // Guards treating an owned equation fragment as standalone native MATLAB.
    let root = Url::parse("file:///C:/syntax-intel/include.mod").unwrap();
    let child = Url::parse("file:///C:/syntax-intel/body.inc").unwrap();
    let text = "y=log(y);\n// log(y)\n";
    let (service, _socket) = new_service();
    for (uri, source) in [
        (child.clone(), text),
        (root, "var y; model;\n@#include \"body.inc\"\nend;"),
    ] {
        service
            .inner()
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "dynare".into(),
                    version: 1,
                    text: source.into(),
                },
            })
            .await;
    }
    let target = |byte| TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: child.clone() },
        position: position(text, byte),
    };
    let Some(CompletionResponse::Array(items)) = service
        .inner()
        .completion(CompletionParams {
            text_document_position: target(6),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
    else {
        panic!("include completions");
    };
    assert!(items.iter().any(|item| item.label == "diff"));
    assert!(
        service
            .inner()
            .signature_help(SignatureHelpParams {
                text_document_position_params: target(6),
                work_done_progress_params: Default::default(),
                context: None,
            })
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        service
            .inner()
            .hover(HoverParams {
                text_document_position_params: target(3),
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .is_some()
    );
    let quiet = service
        .inner()
        .completion(CompletionParams {
            text_document_position: target(text.rfind("log").unwrap() + 1),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap();
    assert!(matches!(quiet, Some(CompletionResponse::Array(items)) if items.is_empty()));
}

#[tokio::test]
async fn highlights_and_linked_editing_use_the_selected_binding_only() {
    // Guards same-text native/model-name tokens entering a source edit or highlight.
    let text = "var y; model; [name='row'] y=log(y(-1)); end;\nvar_model(model_name=y,eqtags=['row']);\ndisp(y);\n";
    let uri = Url::parse("file:///C:/syntax-intel/selected.mod").unwrap();
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
    let target = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: position(text, 4),
    };
    let highlights = service
        .inner()
        .document_highlight(DocumentHighlightParams {
            text_document_position_params: target.clone(),
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    let selected: Vec<_> = [4, text.find("y=log").unwrap(), text.find("y(-1)").unwrap()]
        .into_iter()
        .map(|byte| position(text, byte))
        .collect();
    assert_eq!(highlights.len(), selected.len());
    assert!(
        highlights
            .iter()
            .all(|highlight| selected.contains(&highlight.range.start))
    );
    let linked = service
        .inner()
        .linked_editing_range(LinkedEditingRangeParams {
            text_document_position_params: target,
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(linked.ranges.len(), selected.len());
    assert!(
        linked
            .ranges
            .iter()
            .all(|range| selected.contains(&range.start))
    );
}

#[tokio::test]
async fn functions_have_hover_and_signature_help_in_their_language() {
    // Guards missing special/operator/macro help and dropped alternate arities.
    let text = "@#define t = floor(2.5)\nvar y; model; y=normcdf(y,0,1)+STEADY_STATE(y); end;";
    let uri = Url::parse("file:///C:/syntax-intel/help.mod").unwrap();
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
    for word in ["define", "floor", "normcdf", "STEADY_STATE"] {
        let byte = text.find(word).unwrap();
        let target = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: position(text, byte),
        };
        assert!(
            service
                .inner()
                .hover(HoverParams {
                    text_document_position_params: target,
                    work_done_progress_params: Default::default(),
                })
                .await
                .unwrap()
                .is_some(),
            "{word} hover"
        );
    }
    for (needle, expected) in [("floor(", 1usize), ("normcdf(y,0,", 3)] {
        let byte = text.find(needle).unwrap() + needle.len();
        let help = service
            .inner()
            .signature_help(SignatureHelpParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: position(text, byte),
                },
                work_done_progress_params: Default::default(),
                context: None,
            })
            .await
            .unwrap()
            .expect("function signature");
        assert!(
            help.signatures.iter().any(|signature| signature
                .parameters
                .as_ref()
                .is_some_and(|parameters| parameters.len() == expected)),
            "{needle}: {help:?}"
        );
    }
}

#[tokio::test]
async fn outline_selects_written_macro_names_without_adding_expression_leaves() {
    // Guards inaccessible macro definitions and a duplicate tree for every call.
    let text =
        "@#define transform(x) = log(x)\n@#define value = 1\nvar y; model; y=log(y(-1)); end;";
    let uri = Url::parse("file:///C:/syntax-intel/outline.mod").unwrap();
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
    let Some(DocumentSymbolResponse::Nested(rows)) = service
        .inner()
        .document_symbol(DocumentSymbolParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
    else {
        panic!("nested Outline");
    };
    for (name, kind) in [
        ("transform", SymbolKind::FUNCTION),
        ("value", SymbolKind::CONSTANT),
    ] {
        let row = rows
            .iter()
            .find(|row| row.name == name)
            .expect("macro target");
        assert_eq!(row.kind, kind);
        assert_eq!(
            row.selection_range.start,
            position(text, text.find(name).unwrap())
        );
        assert_eq!(
            row.selection_range.end.character - row.selection_range.start.character,
            name.len() as u32
        );
    }
    let model = rows
        .iter()
        .find(|row| row.name == "model")
        .expect("model block");
    assert_eq!(model.children.as_ref().map(Vec::len), Some(1));
}

async fn function_signature(marked: &str) -> SignatureHelp {
    let byte = marked.find('|').unwrap();
    let text = marked.replacen('|', "", 1);
    let uri = Url::parse("file:///C:/syntax-intel/balanced-signature.mod").unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.clone(),
            },
        })
        .await;
    service
        .inner()
        .signature_help(SignatureHelpParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: position(&text, byte),
            },
            work_done_progress_params: Default::default(),
            context: None,
        })
        .await
        .unwrap()
        .expect("function signature")
}

// Guards array/group delimiters leaking commas into their enclosing call,
// losing the nearest function inside an array, or clearing it at an array row.
#[tokio::test]
async fn function_signatures_balance_array_groups_and_nested_calls() {
    for (source, function, argument) in [
        ("@#define v = length([1,2|])\n", "length", 0),
        ("@#define v = length([1;2|])\n", "length", 0),
        ("@#define v = max([1,2],[3,4|])\n", "max", 1),
        ("@#define v = max([1,2],[3,4]|)\n", "max", 1),
        ("@#define v = max([1,2],length([3,4|]))\n", "length", 0),
        ("@#define v = max([1,2],(length([3,4])+5|))\n", "max", 1),
        ("@#define v = length([max(1,2),max(3,4)|])\n", "length", 0),
        ("var y;model;y=EXPECTATION(|-1)(y);end;", "expectation", 0),
        ("var y;model;y=EXPECTATION(-1)(y|);end;", "expectation", 0),
    ] {
        let help = function_signature(source).await;
        let signature = &help.signatures[help.active_signature.unwrap() as usize];
        assert!(
            signature.label.to_ascii_lowercase().starts_with(function),
            "{source}: {help:?}"
        );
        assert_eq!(help.active_parameter, Some(argument), "{source}: {help:?}");
        assert_eq!(signature.active_parameter, Some(argument));
        assert!((argument as usize) < signature.parameters.as_ref().unwrap().len());
    }
}

// Guards the top-level active parameter remaining out of range when no
// advertised arity accepts it, while preserving every alternate signature.
#[tokio::test]
async fn function_signature_active_parameters_respect_each_arity() {
    let alternate = function_signature("var y;model;y=normcdf(y,0,|1);end;").await;
    assert!(alternate.signatures.iter().any(|signature| {
        signature
            .parameters
            .as_ref()
            .is_some_and(|parameters| parameters.len() == 1)
    }));
    assert!(alternate.signatures.iter().any(|signature| {
        signature
            .parameters
            .as_ref()
            .is_some_and(|parameters| parameters.len() == 3)
    }));
    assert_eq!(alternate.active_parameter, Some(2));
    for signature in &alternate.signatures {
        if let Some(active) = signature.active_parameter {
            assert!((active as usize) < signature.parameters.as_ref().unwrap().len());
        }
    }
    let extra = function_signature("@#define v = length([1,2],|3)\n").await;
    assert!(extra.active_parameter.is_none());
    assert!(
        extra
            .signatures
            .iter()
            .all(|signature| signature.active_parameter.is_none())
    );
}
