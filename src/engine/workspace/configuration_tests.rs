//! Configured execution, source guards and retained-result regression checks.
use super::*;

fn workspace(source: &str, other: &[(&str, &str)]) -> Workspace {
    let mut files = BTreeMap::from([("main.mod".to_owned(), source.to_owned())]);
    files.extend(
        other
            .iter()
            .map(|(file, text)| (file.to_string(), text.to_string())),
    );
    Workspace::overlay_documents(&files)
}

fn target(ws: &mut Workspace, text: &str) -> GuardedTarget {
    ws.source_structure(
        "main.mod",
        &SourceStructureQuery {
            files: vec![SourceFileSelection::Root],
        },
    )
    .unwrap()
    .files[0]
        .sites
        .iter()
        .find(|site| site.expected_text == text)
        .unwrap()
        .target
        .clone()
}

fn names(ws: &mut Workspace) -> Vec<String> {
    let model = ws.get_effective_model("main.mod").unwrap();
    model
        .endogenous
        .iter()
        .map(|decl| model.intern.get(decl.name).to_owned())
        .collect()
}

#[test]
fn worker_snapshots_do_not_reuse_effective_models_from_another_configuration() {
    // Guards a background snapshot keeping Source parser facts after the root
    // selects a different analysis configuration before the worker commits.
    let source = "@#ifndef FULL_MODEL\n@#define FULL_MODEL = 1\n@#endif\n@#if FULL_MODEL == 0\nvar small; model; small=0; end;\n@#else\nvar a; var b; model; a=0; b=0; end;\n@#endif\n";
    let mut ws = workspace(source, &[]);
    assert_eq!(
        ws.get_effective_model("main.mod").unwrap().endogenous.len(),
        2
    );
    let worker = ws.snapshot_for_root_with_limit(
        "main.mod",
        ws.root_paths("main.mod").to_vec(),
        ws.root_macro_work_limit("main.mod"),
    );
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            defines: vec![InitialDefinition {
                name: "FULL_MODEL".into(),
                expression: "0".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    ws.adopt_root_analysis("main.mod", worker);
    assert_eq!(
        ws.get_effective_model("main.mod").unwrap().endogenous.len(),
        1
    );
}

#[test]
fn analysis_choices_and_paths_commit_together_after_one_validation() {
    // Guards native selection changing include paths after rejecting choices,
    // or eagerly rebuilding the model while committing one accepted change.
    let mut ws = workspace(
        "var y;model;\n@#include \"body.inc\"\nend;\n",
        &[("a/body.inc", "y=1;\n"), ("b/body.inc", "y=2;\n")],
    );
    let first = AnalysisConfiguration {
        defines: vec![InitialDefinition {
            name: "FLAG".into(),
            expression: "1".into(),
        }],
        ..Default::default()
    };
    ws.set_root_analysis_configuration_with_paths(
        "main.mod",
        first.clone(),
        Some(vec![PathBuf::from("a")]),
    )
    .unwrap();
    let before = ws.effective_model_snapshot("main.mod").unwrap();
    let revision = ws.input_revision("main.mod").unwrap();
    let mut invalid = first.clone();
    invalid.defines.push(invalid.defines[0].clone());
    assert!(
        ws.set_root_analysis_configuration_with_paths(
            "main.mod",
            invalid,
            Some(vec![PathBuf::from("b")])
        )
        .is_err()
    );
    assert_eq!(*ws.root_analysis_configuration("main.mod"), first);
    assert_eq!(ws.root_paths("main.mod"), &[PathBuf::from("a")]);
    assert_eq!(ws.input_revision("main.mod").unwrap(), revision);
    assert!(Arc::ptr_eq(
        &before,
        &ws.effective_model_snapshot("main.mod").unwrap()
    ));
    let second = AnalysisConfiguration {
        defines: vec![InitialDefinition {
            name: "FLAG".into(),
            expression: "2".into(),
        }],
        ..Default::default()
    };
    let builds = crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get());
    ws.set_root_analysis_configuration_with_paths(
        "main.mod",
        second.clone(),
        Some(vec![PathBuf::from("b"), PathBuf::from("b")]),
    )
    .unwrap();
    assert_eq!(
        crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get()),
        builds
    );
    assert_eq!(ws.root_paths("main.mod"), &[PathBuf::from("b")]);
    let after = ws.effective_model_snapshot("main.mod").unwrap();
    assert!(!Arc::ptr_eq(&before, &after));
    assert!(ws.snapshot_sources("main.mod").contains_key("b/body.inc"));
    assert!(!ws.snapshot_sources("main.mod").contains_key("a/body.inc"));
    ws.set_root_analysis_configuration_with_paths("main.mod", first, None)
        .unwrap();
    assert_eq!(ws.root_paths("main.mod"), &[PathBuf::from("b")]);
}

#[test]
fn initial_false_is_defined_and_later_assignments_keep_source_order() {
    // Guards treating false as absent, globally locking inputs, or accepting
    // duplicate initial names while changing the previous selected analysis.
    let mut ws = workspace(
        "@#ifdef FLAG\nvar a;\n@#endif\n@#define FLAG = 3\n@#if FLAG == 3\nvar b;\n@#endif\nmodel; a=0; b=0; end;\n",
        &[],
    );
    let choices = AnalysisConfiguration {
        defines: vec![
            InitialDefinition {
                name: "N".into(),
                expression: "2".into(),
            },
            InitialDefinition {
                name: "FLAG".into(),
                expression: "N==0".into(),
            },
        ],
        ..Default::default()
    };
    ws.set_root_analysis_configuration("main.mod", choices)
        .unwrap();
    assert_eq!(names(&mut ws), ["a", "b"]);
    assert!(ws.analysis_metadata("main.mod").unwrap().macro_complete);
    let revision = ws.input_revision("main.mod").unwrap();
    let duplicate = AnalysisConfiguration {
        defines: vec![
            InitialDefinition {
                name: "N".into(),
                expression: "1".into()
            };
            2
        ],
        ..Default::default()
    };
    assert_eq!(
        ws.set_root_analysis_configuration("main.mod", duplicate)
            .unwrap_err()
            .code,
        "ANALYSIS_DUPLICATE_DEFINE"
    );
    assert_eq!(ws.input_revision("main.mod").unwrap(), revision);
    assert_eq!(names(&mut ws), ["a", "b"]);
}

#[test]
fn site_replacement_and_skip_keep_include_acquisition_and_replay_equal() {
    // Guards changing every same-named definition, Skip undefining a prior
    // binding, or include acquisition disagreeing with compact expansion.
    let source = "@#define FILE = \"old.inc\"\n@#include FILE\n@#define FILE = \"later.inc\"\n@#include FILE\nmodel; a=0; b=0; end;\n";
    let mut ws = workspace(
        source,
        &[
            ("old.inc", "var old;\n"),
            ("new.inc", "var a;\n"),
            ("later.inc", "var b;\n"),
        ],
    );
    let first = target(&mut ws, "@#define FILE = \"old.inc\"");
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: first,
                expression: "\"new.inc\"".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(names(&mut ws), ["a", "b"]);
    assert!(
        ws.expand_report("main.mod")
            .unwrap()
            .effective_text
            .contains("var a")
    );
    assert_eq!(
        ws.analysis_metadata("main.mod").unwrap().overrides[0].execution_count,
        Some(1)
    );
    let later = target(&mut ws, "@#define FILE = \"later.inc\"");
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            defines: vec![InitialDefinition {
                name: "FILE".into(),
                expression: "\"new.inc\"".into(),
            }],
            overrides: vec![AnalysisOverride::SkipDefinition { target: later }],
            ..Default::default()
        },
    )
    .unwrap();
    // Both includes execute the preserved first written value. The parser may
    // refuse duplicate declarations, but the macro environment is still exact.
    let report = ws.expand_report("main.mod").unwrap();
    assert_eq!(report.effective_text.matches("var old").count(), 2);
}

#[test]
fn forced_chain_skips_missing_guards_and_does_not_activate_empty_parent() {
    // Guards evaluating replaced conditions or inventing an iteration/ancestor
    // to reach a selected nested chain.
    let source = "@#if UNKNOWN\nvar a;\n@#else\nvar b;\n@#endif\n@#for i in []\n@#if 0\nvar c;\n@#endif\n@#endfor\nmodel; b=0; end;\n";
    let mut ws = workspace(source, &[]);
    let view = ws
        .source_structure("main.mod", &SourceStructureQuery::default())
        .unwrap();
    let outer = view.files[0]
        .sites
        .iter()
        .find(|site| site.expected_text == "@#if UNKNOWN")
        .unwrap();
    let nested = view.files[0]
        .sites
        .iter()
        .find(|site| site.expected_text == "@#if 0")
        .unwrap();
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![
                AnalysisOverride::Branch {
                    target: outer.target.clone(),
                    branch_id: Some(outer.branches[1].branch_id.clone()),
                },
                AnalysisOverride::Branch {
                    target: nested.target.clone(),
                    branch_id: Some(nested.branches[0].branch_id.clone()),
                },
            ],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(names(&mut ws), ["b"]);
    let metadata = ws.analysis_metadata("main.mod").unwrap();
    assert!(metadata.selection.preview && metadata.macro_complete && metadata.model_complete);
    assert_eq!(metadata.overrides[0].execution_count, Some(1));
    assert_eq!(
        metadata.overrides[1].application,
        OverrideApplication::NotReached
    );
    let source = ws
        .source_structure("main.mod", &SourceStructureQuery::default())
        .unwrap();
    let c = source.files[0]
        .inventory
        .items
        .iter()
        .position(|item| item.name.as_deref() == Some("c"))
        .unwrap();
    assert_eq!(
        source.files[0].availability[c].availability,
        SourceAvailability::Inactive
    );
}

#[test]
fn repeated_include_receipts_count_successes_and_preserve_failed_prefix() {
    // Guards collapsing repeated include visits or counting a failed replacement
    // evaluation as an applied assignment.
    let mut ws = workspace(
        "@#for i in 1:2\n@#include \"helper.inc\"\n@#endfor\n",
        &[("helper.inc", "@#define N = 1\nvar y@{i};\n")],
    );
    let query = SourceStructureQuery {
        files: vec![SourceFileSelection::File("helper.inc".into())],
    };
    let selected = ws.source_structure("main.mod", &query).unwrap().files[0].sites[0]
        .target
        .clone();
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: selected.clone(),
                expression: "i+1".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(names(&mut ws), ["y1", "y2"]);
    assert_eq!(
        ws.analysis_metadata("main.mod").unwrap().overrides[0].execution_count,
        Some(2)
    );
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: selected,
                expression: "MISSING".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let metadata = ws.analysis_metadata("main.mod").unwrap();
    assert!(!metadata.macro_complete);
    assert_eq!(metadata.overrides[0].known_execution_count, 0);
    assert_eq!(metadata.overrides[0].execution_count, None);
    assert_eq!(
        metadata.overrides[0].application,
        OverrideApplication::Unknown
    );
}

#[test]
fn stale_choices_withhold_facts_and_switching_restores_guarded_snapshots() {
    // Guards silent retarget/fallback after edits and rebuilding matching model
    // and source-inventory results on an unchanged configuration switch.
    let source = "@#define FLAG = 0\n@#if FLAG\nvar a;\n@#else\nvar b;\n@#endif\n";
    let mut ws = workspace(source, &[]);
    let original = ws.effective_model_snapshot("main.mod").unwrap();
    let inventory = ws.source_inventory("main.mod").unwrap();
    let chosen = target(&mut ws, "@#define FLAG = 0");
    let alternative = AnalysisConfiguration {
        overrides: vec![AnalysisOverride::Definition {
            target: chosen,
            expression: "1".into(),
        }],
        ..Default::default()
    };
    ws.set_root_analysis_configuration("main.mod", alternative.clone())
        .unwrap();
    assert_eq!(names(&mut ws), ["a"]);
    ws.set_root_analysis_configuration("main.mod", AnalysisConfiguration::default())
        .unwrap();
    assert!(Arc::ptr_eq(
        &original,
        &ws.effective_model_snapshot("main.mod").unwrap()
    ));
    assert!(Arc::ptr_eq(
        &inventory,
        &ws.source_inventory("main.mod").unwrap()
    ));
    ws.set_root_analysis_configuration("main.mod", alternative)
        .unwrap();
    ws.update_document("main.mod", format!("{source}// edited\n"));
    assert!(ws.get_effective_model("main.mod").is_none());
    assert_eq!(
        ws.analysis_metadata("main.mod").unwrap_err().code,
        "ANALYSIS_TARGET_STALE"
    );
    assert!(matches!(
        ws.analysis_selection("main.mod"),
        AnalysisSelectionState::Stale { .. }
    ));
}

#[test]
fn later_failed_replacement_preserves_only_successful_applications() {
    // Guards counting the failing second visit and discarding the first
    // successfully applied assignment when the root then stops.
    let mut ws = workspace(
        "@#for i in 1:2\n@#define N = 0\nvar y@{i};\n@#endfor\n",
        &[],
    );
    let selected = target(&mut ws, "@#define N = 0");
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            defines: vec![InitialDefinition {
                name: "VALUES".into(),
                expression: "[7]".into(),
            }],
            overrides: vec![AnalysisOverride::Definition {
                target: selected,
                expression: "VALUES[i]".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let metadata = ws.analysis_metadata("main.mod").unwrap();
    assert!(!metadata.macro_complete);
    let receipt = &metadata.overrides[0];
    assert_eq!(receipt.application, OverrideApplication::Applied);
    assert_eq!(receipt.known_execution_count, 1);
    assert_eq!(receipt.execution_count, None);
    assert!(receipt.cause.is_some());
    assert!(
        ws.expand_report("main.mod")
            .unwrap()
            .effective_text
            .contains("y1")
    );
}

#[test]
fn guards_reject_wrong_stale_function_conflicting_and_foreign_branch_targets() {
    // Guards treating overrides as syntax repairs, retargeting stale source,
    // replacing functions or silently ignoring conflicting operations.
    let mut ws = workspace(
        "@#define N = 0\n@#define f(x) = x+1\n@#if N\nvar a;\n@#else\nvar b;\n@#endif\n",
        &[],
    );
    let selected = target(&mut ws, "@#define N = 0");
    let function = target(&mut ws, "@#define f(x) = x+1");
    let chain = target(&mut ws, "@#if N");
    let mut stale = selected.clone();
    stale.source_revision = "old revision".into();
    for (overrides, code) in [
        (
            vec![AnalysisOverride::SkipDefinition { target: stale }],
            "ANALYSIS_TARGET_STALE",
        ),
        (
            vec![AnalysisOverride::SkipDefinition { target: function }],
            "ANALYSIS_TARGET_UNSUPPORTED",
        ),
        (
            vec![AnalysisOverride::Branch {
                target: selected.clone(),
                branch_id: None,
            }],
            "ANALYSIS_TARGET_WRONG_KIND",
        ),
        (
            vec![
                AnalysisOverride::SkipDefinition {
                    target: selected.clone(),
                },
                AnalysisOverride::Definition {
                    target: selected,
                    expression: "1".into(),
                },
            ],
            "ANALYSIS_TARGET_CONFLICT",
        ),
        (
            vec![AnalysisOverride::Branch {
                target: chain,
                branch_id: Some("foreign arm".into()),
            }],
            "ANALYSIS_BRANCH_MISSING",
        ),
    ] {
        assert_eq!(
            ws.set_root_analysis_configuration(
                "main.mod",
                AnalysisConfiguration {
                    overrides,
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code,
            code
        );
        assert!(ws.root_analysis_configuration("main.mod").is_source());
        assert_eq!(names(&mut ws), ["b"]);
    }
}

#[test]
fn no_branch_unnamed_guards_and_map_policy_keep_source_identity() {
    // Guards executing an arm for an explicit no-arm choice, interpreting a
    // null file as a disk path, and reading disk from supplied-only sources.
    let mut ws = workspace("@#if 1\nvar a;\n@#else\nvar b;\n@#endif\n", &[]);
    ws.set_unnamed_root("main.mod");
    let selected = target(&mut ws, "@#if 1");
    assert!(selected.file.is_none());
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Branch {
                target: selected.clone(),
                branch_id: None,
            }],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(names(&mut ws).is_empty());
    assert_eq!(
        ws.analysis_metadata("main.mod").unwrap().overrides[0].execution_count,
        Some(1)
    );
    let mut external = selected;
    external.file = Some("C:/dynare/7.2/examples/example1.mod".into());
    assert_eq!(
        ws.set_root_analysis_configuration(
            "main.mod",
            AnalysisConfiguration {
                overrides: vec![AnalysisOverride::Branch {
                    target: external,
                    branch_id: None
                }],
                ..Default::default()
            }
        )
        .unwrap_err()
        .code,
        "ANALYSIS_TARGET_MISSING"
    );
    let unavailable = ws
        .source_structure(
            "main.mod",
            &SourceStructureQuery {
                files: vec![SourceFileSelection::File(
                    "C:/dynare/7.2/examples/example1.mod".into(),
                )],
            },
        )
        .unwrap();
    assert!(unavailable.files.is_empty());
    assert!(!unavailable.coverage.complete);
    assert!(
        unavailable
            .coverage
            .indexed_files
            .iter()
            .all(|file| !file.acquired && file.cause.is_some())
    );
}

#[test]
fn negative_include_guards_prevent_restoring_an_old_failure() {
    // Guards restoring an incomplete alternate after its formerly missing
    // active include becomes available, while dormant absence stays harmless.
    let source = "@#define USE = 0\n@#if USE\n@#include \"new.inc\"\n@#else\nvar b;\n@#endif\n";
    let mut ws = workspace(source, &[]);
    let selected = target(&mut ws, "@#define USE = 0");
    let alternate = AnalysisConfiguration {
        overrides: vec![AnalysisOverride::Definition {
            target: selected,
            expression: "1".into(),
        }],
        ..Default::default()
    };
    ws.set_root_analysis_configuration("main.mod", alternate.clone())
        .unwrap();
    assert!(!ws.analysis_metadata("main.mod").unwrap().model_complete);
    ws.set_root_analysis_configuration("main.mod", AnalysisConfiguration::default())
        .unwrap();
    assert_eq!(names(&mut ws), ["b"]);
    ws.update_document("new.inc", "var a;\n");
    ws.set_root_analysis_configuration("main.mod", alternate)
        .unwrap();
    assert_eq!(names(&mut ws), ["a"]);
    assert!(ws.analysis_metadata("main.mod").unwrap().macro_complete);
}

#[test]
fn cancelled_selection_does_not_publish_and_retained_variants_stay_bounded() {
    // Guards publishing a canceled selection and retaining every visited
    // configuration forever rather than evicting the bounded inactive cache.
    let mut ws = workspace(
        "@#define N = 0\n@#if N\nvar a;\n@#else\nvar b;\n@#endif\n",
        &[],
    );
    let selected = target(&mut ws, "@#define N = 0");
    for number in 1..=4 {
        ws.set_root_analysis_configuration(
            "main.mod",
            AnalysisConfiguration {
                overrides: vec![AnalysisOverride::Definition {
                    target: selected.clone(),
                    expression: number.to_string(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
        ws.get_effective_model("main.mod").unwrap();
    }
    assert!(ws.retained_roots.len() <= 2);
    assert!(
        ws.retained_roots
            .iter()
            .map(|entry| entry.retained_bytes)
            .sum::<usize>()
            <= 16 * 1024 * 1024
    );
    let previous = ws.root_analysis_configuration("main.mod");
    ws.cancellation = Some(Arc::new(AtomicBool::new(true)));
    assert!(
        ws.set_root_analysis_configuration("main.mod", AnalysisConfiguration::default())
            .is_err()
    );
    assert_eq!(*previous, *ws.root_analysis_configuration("main.mod"));
}

#[test]
fn label_changes_refresh_model_context_without_another_parse() {
    // Guards stale model labels after a label-only selection and mutation of
    // previously captured model snapshots while parser facts are reused.
    let mut ws = workspace("@#ifdef ON\nvar a;\n@#endif\n", &[]);
    let mut choices = AnalysisConfiguration {
        name: Some("First".into()),
        defines: vec![InitialDefinition {
            name: "ON".into(),
            expression: "true".into(),
        }],
        ..Default::default()
    };
    ws.set_root_analysis_configuration("main.mod", choices.clone())
        .unwrap();
    let before = ws.effective_model_snapshot("main.mod").unwrap();
    let count = crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get());
    choices.name = Some("Second".into());
    ws.set_root_analysis_configuration("main.mod", choices)
        .unwrap();
    let after = ws.effective_model_snapshot("main.mod").unwrap();
    assert_eq!(before.analysis_selection().name.as_deref(), Some("First"));
    assert_eq!(after.analysis_selection().name.as_deref(), Some("Second"));
    assert_eq!(
        before.analysis_selection().configuration_id,
        after.analysis_selection().configuration_id
    );
    assert_eq!(
        crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get()),
        count
    );
    assert!(Arc::ptr_eq(
        before.macro_source.as_ref().unwrap(),
        after.macro_source.as_ref().unwrap()
    ));
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            name: Some("Written".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        ws.get_effective_model("main.mod")
            .unwrap()
            .analysis_selection()
            .name
            .as_deref(),
        Some("Written")
    );
}

#[test]
fn configured_discarded_regions_feed_auxiliary_checks_and_preview() {
    // Guards auxiliary parser/diagnostic checks re-expanding with Source inputs
    // and reporting malformed declaration/assignment text in an inactive arm.
    let mut ws = workspace(
        "@#if ON\nvar a;\nmodel; a=0; end;\n@#else\nvar 2bad;\nx = 1\n@#endif\n",
        &[],
    );
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            defines: vec![InitialDefinition {
                name: "ON".into(),
                expression: "true".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let model = ws.effective_model_snapshot("main.mod").unwrap();
    let discarded = model.source.find("var 2bad").unwrap() as u32;
    assert!(
        model
            .inactive_macro_spans()
            .iter()
            .any(|span| span.start <= discarded && discarded < span.end)
    );
    assert!(crate::engine::diagnostics::check_parse::check_parse(&model).is_empty());
    let report = ws.expand_report("main.mod").unwrap();
    let preview = crate::engine::preview::preview_source::source(report, &model, &[], &[]).unwrap();
    assert!(preview.proven);
    assert!(preview.text.contains("a=0"));
    assert!(!preview.text.contains("2bad"));
}

#[test]
fn source_document_availability_keeps_child_and_iteration_outcomes() {
    // Guards active parent text hiding a nested inactive arm, and losing
    // different emission outcomes or caller tuple-loop values in included text.
    let source = "@#for (i,j) in [(1,2),(0,3)]\n@#include \"child.inc\"\n@#endfor\n";
    let child = "@#if i\nvar a;\n@#else\nvar b;\n@#endif\n";
    let mut ws = workspace(source, &[("child.inc", child)]);
    ws.get_effective_model("main.mod").unwrap();
    let doc = ws.source_document("main.mod", "child.inc").unwrap();
    let at = child.find("a;").unwrap();
    assert_eq!(
        doc.availability(Span::new(at, at + 1)),
        SourceAvailability::Mixed
    );
    let structure = ws
        .source_structure(
            "main.mod",
            &SourceStructureQuery {
                files: vec![SourceFileSelection::File("child.inc".into())],
            },
        )
        .unwrap();
    let loops: Vec<_> = structure.files[0]
        .availability
        .iter()
        .flat_map(|item| item.occurrences.iter())
        .flat_map(|occurrence| occurrence.loops.iter())
        .collect();
    assert!(loops.iter().any(|frame| {
        frame
            .variable
            .as_deref()
            .is_some_and(|name| name.contains('i'))
            && frame
                .value
                .as_deref()
                .is_some_and(|value| value.contains('2'))
    }));
}

#[test]
fn source_document_availability_retains_successful_empty_substitutions() {
    // Guards an executed empty tag prefix losing its availability because it
    // emits no token, while preserving dormant and failed substitution states.
    let child = "@#if key == \"name\"\n@#define prefix=\"label_\"\n@#else\n@#define prefix=\"\"\n@#endif\n[@{key}='@{prefix}y'] y=0;\n";
    let mut ws = workspace(
        "var y; model;\n@#for key in [\"endogenous\",\"endogenous\"]\n@#include \"child.inc\"\n@#endfor\nend;\n",
        &[("child.inc", child)],
    );
    ws.get_effective_model("main.mod").unwrap();
    let at = child.find("@{prefix}").unwrap();
    let span = Span::new(at, at + "@{prefix}".len());
    let doc = ws.source_document("main.mod", "child.inc").unwrap();
    assert_eq!(doc.availability(span), SourceAvailability::Active);
    let occurrences: Vec<_> = doc.availability_index.matching("child.inc", span).collect();
    assert_eq!(occurrences.len(), 2);
    assert!(
        occurrences
            .iter()
            .all(|region| region.active && region.loops.len() == 1)
    );
    assert_ne!(occurrences[0].include_call, occurrences[1].include_call);

    for (source, expected) in [
        (
            "@#if 0\n@{\"\"}\n@#endif\nvar y;model;y=0;end;\n",
            SourceAvailability::Inactive,
        ),
        (
            "@#for active in [true,false]\n@#if active\n@{\"\"}\n@#endif\n@#endfor\nvar y;model;y=0;end;\n",
            SourceAvailability::Mixed,
        ),
        (
            "@{MISSING}\nvar y;model;y=0;end;\n",
            SourceAvailability::Unknown,
        ),
    ] {
        let mut ws = workspace(source, &[]);
        ws.get_effective_model("main.mod").unwrap();
        let at = source.find("@{").unwrap();
        let end = at + source[at..].find('}').unwrap() + 1;
        assert_eq!(
            ws.source_document("main.mod", "main.mod")
                .unwrap()
                .availability(Span::new(at, end)),
            expected
        );
    }
}

#[test]
fn compatible_configurations_share_source_work_and_restore_model_facts() {
    // Guards scanning/hashing a shared helper once per root or alternative,
    // rebuilding inventory on switches, and losing exact work-limit cache keys.
    let mut body = String::from("var y;\nmodel;\n");
    for _ in 0..1000 {
        body.push_str("y=0;\n");
    }
    body.push_str("end;\n");
    let mut ws = workspace(
        "@#if ON\n@#include \"body.inc\"\n@#endif\n",
        &[("body.inc", &body)],
    );
    let selected = AnalysisConfiguration {
        defines: vec![InitialDefinition {
            name: "ON".into(),
            expression: "true".into(),
        }],
        ..Default::default()
    };
    ws.set_root_analysis_configuration("main.mod", selected.clone())
        .unwrap();
    let before_model =
        crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get());
    let first = ws.effective_model_snapshot("main.mod").unwrap();
    let cold_model_parses = crate::engine::language::parser::PARSE_EXPANDED_BUILDS
        .with(|value| value.get())
        - before_model;
    let view = ws
        .source_structure("main.mod", &SourceStructureQuery::default())
        .unwrap();
    let doc = Arc::clone(ws.docs.get("body.inc").unwrap());
    let mut budget = ws.macro_budget("main.mod");
    let scan = doc.macro_source(&mut budget).unwrap();
    let inventory = doc.source_inventory(None).unwrap();
    let first_work = ws.spliced.get("main.mod").unwrap().work_used;
    let preparation_work = ws
        .spliced
        .get("main.mod")
        .unwrap()
        .macro_source_preparation_work;
    ws.set_root_analysis_configuration("main.mod", AnalysisConfiguration::default())
        .unwrap();
    ws.get_effective_model("main.mod").unwrap();
    let count = crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get());
    ws.set_root_analysis_configuration("main.mod", selected)
        .unwrap();
    let restored = ws.effective_model_snapshot("main.mod").unwrap();
    assert!(Arc::ptr_eq(&first, &restored));
    assert!(Arc::ptr_eq(&scan, &doc.macro_source(&mut budget).unwrap()));
    assert!(Arc::ptr_eq(
        &inventory,
        &doc.source_inventory(None).unwrap()
    ));
    // Restoring the previously captured configuration needs no parse.
    assert_eq!(
        crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|value| value.get()),
        count
    );
    let new_limit = MacroWorkLimit::new(1_500_000).unwrap();
    let limited = doc.with_work_limit(new_limit);
    assert!(Arc::ptr_eq(
        &scan,
        &limited
            .macro_source(&mut MacroBudget::with_limit(new_limit, None))
            .unwrap()
    ));
    let limited_inventory = limited.source_inventory(None).unwrap();
    assert!(!Arc::ptr_eq(&inventory, &limited_inventory));
    println!(
        "shared1000: macro_source_work={} inventory_work={} original_walk_and_splice_work={} joined_preparation_work={} cold_model_parses={} restored_model_parses=0 source_files={}",
        scan.work_used,
        inventory.work_used,
        first_work,
        preparation_work,
        cold_model_parses,
        view.files.len()
    );
}

#[test]
fn parser_resource_refusals_preserve_selected_preview_context() {
    // Guards resource refusals before source scanning losing the selected
    // preview identity and requested choices on the returned incomplete model.
    let source = "@#define N = 0\nvar y; model; y=@{N}; end;\n";
    let mut ws = workspace(source, &[]);
    let selected = target(&mut ws, "@#define N = 0");
    let choices = AnalysisConfiguration {
        name: Some("Alternative".into()),
        overrides: vec![AnalysisOverride::Definition {
            target: selected,
            expression: "1".into(),
        }],
        ..Default::default()
    };
    let selection = ws
        .set_root_analysis_configuration("main.mod", choices.clone())
        .unwrap();
    let model = ws.effective_model_snapshot("main.mod").unwrap();
    let context = model.analysis_context.as_ref().unwrap();
    // The first allowance stops the initial source guard. The second passes
    // that guard, then stops the uncached MacroSource acquisition.
    for allowance in [1, source.len() + 1] {
        let mut budget =
            MacroBudget::with_limit(MacroWorkLimit::new(allowance as u64).unwrap(), None);
        let refused = crate::engine::language::parser::parse_configured_with_budget(
            source,
            &[],
            &[],
            &mut budget,
            Some(Arc::clone(context)),
            None,
        );
        assert!(refused.macro_incomplete());
        assert!(refused.is_analysis_preview());
        assert_eq!(refused.analysis_selection(), &selection);
        assert_eq!(refused.analysis_configuration(), &choices);
        assert!(Arc::ptr_eq(
            refused.analysis_context.as_ref().unwrap(),
            context
        ));
    }
}

#[test]
fn worker_snapshots_keep_new_closed_dormant_override_targets() {
    // Guards worker capture/retention dropping a valid closed target before
    // input_revision has recorded it as an explicitly selected source guard.
    let folder = std::env::temp_dir().join(format!(
        "dyg-configured-snapshot-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&folder).unwrap();
    // Normalize the existing directory before joining missing files: this
    // resolves macOS aliases and strips Windows's verbatim path prefix.
    let folder = PathBuf::from(path_key(&folder));
    let root = path_key(&folder.join("root.mod"));
    let dormant = path_key(&folder.join("dormant.inc"));
    std::fs::write(&dormant, "@#define N = 0\n").unwrap();
    let mut ws = Workspace::new();
    ws.update_document(
        &root,
        "@#if 0\n@#include \"dormant.inc\"\n@#endif\nvar y; model; y=0; end;\n",
    );
    let discovered = ws
        .source_structure(
            &root,
            &SourceStructureQuery {
                files: vec![SourceFileSelection::File(dormant.clone())],
            },
        )
        .unwrap();
    let selected = discovered.files[0].sites[0].target.clone();
    ws.set_root_analysis_configuration(
        &root,
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: selected,
                expression: "1".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        ws.input_snapshots
            .get(&root)
            .is_none_or(|files| !files.contains_key(&dormant))
    );
    assert!(!ws.docs[&dormant].overlay);
    let original = Arc::clone(&ws.docs[&dormant]);
    let mut snapshot = ws.snapshot_with_root_settings(&root);
    assert!(Arc::ptr_eq(&original, &snapshot.docs[&dormant]));
    assert!(matches!(
        snapshot.analysis_selection(&root),
        AnalysisSelectionState::Selected(_)
    ));
    let metadata = snapshot.analysis_metadata(&root).unwrap();
    assert!(metadata.model_complete);
    assert_eq!(
        metadata.overrides[0].application,
        OverrideApplication::NotReached
    );
    assert_eq!(metadata.overrides[0].execution_count, Some(0));
    ws.retain_analysis_roots(&HashSet::from([root.clone()]));
    assert!(Arc::ptr_eq(&original, &ws.docs[&dormant]));
    assert!(matches!(
        ws.analysis_selection(&root),
        AnalysisSelectionState::Selected(_)
    ));
    let resolved = folder.canonicalize().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    assert!(resolved.starts_with(&temporary) && resolved != temporary);
    std::fs::remove_dir_all(resolved).unwrap();
}

#[test]
fn override_origins_follow_actual_reads_aliases_and_source_redefinitions() {
    // Guards missing definition links, lexical guesses through local function
    // parameters or a comprehension predicate, and stale links after a source
    // assignment replaces a value. The filtered array depends on N; its leaked
    // index i receives independent literal values and must have no such link.
    let source = "@#define N = 0\n@#define alias = N+1\n@#define f(x) = x+N\n@#define shadow(N) = N\n@#define filtered = [i in [1,2] when N == 0]\nvar y; model;\ny=@{N};\ny=@{alias};\ny=@{f(1)};\ny=@{shadow(5)};\ny=@{i};\ny=@{filtered[1]};\n@#define N = 7\ny=@{N};\ny=@{alias};\ny=@{f(1)};\n@#define alias = 9\ny=@{alias};\nend;\n";
    let mut ws = workspace(source, &[]);
    let selected = target(&mut ws, "@#define N = 0");
    let definition = Span::new(0, "@#define N = 0".len());
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: selected,
                expression: "0".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let model = ws.effective_model_snapshot("main.mod").unwrap();
    let report = ws.expand_report_snapshot("main.mod").unwrap();
    assert!(report.complete && report.navigation_complete);
    let linked: Vec<_> = report
        .model_map
        .equations
        .iter()
        .map(|row| {
            row.source.origin_frames.iter().any(|frame| {
                frame.kind == "override_definition"
                    && frame.segments.iter().any(|segment| {
                        segment.file.as_deref() == Some("main.mod") && segment.span == definition
                    })
            })
        })
        .collect();
    assert_eq!(
        linked,
        [
            true, true, true, false, false, true, false, true, false, false
        ]
    );
    let substitution = source.find("@{N}").unwrap();
    let span = Span::new(substitution, substitution + "@{N}".len());
    let direct = report.navigation[0]
        .macro_frames
        .iter()
        .find(|frame| frame.kind == "override_definition")
        .unwrap();
    assert!(
        direct
            .directive_segments
            .iter()
            .any(|segment| segment.span == definition)
    );
    assert!(
        direct
            .body_segments
            .iter()
            .any(|segment| segment.span == span)
    );
    let generated = model
        .expanded_tokens
        .iter()
        .find(|token| token.span == span && token.text(&model.source) == "0")
        .unwrap();
    assert!(
        generated
            .emitted
            .as_ref()
            .unwrap()
            .source
            .origins
            .iter()
            .any(|origin| origin.written == span && !origin.copied)
    );
    assert!(
        report.origins[0]
            .origin_frames
            .iter()
            .any(|frame| frame.kind == "override_definition" && frame.origin_span == definition)
    );
}

#[test]
fn override_origins_survive_include_loop_reconstruction() {
    // Different nested include targets force loop reconstruction. Every
    // generated loop value must retain the actual overridden collection link.
    let source = "@#define V = [0]\n@#for i in V\n@#include \"switch.inc\"\n@#endfor\n";
    let a = "var a; model; a=@{i}; end;\n";
    let b = "var b; model; b=@{i}; end;\n";
    let mut ws = workspace(
        source,
        &[
            (
                "switch.inc",
                "@#if i == 1\n@#include \"a.inc\"\n@#else\n@#include \"b.inc\"\n@#endif\n",
            ),
            ("a.inc", a),
            ("b.inc", b),
        ],
    );
    let selected = target(&mut ws, "@#define V = [0]");
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: selected,
                expression: "[1,2]".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let model = ws.effective_model_snapshot("main.mod").unwrap();
    let report = ws.expand_report_snapshot("main.mod").unwrap();
    assert!(report.complete && report.navigation_complete);
    assert_eq!(report.n_equations, 2);
    for (row, (file, text)) in report.navigation.iter().zip([("a.inc", a), ("b.inc", b)]) {
        let at = text.find("@{i}").unwrap();
        let frame = row
            .macro_frames
            .iter()
            .find(|frame| frame.kind == "override_definition")
            .unwrap();
        assert!(
            frame
                .directive_segments
                .iter()
                .any(|segment| segment.file.as_deref() == Some("main.mod")
                    && segment.span == Span::new(0, "@#define V = [0]".len()))
        );
        assert!(
            frame
                .body_segments
                .iter()
                .any(|segment| segment.file.as_deref() == Some(file)
                    && segment.span == Span::new(at, at + "@{i}".len()))
        );
    }
    let evaluations = ws.source_evaluations("main.mod");
    let preview = crate::engine::preview::preview_source::source_with_evaluations(
        &report,
        &model,
        &ws.source_layout_gaps("main.mod"),
        &ws.source_line_segments("main.mod"),
        &evaluations,
        Default::default(),
    )
    .unwrap()
    .unwrap();
    assert!(preview.proven);
    assert!(preview.text.contains("a=1") && preview.text.contains("b=2"));
}

#[test]
fn forced_branch_frames_keep_the_overridden_chain_link() {
    // Guards an explicitly selected else arm losing its chain origin or being
    // labelled as an ordinary source branch in expansion navigation.
    let source =
        "@#if MISSING\nvar a; model; a=0; end;\n@#else\nvar b; model; b=1; end;\n@#endif\n";
    let mut ws = workspace(source, &[]);
    let view = ws
        .source_structure("main.mod", &SourceStructureQuery::default())
        .unwrap();
    let site = &view.files[0].sites[0];
    ws.set_root_analysis_configuration(
        "main.mod",
        AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Branch {
                target: site.target.clone(),
                branch_id: Some(site.branches[1].branch_id.clone()),
            }],
            ..Default::default()
        },
    )
    .unwrap();
    let report = ws.expand_report_snapshot("main.mod").unwrap();
    assert!(report.complete && report.navigation_complete);
    let frame = report.navigation[0]
        .macro_frames
        .iter()
        .find(|frame| frame.kind == "override_branch")
        .unwrap();
    assert!(
        frame
            .directive_segments
            .iter()
            .any(|segment| segment.span == site.directive_span)
    );
    assert!(
        report.model_map.equations[0]
            .source
            .origin_frames
            .iter()
            .any(|frame| frame.kind == "override_branch")
    );
}
