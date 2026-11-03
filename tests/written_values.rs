//! Fixed syntax values retain their written role in active and inactive source.
use dygnosis::server::new_service;
use dygnosis::span::LineIndex;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

async fn answers(marked: &str) -> (Option<Hover>, Vec<CompletionItem>) {
    let byte = marked.find('|').unwrap();
    let text = marked.replacen('|', "", 1);
    let at = LineIndex::new(&text).position_utf16(&text, byte as u32);
    let uri = Url::parse("file:///C:/written-values/model.mod").unwrap();
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
    let position = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position: Position::new(at.line, at.character),
    };
    let hover = service
        .inner()
        .hover(HoverParams {
            text_document_position_params: position.clone(),
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap();
    let items = service
        .inner()
        .completion(CompletionParams {
            text_document_position: position,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        })
        .await
        .unwrap();
    (
        hover,
        match items {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => vec![],
        },
    )
}

fn markdown(hover: &Hover) -> &str {
    match &hover.contents {
        HoverContents::Markup(value) => &value.value,
        _ => panic!("Expected Markdown"),
    }
}

#[tokio::test]
async fn fixed_roles_have_the_same_meaning_and_range_in_both_activity_states() {
    // Guards declaration/operator fallback and missing inactive syntax help.
    for (source, label) in [
        ("parameters p; change_type(v|ar) p;", "var"),
        ("moment_calibration; y,c,|+; end;", "+"),
        ("irf_calibration; y(1:4),e,|-; end;", "-"),
        ("var y; model; [sta|tic] y=0; [dynamic] y=0; end;", "static"),
        ("@#define x=tr|ue\n", "true"),
        (
            "@#for x\tin\t[tr|ue,false]\n@#define z=1\n@#endfor\n",
            "true",
        ),
        ("var_expectation_model(horizon=1:I|nf);", "inf"),
    ] {
        let (active, items) = answers(source).await;
        let active = active.unwrap_or_else(|| panic!("No hover: {source}"));
        let (inactive, _) = answers(&format!("@#if 0\n{source}\n@#endif\n")).await;
        let inactive = inactive.unwrap_or_else(|| panic!("No inactive hover: {source}"));
        assert_eq!(markdown(&active), markdown(&inactive), "{source}");
        let a = active.range.unwrap();
        let b = inactive.range.unwrap();
        assert_eq!(a.start.character, b.start.character, "{source}");
        assert_eq!(a.end.character, b.end.character, "{source}");
        assert!(
            items
                .iter()
                .any(|item| item.label.eq_ignore_ascii_case(label)
                    && item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            "Missing {label}: {source}"
        );
    }
}

#[tokio::test]
async fn role_completion_preserves_surrounding_source() {
    // Guards replacing a lower bound, a tag opener or the whole type command.
    for (source, label, insertion) in [
        ("var_expectation_model(horizon=1:|);", "inf", "inf"),
        ("change_type(|) p;", "var", "var"),
        ("var y; model; [|] y=0; end;", "static", "static"),
        ("moment_calibration; y,c,|; end;", "+", "+"),
        ("@#define x=|\n", "true", "true"),
    ] {
        let (_, items) = answers(source).await;
        let item = items
            .iter()
            .find(|item| item.label.eq_ignore_ascii_case(label))
            .unwrap_or_else(|| panic!("No {label}: {source}"));
        let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
            panic!("No edit: {source}")
        };
        assert_eq!(edit.range.start, edit.range.end, "{source}");
        assert_eq!(edit.new_text, insertion, "{source}");
    }
}

#[tokio::test]
async fn roles_reject_other_operands_and_generated_contexts() {
    // Guards literal help leaking through comments, names, unary signs and macro joins.
    for source in [
        "var_expectation_model(horizon=I|nf);",
        "var_expectation_model(horizon=1:I|nf+1);",
        "moment_calibration; y,c,|+1; end;",
        "var y; model; [name='sta|tic'] y=0; end;",
        "var y; model; [/* com|ment */] y=0; end;",
        "@#define x=\"tr|ue\"\n",
        "@#define tr|ue=1\n",
        "// change_type(v|ar) p;",
        "verbatim;\nchange_type(v|ar) p;\nend;",
        "change_type(@{x} v|ar) p;",
    ] {
        let (hover, items) = answers(source).await;
        assert!(
            hover
                .as_ref()
                .is_none_or(|hover| !markdown(hover).contains("target_type")
                    && !markdown(hover).contains("boolean_literal")
                    && !markdown(hover).contains("interval")
                    && !markdown(hover).contains("horizon =")),
            "{source}: {hover:?}"
        );
        assert!(
            !items
                .iter()
                .any(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            "{source}"
        );
    }
}

#[tokio::test]
async fn nested_values_require_their_own_written_selector_for_completion() {
    // Guards borrowing a runtime default or a selector from another command/branch.
    for (source, label) in [
        (
            "estimation(mode_compute=4,optim=('NumgradAlgorithm',|2));",
            "2",
        ),
        (
            "method_of_moments(optim=('NumgradAlgorithm',|3),mode_compute=4);",
            "3",
        ),
        ("osr(opt_algo=04,optim=('NumgradAlgorithm',|5));", "5"),
        (
            "estimation(posterior_sampling_method='random_walk_metropolis_hastings',posterior_sampler_options=('proposal_distribution','rand_multivariate_st|udent'));",
            "rand_multivariate_student",
        ),
    ] {
        let (active, items) = answers(source).await;
        let active = active.unwrap_or_else(|| panic!("No nested hover: {source}"));
        assert!(
            !markdown(&active).contains("does not establish"),
            "{source}"
        );
        assert!(
            items
                .iter()
                .any(|item| item.label == label
                    && item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            "{source}"
        );
        let (inactive, items) = answers(&format!("@#if 0\n{source}\n@#endif\n")).await;
        assert_eq!(markdown(&active), markdown(&inactive.unwrap()), "{source}");
        assert!(
            items
                .iter()
                .any(|item| item.label == label
                    && item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            "{source}"
        );
    }
    for source in [
        "estimation(optim=('NumgradAlgorithm',|2));",
        "estimation(mode_compute=4,mode_compute=x,optim=('NumgradAlgorithm',|2));",
        "estimation(mode_compute=x,mode_compute=4,optim=('NumgradAlgorithm',|2));",
        "estimation(mode_compute=4); estimation(optim=('NumgradAlgorithm',|2));",
    ] {
        for source in [source.to_owned(), format!("@#if 0\n{source}\n@#endif\n")] {
            let (hover, items) = answers(&source).await;
            assert!(
                markdown(&hover.unwrap()).contains("does not establish"),
                "{source}"
            );
            assert!(
                !items
                    .iter()
                    .any(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
                "{source}"
            );
        }
    }
    let (hover, items) = answers("estimation(mode_compute=6,optim=('NumgradAlgorithm',|2));").await;
    assert!(markdown(&hover.unwrap()).contains("sets `mode_compute=6`"));
    assert!(
        !items
            .iter()
            .any(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
    );
}

#[tokio::test]
async fn tarb_optimizer_uses_the_sampler_pair_selector() {
    // The command's optimizer does not control the nested TaRB optimizer.
    let source = "estimation(mode_compute=5,posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('optim',('NumgradAlgorithm',|3),'mode_compute',4));";
    for source in [source.to_owned(), format!("@#if 0\n{source}\n@#endif\n")] {
        let (hover, items) = answers(&source).await;
        assert!(
            markdown(&hover.unwrap()).contains("posterior_sampler_options.optim.NumgradAlgorithm")
        );
        let item = items
            .iter()
            .find(|item| item.label == "3" && item.kind == Some(CompletionItemKind::ENUM_MEMBER))
            .unwrap();
        let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
            panic!("Expected a literal replacement")
        };
        assert_eq!(edit.new_text, "3");
        assert_eq!(edit.range.end.character - edit.range.start.character, 1);
    }
    let (_, items) = answers("estimation(mode_compute=4,posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',5,'optim',('NumgradAlgorithm',|3)));").await;
    assert!(
        !items
            .iter()
            .any(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
    );
}

#[tokio::test]
async fn nested_reference_distinguishes_optimizer_sequences_and_inheritance() {
    // The initial estimation zero suppresses later steps. The moments zero does not.
    for (source, expected) in [
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[3:5],optim=('NumgradAlgorithm',|3));",
            true,
        ),
        (
            "estimation(mode_compute=0,additional_optimizer_steps=[4],optim=('NumgradAlgorithm',|3));",
            false,
        ),
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[4],mh_posterior_mode_estimation,optim=('NumgradAlgorithm',|3));",
            false,
        ),
        (
            "method_of_moments(mode_compute=0,additional_optimizer_steps=[4],optim=('NumgradAlgorithm',|3));",
            true,
        ),
        (
            "estimation(mode_compute=5,posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',4),optim=('NumgradAlgorithm',|3));",
            true,
        ),
        (
            "estimation(mode_compute=5,posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',4,'optim',('TolFun',1e-6)),optim=('NumgradAlgorithm',|3));",
            false,
        ),
    ] {
        let (_, items) = answers(source).await;
        assert_eq!(items.iter().any(|item| item.label == "3" && item.kind == Some(CompletionItemKind::ENUM_MEMBER)), expected, "{source}");
    }
}

#[tokio::test]
async fn nested_completion_proves_selector_spelling_and_written_form() {
    // Guards false applicability from skipped punctuation, case, and flag syntax.
    for (source, label, expected) in [
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[3 /* bound */ : 5],optim=('NumgradAlgorithm',|3));",
            "3",
            true,
        ),
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[4:],optim=('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[:4],optim=('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[3::5],optim=('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(mode_compute=+4,optim=('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(mode_compute=2,additional_optimizer_steps=[,:4],optim=('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(mode_compute:=4,optim=('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(mode_compute=4,optim=:('NumgradAlgorithm',|3));",
            "3",
            false,
        ),
        (
            "estimation(posterior_sampling_method='slice',load_mh_file:,posterior_sampler_options=('use_mh_covariance_matrix',|1));",
            "1",
            false,
        ),
        (
            "estimation(posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',+:4,'optim',('NumgradAlgorithm',|3)));",
            "3",
            false,
        ),
        (
            "estimation(posterior_sampling_method='SLICE',posterior_sampler_options=('rotated',|1));",
            "1",
            false,
        ),
        (
            "estimation(posterior_sampling_method='slice',load_mh_file=true,posterior_sampler_options=('use_mh_covariance_matrix',|1));",
            "1",
            false,
        ),
        (
            "estimation(posterior_sampling_method='slice',load_mh_file,posterior_sampler_options=('use_mh_covariance_matrix',|1));",
            "1",
            true,
        ),
        (
            "estimation(posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',4,'optim',:('NumgradAlgorithm',|3)));",
            "3",
            false,
        ),
        (
            "estimation(posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',4.0,'optim',('NumgradAlgorithm',|3)));",
            "3",
            true,
        ),
    ] {
        let (_, items) = answers(source).await;
        assert_eq!(
            items
                .iter()
                .any(|item| item.label == label
                    && item.kind == Some(CompletionItemKind::ENUM_MEMBER)),
            expected,
            "{source}"
        );
    }
}

#[tokio::test]
async fn sampler_and_particle_values_respect_their_written_pair() {
    // Quoted strings and string vectors cannot shift the subsequent pair roles.
    for (source, expected) in [
        (
            "estimation(posterior_sampling_method='independent_metropolis_hastings',posterior_sampler_options=('opaque',['x','y'],'proposal_distribution','rand_multivariate_st|udent'));",
            Some("rand_multivariate_student"),
        ),
        (
            "estimation(filter_algorithm=sis,particle_filter_options=('pruning',tr|ue));",
            Some("true"),
        ),
        (
            "estimation(filter_algorithm=gf,particle_filter_options=('pruning',tr|ue));",
            None,
        ),
        (
            "estimation(filter_algorithm=sis,particle_filter_options=('Pruning',tr|ue));",
            None,
        ),
        (
            "estimation(mode_compute=4,optim=('NumgradAlgorithm',:|3));",
            None,
        ),
        (
            "estimation(posterior_sampling_method='tailored_random_block_metropolis_hastings',posterior_sampler_options=('mode_compute',4,'mode_compute',5,'optim',('NumgradAlgorithm',|3)));",
            None,
        ),
        (
            "estimation(mode_compute=4,optim=('NumgradAlgorithm',@{x}3|));",
            None,
        ),
    ] {
        let (_, items) = answers(source).await;
        let choices: Vec<_> = items
            .iter()
            .filter(|item| item.kind == Some(CompletionItemKind::ENUM_MEMBER))
            .collect();
        if let Some(expected) = expected {
            assert!(
                choices.iter().any(|item| item.label == expected),
                "{source}"
            );
        } else {
            assert!(choices.is_empty(), "{source}");
        }
    }
}
