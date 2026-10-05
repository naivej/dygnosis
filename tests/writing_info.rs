use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use dygnosis::explain::{explain, ExplainKind};
use dygnosis::{analyze, check_file, parse, Diagnostic};

#[test]
fn included_writing_notes_use_owner_scalar_positions_in_cli_and_mcp() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-writing-wire-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "// root 😀\r\n@#include \"child.inc\"\r\n";
    let child_text = "/*😀*/var y;\r\nmodel;\r\ny = 2;\r\nend;\r\n";
    std::fs::write(&root, root_text).unwrap();
    std::fs::write(&child, child_text).unwrap();
    let root_key = root.to_string_lossy().to_string();
    let child_key = child.to_string_lossy().to_string();
    let set = dygnosis::check_file_with_origins(root_text, &root_key);
    let output = dygnosis::format_check_lines_with_origins(&root_key, &set, root_text);
    assert!(output.contains("child.inc:1:6: INFO [I209]"), "{output}");
    assert!(output.contains("child.inc:2:1: INFO [I208]"), "{output}");
    assert!(output.contains("child.inc:3:5: INFO [I210]"), "{output}");
    let files = HashMap::from([
        (root_key.clone(), root_text.to_string()),
        (child_key.clone(), child_text.to_string()),
    ]);
    let notes = dygnosis::dynare_diagnose(root_text, Some(&root_key), Some(&files));
    for (code, line, column) in [("I209", 1, 6), ("I208", 2, 1), ("I210", 3, 5)] {
        let note = notes.iter().find(|note| note.code == code).unwrap();
        assert_eq!(note.file.as_deref(), Some(child_key.as_str()));
        assert_eq!((note.line, note.column), (line, column));
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

fn fixture(name: &str) -> (String, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/writing")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("missing {}: {err}", path.display()))
        .replace("\r\n", "\n");
    (text, path.to_string_lossy().to_string())
}

fn codes(name: &str) -> Vec<String> {
    let (text, path) = fixture(name);
    check_file(&text, &path)
        .into_iter()
        .map(|diag| diag.code)
        .filter(|code| code.starts_with('I') && code != "I050")
        .collect()
}

fn one(name: &str, code: &str) -> Diagnostic {
    let (text, path) = fixture(name);
    let hits: Vec<_> = check_file(&text, &path)
        .into_iter()
        .filter(|diag| diag.code == code)
        .collect();
    assert_eq!(hits.len(), 1, "{name} {code}: {hits:?}");
    hits.into_iter().next().unwrap()
}

fn slice_of(name: &str, diag: &Diagnostic) -> String {
    let (text, _) = fixture(name);
    let start = diag.span.start as usize;
    let end = diag.span.end as usize;
    text.get(start..end).unwrap_or("").to_string()
}

#[test]
fn singular_summaries_anchor_on_their_statement_keyword() {
    let unnamed = one("one_each.mod", "I208");
    assert_eq!(unnamed.message, "1 counted equation has no name tag.");
    assert_eq!(slice_of("one_each.mod", &unnamed), "model");

    let (text, path) = fixture("one_each.mod");
    let symbols: Vec<_> = check_file(&text, &path)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "I209")
        .collect();
    assert_eq!(symbols.len(), 3);
    for (symbol, keyword) in symbols.iter().zip(["var", "varexo", "parameters"]) {
        assert_eq!(symbol.message, "1 symbol has no long_name.");
        assert_eq!(slice_of("one_each.mod", symbol), keyword);
    }

    let numbers = one("one_each.mod", "I210");
    assert_eq!(
        numbers.message,
        "1 number is written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("one_each.mod", &numbers), "2");
}

#[test]
fn plural_wording() {
    assert_eq!(
        one("plural.mod", "I208").message,
        "2 counted equations have no name tag."
    );
    assert_eq!(
        one("plural.mod", "I209").message,
        "2 symbols have no long_name."
    );
    assert_eq!(
        one("plural.mod", "I210").message,
        "2 numbers are written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("plural.mod", &one("plural.mod", "I210")), "2");
}

#[test]
fn named_zero_and_one_stay_quiet() {
    let got = codes("quiet_named.mod");
    assert!(
        !got.iter()
            .any(|code| code == "I208" || code == "I209" || code == "I210"),
        "{got:?}"
    );
}

#[test]
fn power_counts_and_signed_one_is_quiet() {
    let numbers = one("power.mod", "I210");
    assert_eq!(
        numbers.message,
        "1 number is written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("power.mod", &numbers), "2");
    assert!(codes("power.mod").iter().all(|code| code != "I208"));
    assert!(codes("power.mod").iter().all(|code| code != "I209"));
}

#[test]
fn macro_copies_count_and_share_the_template_anchor() {
    assert_eq!(
        one("loop.mod", "I208").message,
        "3 counted equations have no name tag."
    );
    let numbers = one("loop.mod", "I210");
    assert_eq!(
        numbers.message,
        "3 numbers are written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("loop.mod", &numbers), "4");
}

#[test]
fn macro_names_count_once_each() {
    let (text, path) = fixture("macro_names.mod");
    let symbols: Vec<_> = check_file(&text, &path)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "I209")
        .collect();
    assert_eq!(symbols.len(), 3);
    for symbol in &symbols {
        assert_eq!(symbol.message, "1 symbol has no long_name.");
        assert_eq!(slice_of("macro_names.mod", symbol), "var");
    }
    assert!(!codes("macro_names.mod").iter().any(|code| code == "I208"));
}

#[test]
fn removed_equation_is_not_counted() {
    let got = codes("removed.mod");
    assert!(
        !got.iter().any(|code| code == "I208" || code == "I210"),
        "{got:?}"
    );
}

#[test]
fn heterogeneous_rows_share_one_summary() {
    assert_eq!(
        one("het.mod", "I208").message,
        "2 counted equations have no name tag."
    );
    let symbols = one("het.mod", "I209");
    assert_eq!(symbols.message, "1 symbol has no long_name.");
    assert_eq!(slice_of("het.mod", &symbols), "var");
    let numbers = one("het.mod", "I210");
    assert_eq!(
        numbers.message,
        "1 number is written directly in equations. Consider named parameters."
    );
    assert_eq!(slice_of("het.mod", &numbers), "3");
}

#[test]
fn include_rows_use_the_root_model_keyword_and_keep_literal_ownership() {
    let (text, path) = fixture("parent.mod");
    let diags = check_file(&text, &path);
    let unnamed = diags.iter().find(|diag| diag.code == "I208").unwrap();
    let child = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/writing/child.mod"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    assert_eq!(
        &text[unnamed.span.start as usize..unnamed.span.end as usize],
        "model"
    );
    let number = diags.iter().find(|diag| diag.code == "I210").unwrap();
    let literal = &child[number.span.start as usize..number.span.end as usize];
    assert_eq!(literal, "4");
    assert!(diags.iter().all(|diag| diag.code != "I209"));
}

#[test]
fn unresolved_include_and_syntax_withhold_the_summaries() {
    for name in ["unresolved.mod", "syntax.mod"] {
        let got = codes(name);
        assert!(
            !got.iter()
                .any(|code| code == "I208" || code == "I209" || code == "I210"),
            "{name}: {got:?}"
        );
    }
}

#[test]
fn disable_comment_does_not_silence_information() {
    let got = codes("suppress.mod");
    assert!(got.iter().any(|code| code == "I208"), "{got:?}");
    assert!(got.iter().any(|code| code == "I210"), "{got:?}");
}

#[test]
fn empty_long_name_still_counts() {
    let symbols = one("empty_long_name.mod", "I209");
    assert_eq!(symbols.message, "1 symbol has no long_name.");
    assert_eq!(slice_of("empty_long_name.mod", &symbols), "var");
}

#[test]
fn deterministic_exogenous_is_counted_once() {
    let symbols = one("det_once.mod", "I209");
    assert_eq!(symbols.message, "1 symbol has no long_name.");
    assert_eq!(slice_of("det_once.mod", &symbols), "varexo_det");
}

#[test]
fn concatenated_names_count_once_each_and_are_not_duplicates() {
    let src = "\
@#define is = 1:2
@#for i in is
var x@{i};
@#endfor
model;
@#for i in is
x@{i} = 0;
@#endfor
end;
";
    let model = parse(src);
    let names: Vec<&str> = model
        .endogenous
        .iter()
        .map(|decl| model.name(decl.name))
        .collect();
    assert_eq!(names, ["x1", "x2"]);
    let diags = analyze(&model);
    assert!(diags.iter().all(|diag| diag.code != "W031"), "{diags:?}");
    let note = diags.iter().find(|diag| diag.code == "I209").unwrap();
    assert_eq!(note.message, "2 symbols have no long_name.");
}

#[test]
fn repeated_declaration_still_warns_and_counts_once() {
    let src = "\
var x;
var x;
model;
x = 0;
end;
";
    let diags = analyze(&parse(src));
    assert!(diags.iter().any(|diag| diag.code == "W031"), "{diags:?}");
    let note = diags.iter().find(|diag| diag.code == "I209").unwrap();
    assert_eq!(note.message, "1 symbol has no long_name.");
}

#[test]
fn unresolved_name_expansion_withholds_the_summary() {
    let src = "\
var x@{UNDEF};
model;
x = 0;
end;
";
    let diags = analyze(&parse(src));
    assert!(diags.iter().all(|diag| diag.code != "I209"), "{diags:?}");
    assert!(diags.iter().any(|diag| diag.code == "E063"), "{diags:?}");
}

#[test]
fn explain_calls_them_writing_preferences() {
    for code in ["I208", "I209", "I210"] {
        let entry = explain(code).unwrap();
        assert_eq!(entry.kind, ExplainKind::Added);
        assert!(entry.body.contains("writing preference"));
        assert!(entry.body.contains("not a Dynare refusal"));
    }
}

mod ownership {
    use dygnosis::{analyze, check_file_with_origins, parse, Diagnostic, WritingRows};
    use std::collections::{BTreeMap, HashMap, HashSet};

    fn notes(source: &str, code: &str) -> Vec<Diagnostic> {
        analyze(&parse(source))
            .into_iter()
            .filter(|diagnostic| diagnostic.code == code)
            .collect()
    }

    #[test]
    fn change_type_keeps_the_first_eligible_declaration_owner() {
        let source = "parameters p; varexo e; change_type(var) p; model; p=.5*p(-1)+e; end;";
        let model = parse(source);
        let summaries = notes(source, "I209");
        assert_eq!(summaries.len(), 2);
        for (note, keyword) in summaries.iter().zip(["parameters", "varexo"]) {
            let context = note.writing.as_ref().unwrap();
            assert_eq!(context.statement_ids.len(), 1);
            assert_eq!(context.rows.ids().len(), 1);
            assert_eq!(
                &source[note.span.start as usize..note.span.end as usize],
                keyword
            );
            let row = model
                .written_declarations
                .iter()
                .find(|row| row.declaration.parse_order == context.rows.ids()[0])
                .unwrap();
            assert_eq!(row.statement_id, context.statement_ids[0]);
            assert_eq!(model.statements[row.statement_id].name, keyword);
        }
    }

    #[test]
    fn missing_declaration_metadata_keeps_a_range_without_inventing_an_owner() {
        // The parser retains every eligible declaration. Exercise the fallback
        // on a public Model whose caller removed that ownership metadata.
        let mut model = parse("var x y; parameters p; model; x=y; y=p*x; end;");
        let prior = model.endogenous[0].span;
        model.written_declarations.clear();
        let diagnostics = analyze(&model);
        let summaries: Vec<_> = diagnostics
            .iter()
            .filter(|note| note.code == "I209")
            .collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].message, "3 symbols have no long_name.");
        assert_eq!(summaries[0].span, prior);
        let context = summaries[0].writing.as_ref().unwrap();
        assert!(context.statement_ids.is_empty());
        assert_eq!(context.rows.ids().len(), 3);
    }

    #[test]
    fn statements_own_only_their_surviving_counted_unnamed_rows() {
        let source = "var x y z; varexo e; parameters p; p=.8;\nmodel; #l=2; [static,name='static'] x=l; [dynamic] x=e; [name='old'] y=p*y(-1); z=y; end;\nmodel_replace('old'); y=x; end;\nmodel; [name='drop'] z=3; end;\nmodel_remove('drop');";
        let model = parse(source);
        let diagnostics = analyze(&model);
        let summaries: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "I208")
            .collect();
        assert_eq!(summaries.len(), 2, "{diagnostics:?}");
        let active: HashSet<_> = model
            .equations
            .iter()
            .map(|equation| equation.parse_order)
            .collect();
        for (note, keyword, count) in [
            (summaries[0], "model", 2),
            (summaries[1], "model_replace", 1),
        ] {
            assert_eq!(
                &source[note.span.start as usize..note.span.end as usize],
                keyword
            );
            assert_eq!(note.severity, dygnosis::Severity::Information);
            let context = note.writing.as_ref().unwrap();
            assert_eq!(context.statement_ids.len(), 1);
            assert_eq!(context.rows.ids().len(), count);
            assert!(matches!(context.rows, WritingRows::Equations(_)));
            let expected: Vec<_> = model
                .written_equations
                .iter()
                .filter(|row| {
                    row.statement_id == context.statement_ids[0]
                        && active.contains(&row.token_range.start)
                        && !row.equation.is_local
                        && !row.equation.static_tag
                        && row.equation.name.is_empty()
                        && row
                            .equation
                            .tag_map
                            .get("name")
                            .is_none_or(String::is_empty)
                })
                .map(|row| row.token_range.start)
                .collect();
            assert_eq!(context.rows.ids(), expected);
        }
        assert_eq!(
            summaries[0].message,
            "2 counted equations have no name tag."
        );
        assert_eq!(summaries[1].message, "1 counted equation has no name tag.");
        let declared: Vec<_> = diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "I209")
            .collect();
        assert_eq!(
            declared
                .iter()
                .map(|note| note.message.as_str())
                .collect::<Vec<_>>(),
            [
                "3 symbols have no long_name.",
                "1 symbol has no long_name.",
                "1 symbol has no long_name."
            ]
        );
    }

    #[test]
    fn long_name_deduplication_assigns_each_symbol_to_its_first_eligible_statement() {
        let source =
            "var x(long_name='X');\nvar x y;\nvar y;\nparameters p;\nmodel; x=y(-1); y=p*x; end;";
        let model = parse(source);
        let summaries = notes(source, "I209");
        assert_eq!(summaries.len(), 2);
        let names: Vec<Vec<_>> = summaries
            .iter()
            .map(|diagnostic| {
                let context = diagnostic.writing.as_ref().unwrap();
                assert_eq!(context.statement_ids.len(), 1);
                context
                    .rows
                    .ids()
                    .iter()
                    .map(|id| {
                        let row = model
                            .written_declarations
                            .iter()
                            .find(|row| row.declaration.parse_order == *id)
                            .unwrap();
                        assert_eq!(row.statement_id, context.statement_ids[0]);
                        model.name(row.declaration.name)
                    })
                    .collect()
            })
            .collect();
        assert_eq!(names, [vec!["y"], vec!["p"]]);
        assert_eq!(
            summaries[0].span.start as usize,
            source.find("var x y").unwrap()
        );
        assert_eq!(
            summaries[1].span.start as usize,
            source.find("parameters").unwrap()
        );
        assert!(summaries
            .iter()
            .all(|note| note.message == "1 symbol has no long_name."));
    }

    #[test]
    fn repeated_macro_openers_sum_exact_rows_and_keep_execution_ids() {
        let source =
            "@#for i in 1:3\n/*😀中*/var x@{i};\n/*😀中*/model; x@{i}=x@{i}(-1); end;\n@#endfor\n";
        let set = check_file_with_origins(source, "C:/writing-scope/repeat.mod");
        for (code, keyword, message) in [
            ("I208", "model", "3 counted equations have no name tag."),
            ("I209", "var", "3 symbols have no long_name."),
        ] {
            let hits: Vec<_> = set
                .diagnostics
                .iter()
                .filter(|note| note.code == code)
                .collect();
            assert_eq!(hits.len(), 1);
            let note = hits[0];
            let context = note.writing.as_ref().unwrap();
            assert_eq!(note.message, message);
            assert_eq!(
                &source[note.span.start as usize..note.span.end as usize],
                keyword
            );
            assert_eq!(context.statement_ids.len(), 3);
            assert_eq!(context.rows.ids().len(), 3);
            assert_eq!(
                context
                    .rows
                    .ids()
                    .iter()
                    .copied()
                    .collect::<HashSet<_>>()
                    .len(),
                3
            );
            assert_eq!(context.root, set.root);
            assert!(!context.input_revision.is_empty());
        }
        let same_symbol = source.replace("x@{i}", "x");
        let declaration = notes(&same_symbol, "I209");
        assert_eq!(declaration.len(), 1);
        assert_eq!(declaration[0].message, "1 symbol has no long_name.");
        assert_eq!(declaration[0].writing.as_ref().unwrap().rows.ids().len(), 1);
    }

    #[test]
    fn split_include_owners_remain_distinct_per_root_and_note() {
        let files = HashMap::from([
        ("a.mod".to_string(), "@#include \"open.inc\"\n".to_string()),
        ("b.mod".to_string(), "parameters p; p=1;\n@#include \"open.inc\"\n".to_string()),
        ("open.inc".to_string(), "/*😀中*/var\n@#include \"names.inc\"\n;\n/*😀中*/model;\n@#include \"rows.inc\"\nend;\n/*😀中*/model; y=x; end;".to_string()),
        ("names.inc".to_string(), "x y".to_string()),
        ("rows.inc".to_string(), "x=x(-1);".to_string()),
    ]);
        // The wire report proves each root keeps both model notes and the written
        // keyword owner, instead of the first child's file or a code-only owner.
        let report = dygnosis::dynare_workspace_diagnose(
            Some(&files),
            Some(&["a.mod".into(), "b.mod".into()]),
            None,
        )
        .unwrap();
        for root in report["roots"].as_array().unwrap() {
            let diagnostics = root["diagnostics"].as_array().unwrap();
            let equations: Vec<_> = diagnostics
                .iter()
                .filter(|diagnostic| diagnostic["code"] == "I208")
                .collect();
            assert_eq!(equations.len(), 2);
            for (note, line) in equations.iter().zip([4, 7]) {
                assert_eq!(note["file"], "open.inc");
                assert_eq!(note["line"], line);
                assert_eq!(note["column"], 7);
                assert_eq!(note["end_column"], 12);
                assert_eq!(note["message"], "1 counted equation has no name tag.");
            }
            let declaration = diagnostics
                .iter()
                .find(|diagnostic| diagnostic["code"] == "I209" && diagnostic["file"] == "open.inc")
                .unwrap();
            assert_eq!(declaration["line"], 1);
            assert_eq!(declaration["column"], 7);
            assert_eq!(declaration["end_column"], 10);
            assert_eq!(declaration["message"], "2 symbols have no long_name.");
        }
    }

    #[test]
    fn unsafe_keywords_keep_the_previous_range_without_losing_scoped_counts() {
        let source = "@#define d=\"var\"\n@#define k=\"model\"\n@#for i in 1:2\n@{d} x@{i};\n@{k}; x@{i}=x@{i}(-1); end;\n@#endfor\n";
        let model = parse(source);
        let summaries = analyze(&model);
        let declaration = summaries.iter().find(|note| note.code == "I209").unwrap();
        let equation = summaries.iter().find(|note| note.code == "I208").unwrap();
        assert_eq!(
            declaration.span,
            model.written_declarations[0].declaration.span
        );
        assert_eq!(equation.span, model.written_equations[0].equation.span);
        assert_eq!(declaration.message, "2 symbols have no long_name.");
        assert_eq!(equation.message, "2 counted equations have no name tag.");
        assert_eq!(declaration.writing.as_ref().unwrap().statement_ids.len(), 2);
        assert_eq!(equation.writing.as_ref().unwrap().statement_ids.len(), 2);
    }

    #[test]
    fn several_statements_keep_explicit_expected_counts_and_multiplicity() {
        let source =
            "var x y; varexo e; parameters p; p=.8; model; x=p*x(-1)+e; end; model; y=x; end;";
        let model = parse(source);
        let summaries = analyze(&model);
        let mut expected = BTreeMap::from([("I208", vec![1, 1]), ("I209", vec![2, 1, 1])]);
        for code in ["I208", "I209"] {
            let hits: Vec<_> = summaries.iter().filter(|note| note.code == code).collect();
            let counts: Vec<_> = hits
                .iter()
                .map(|note| note.writing.as_ref().unwrap().rows.ids().len())
                .collect();
            assert_eq!(counts, expected.remove(code).unwrap());
            let owners: Vec<_> = hits
                .iter()
                .map(|note| note.writing.as_ref().unwrap().statement_ids[0])
                .collect();
            assert_eq!(
                owners.iter().copied().collect::<HashSet<_>>().len(),
                hits.len()
            );
            for (note, owner) in hits.iter().zip(owners) {
                assert_eq!(note.span, model.statements[owner].keyword_span);
            }
        }
    }
}
