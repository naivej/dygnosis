use dygnosis::{dynare_extract, parse};
use std::collections::HashMap;
use std::time::Duration;

fn fragment(source: &str) -> String {
    let result = dynare_extract(source, None, None, &["Y".into()], &HashMap::new(), None).unwrap();
    assert_eq!(result["status"], "ok", "{result}");
    result["fragment"].as_str().unwrap().to_string()
}

fn check_fragment(text: &str) {
    if let Some(pp) = dygnosis::find_preprocessor(None) {
        let official = dygnosis::run_preprocessor(
            text,
            &pp,
            None,
            Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(official.success, "{text}: {official:?}");
    }
}

#[test]
fn extract_preserves_the_types_needed_by_selected_equations() {
    let source = "var y z extra; change_type(parameters) z extra; parameters p; change_type(var) p; model; [name='Y'] y=z+p; [name='P'] p=0; end;";
    let result = dynare_extract(source, None, None, &["Y".into()], &HashMap::new(), None).unwrap();
    assert_eq!(result["status"], "ok", "{result}");
    let text = result["fragment"].as_str().unwrap();
    let model = parse(text);
    assert_eq!(
        model
            .final_parameters()
            .iter()
            .map(|d| model.name(d.name))
            .collect::<Vec<_>>(),
        ["z"],
        "{text}"
    );
    assert_eq!(
        model
            .final_endogenous()
            .iter()
            .map(|d| model.name(d.name))
            .collect::<Vec<_>>(),
        ["y", "p"],
        "{text}"
    );
    assert!(!text.contains("extra"), "{text}");
    check_fragment(text);
}

#[test]
fn whitespace_comments_and_metadata_do_not_keep_unselected_names() {
    let source = "var y; var a $A$ (long_name='A, (level)',country='us') // a comment\n drop, b $B$ (long_name='B'); change_type(parameters) a b; model; [name='Y'] y=a+b; end;";
    let text = fragment(source);
    assert!(!text.contains("drop"), "{text}");
    assert!(
        text.contains("long_name='A, (level)',country='us'"),
        "{text}"
    );
    let model = parse(&text);
    let a = model
        .final_parameters()
        .into_iter()
        .find(|d| model.name(d.name) == "a")
        .unwrap();
    assert_eq!(a.long_name.as_deref(), Some("A, (level)"));
    assert_eq!(a.tex_name.as_deref(), Some("A"));
    check_fragment(&text);
}

#[test]
fn macro_type_changes_keep_execution_order_and_expanded_names() {
    let source = "var y z;\n@#for j in 1:2\n@#if j==2\nchange_type(parameters) z;\n@#endif\n@#if j==1\nchange_type(varexo) z;\n@#endif\n@#endfor\nmodel; [name='Y'] y=z; end;";
    let text = fragment(source);
    assert!(
        text.find("change_type(varexo)").unwrap() < text.find("change_type(parameters)").unwrap(),
        "{text}"
    );
    let model = parse(&text);
    assert_eq!(model.final_parameters().len(), 1, "{text}");
    check_fragment(&text);

    let source = "var y;\n@#for j in 1:2\nvar z@{j};\nchange_type(parameters) z@{j};\n@#endfor\nmodel; [name='Y'] y=z1+z2; end;";
    let text = fragment(source);
    let model = parse(&text);
    assert_eq!(model.final_parameters().len(), 2, "{text}");
    assert!(!text.contains("@{"), "{text}");
    check_fragment(&text);
}

#[test]
fn includes_and_heterogeneous_retypes_keep_required_directives() {
    let source = "@#include \"defs.inc\"\nmodel; [name='Y'] y=z; end;";
    let files = HashMap::from([
        ("root.mod".into(), source.into()),
        (
            "defs.inc".into(),
            "var y z; change_type(parameters) z;".into(),
        ),
    ]);
    let result = dynare_extract(
        source,
        Some("root.mod"),
        Some(&files),
        &["Y".into()],
        &HashMap::new(),
        None,
    )
    .unwrap();
    let text = result["fragment"].as_str().unwrap();
    assert_eq!(parse(text).final_parameters().len(), 1, "{text}");
    check_fragment(text);

    let text = fragment("heterogeneity_dimension h; var y; var(heterogeneity=h) z; change_type(parameters) z; model; [name='Y'] y=z; end;");
    let model = parse(&text);
    assert_eq!(model.final_parameters().len(), 1, "{text}");
    assert!(text.contains("heterogeneity_dimension h"), "{text}");
    check_fragment(&text);
}

#[test]
fn type_dependent_declarations_stay_between_their_changes() {
    for source in [
        "parameters p; change_type(var) p; predetermined_variables p; model; [name='Y'] p=.5*p(-1); end;",
        "var y p; predetermined_variables p; change_type(parameters) p; model; [name='Y'] y=p; end;",
        "var z; change_type(parameters) z; var(deflator=z) y; model; [name='Y'] y=0; end;",
    ] {
        check_fragment(source);
        let text = fragment(source);
        check_fragment(&text);
        assert_eq!(parse(&text).final_parameters().len(), parse(source).final_parameters().len(), "{text}");
    }
}

#[test]
fn declaration_words_and_semicolons_inside_metadata_are_not_boundaries() {
    for description in ["var wrong", "A; parameters fake; (level)"] {
        let source = format!("var y; var a(long_name='{description}') b $B$ (long_name='B'); change_type(parameters) b; model; [name='Y'] y=b; end;");
        let text = fragment(&source);
        let model = parse(&text);
        let parameters = model.final_parameters();
        assert_eq!(parameters.len(), 1, "{text}");
        assert_eq!(model.name(parameters[0].name), "b", "{text}");
        assert_eq!(parameters[0].tex_name.as_deref(), Some("B"));
        check_fragment(&text);
    }
}

#[test]
fn repeated_type_changes_with_the_same_span_keep_every_execution() {
    let source="var y z;\n@#for j in 1:2\nchange_type(parameters) z;\n@#if j==1\nchange_type(varexo) z;\n@#endif\n@#endfor\nmodel; [name='Y'] y=z; end;";
    let text = fragment(source);
    assert_eq!(
        text.matches("change_type(parameters) z;").count(),
        2,
        "{text}"
    );
    assert_eq!(parse(&text).final_parameters().len(), 1, "{text}");
    check_fragment(&text);
}
