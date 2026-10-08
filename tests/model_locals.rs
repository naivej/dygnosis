//! Model-local grammar, callback validity, execution order, and binding reach.
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use dygnosis::model_locals::{scope_at_order, ModelLocals};
use dygnosis::{analyze, parse, run_preprocessor, JsonStage, Severity, Workspace};

fn official(source: &str, accepted: bool, needle: Option<&str>) {
    let binary = Path::new("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        return;
    }
    let result = run_preprocessor(
        source,
        binary,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    let report = format!("{} {}", result.raw_stdout, result.raw_stderr);
    assert_eq!(result.success, accepted, "{source}\n{report}");
    if let Some(needle) = needle {
        assert!(report.contains(needle), "{needle}: {source}\n{report}");
    }
}

fn errors(source: &str) -> Vec<dygnosis::Diagnostic> {
    analyze(&parse(source))
        .into_iter()
        .filter(|row| row.severity == Severity::Error)
        .collect()
}

#[test]
fn exact_local_production_rejects_the_five_starting_gaps() {
    for (body, needle) in [
        ("#z+q=p; y=z;", "unexpected PLUS, expecting EQUAL"),
        ("#z(0)=p; y=z;", "unexpected '(', expecting EQUAL"),
        ("#z; y=0;", "unexpected ';', expecting EQUAL"),
        ("[name='helper'] #z=p; y=z;", "unexpected '#'"),
        ("#z=p\ny=z;", "unexpected IDENTIFIER"),
        ("#z=; y=0;", "unexpected ';'"),
        ("#z=", "unexpected END"),
        ("#z=p+; y=0;", "unexpected ';'"),
        ("#z=(p; y=0;", "unexpected ';'"),
        ("#exp=p; y=0;", "unexpected EXP"),
        ("#values=p; y=0;", "unexpected VALUES"),
    ] {
        let source = format!("var y; parameters p q; p=.9; q=.1; model; {body} end;");
        official(&source, false, Some(needle));
        let model = parse(&source);
        let rows = analyze(&model);
        assert!(
            rows.iter()
                .any(|row| row.code == "E001" && row.message.contains(needle)),
            "{source}: {rows:?}"
        );
        assert!(
            ModelLocals::collect(&model).definitions.is_empty(),
            "{source}"
        );
        assert!(
            model
                .written_equations
                .iter()
                .all(|row| !row.equation.is_local),
            "{source}"
        );
        assert!(
            model
                .write_targets
                .iter()
                .all(|write| model.name(write.name) != "z"),
            "{source}"
        );
    }
}

#[test]
fn explicit_local_declaration_has_its_own_name_and_tex_grammar() {
    let source = "model_local_variable z $Z$ w, q $Q$; var y; model; y=0; end;";
    official(source, true, None);
    let model = parse(source);
    let facts = ModelLocals::collect(&model);
    assert_eq!(facts.declarations.len(), 3);
    assert_eq!(facts.declarations[0].tex_name.as_deref(), Some("Z"));
    assert_eq!(facts.declarations[2].tex_name.as_deref(), Some("Q"));
    for statement in [
        "model_local_variable(log) z;",
        "model_local_variable(heterogeneity=h) z;",
        "model_local_variable z (long_name='helper');",
        "model_local_variable z $Z$ $Q$;",
        "model_local_variable;",
        "model_local_variable z,;",
        "model_local_variable ,z;",
        "model_local_variable z=1;",
        "model_local_variable exp;",
        "model_local_variable datafile;",
        "model_local_variable z $Z$ exp;",
    ] {
        let source = format!("{statement} var y; model; y=0; end;");
        official(&source, false, Some("syntax error"));
        let model = parse(&source);
        assert!(
            analyze(&model).iter().any(|row| row.code == "E001"),
            "{source}"
        );
        assert!(model.model_local_variables.is_empty(), "{source}");
        assert!(
            ModelLocals::collect(&model).declarations.is_empty(),
            "{source}"
        );
    }
}

#[test]
fn accepted_shifted_and_explicit_forward_uses_bind_in_one_scope() {
    for source in [
        "var y; parameters p; p=.9; model; #z=p*y(-1); y=z(+1); end;",
        "var y; parameters p; p=.9; model_local_variable z $Z$; model; y=z; #z=p; end;",
        "var y; model; #z=1; y=z; end; model; y=z; end;",
        "heterogeneity_dimension h; var(heterogeneity=h) y; model(heterogeneity=h); #z=1; y=z; end; model(heterogeneity=h); y=z; end;",
    ] {
        official(source, true, None);
        assert!(errors(source).is_empty(), "{source}: {:?}", errors(source));
        let model = parse(source);
        let facts = ModelLocals::collect(&model);
        assert_eq!(facts.definitions.len(), 1);
        assert!(!facts.uses.is_empty());
        assert!(facts.uses.iter().all(|read| read.definition == Some(0)));
        assert_eq!((facts.definitions[0].target_span.end - facts.definitions[0].target_span.start), 1);
    }
}

#[test]
fn duplicate_definitions_share_dimension_scope_across_blocks() {
    for source in [
        "var y; model; #z=1; y=z; end; model; #z=2; end;",
        "heterogeneity_dimension h; var(heterogeneity=h) y; model(heterogeneity=h); #z=1; y=z; end; model(heterogeneity=h); #z=2; end;",
        "var y; model;\n@#for j in 1:2\n#z=1;\n@#endfor\ny=z; end;",
    ] {
        official(source, false, Some("Local model variable z declared twice."));
        let model = parse(source);
        let rows = analyze(&model);
        assert!(rows.iter().any(|row| row.code == "E030" && row.message == "Local model variable z declared twice."), "{source}: {rows:?}");
        assert_eq!(ModelLocals::collect(&model).definitions.len(), 1);
        assert_eq!(model.write_targets.iter().filter(|write| model.name(write.name) == "z").count(), 1);
    }
}

#[test]
fn scope_binding_excludes_another_dimensions_expression() {
    let source = "heterogeneity_dimension h g; var y; var(heterogeneity=h) yh; var(heterogeneity=g) yg; model_local_variable z; model; #z=1; y=z; end; model(heterogeneity=h); #z=2; yh=z; end; model(heterogeneity=g); #z=3; yg=z; end;";
    official(source, true, None);
    let model = parse(source);
    let facts = ModelLocals::collect(&model);
    assert_eq!(facts.definitions.len(), 3);
    for read in &facts.uses {
        assert_eq!(
            facts.definitions[read.definition.unwrap()].dimension,
            read.dimension
        );
    }
    assert_eq!(facts.declarations.len(), 1);
    for definition in &facts.definitions {
        assert_eq!(definition.declaration, Some(0));
    }
}

#[test]
fn unfinished_rows_preserve_earlier_locals_and_corrected_input_restores_facts() {
    for tail in ["#bad =", "#bad =; y=keep;", "#bad(0)=1; y=keep;"] {
        let source = format!("var y; model; #keep=1; {tail} end;");
        let model = parse(&source);
        let facts = ModelLocals::collect(&model);
        assert_eq!(facts.definitions.len(), 1, "{source}: {facts:?}");
        assert_eq!(model.name(facts.definitions[0].name), "keep");
        let order = model
            .statements
            .iter()
            .find(|row| row.name == "model")
            .unwrap()
            .opener_range
            .end
            + 5;
        assert_eq!(scope_at_order(&model, order), Some(None));
        assert_eq!(facts.available(None, order).len(), 1);
    }
    let corrected = "var y; model; #keep=1; #bad=keep+1; y=bad; end;";
    official(corrected, true, None);
    assert!(errors(corrected).is_empty());
    assert_eq!(ModelLocals::collect(&parse(corrected)).definitions.len(), 2);
}

#[test]
fn rejected_types_and_early_uses_do_not_create_valid_bindings() {
    for source in [
        "var y z; model; #z=1; y=0; z=0; end;",
        "var y; model; y=z; #z=1; end;",
        "var y; model; #z=z+1; y=0; end;",
    ] {
        official(
            source,
            false,
            Some("wrong type or was already used on the right-hand side"),
        );
        let model = parse(source);
        assert!(
            analyze(&model).iter().any(|row| row.code == "E025"),
            "{source}: {:?}",
            analyze(&model)
        );
        assert!(ModelLocals::collect(&model).definitions.is_empty());
        assert!(model
            .write_targets
            .iter()
            .all(|write| model.name(write.name) != "z"));
    }
}

#[test]
fn execution_order_keeps_repeated_written_macro_sites_distinct() {
    let source = "var y; parameters p; p=.9; model;\n@#for n in [\"a\",\"b\"]\n#@{n}=p;\ny=@{n};\n@#endfor\nend;";
    official(source, true, None);
    let model = parse(source);
    let facts = ModelLocals::collect(&model);
    assert_eq!(facts.definitions.len(), 2);
    assert_eq!(
        facts.definitions[0].target_span,
        facts.definitions[1].target_span
    );
    assert!(facts.definitions[0].parse_order < facts.definitions[1].parse_order);
    assert_eq!(facts.uses.len(), 2);
    assert_eq!(facts.uses[0].definition, Some(0));
    assert_eq!(facts.uses[1].definition, Some(1));
    let between = facts.definitions[1].parse_order;
    assert_eq!(facts.available(None, between).len(), 1);
}

#[test]
fn included_definitions_use_the_same_binding_and_written_origin() {
    let mut files = HashMap::new();
    files.insert(
        "root.mod".to_string(),
        "var y; model;\n@#include \"locals.inc\"\ny=z; end;".to_string(),
    );
    files.insert("locals.inc".to_string(), "#z=1;".to_string());
    let mut workspace = Workspace::new();
    for (file, text) in &files {
        workspace.update_document(file, text.clone());
    }
    let model = workspace.get_effective_model("root.mod").unwrap();
    let facts = ModelLocals::collect(model);
    assert_eq!(facts.definitions.len(), 1);
    assert_eq!(facts.uses[0].definition, Some(0));
    let map = &workspace.expand_report("root.mod").unwrap().model_map;
    let source = &map.equations[facts.definitions[0].equation_index].source;
    assert!(source
        .anchor
        .as_ref()
        .unwrap()
        .file
        .as_ref()
        .unwrap()
        .ends_with("locals.inc"));
}

#[test]
fn native_top_level_hash_and_cross_tree_crash_stay_quiet() {
    let native = "var y; #z=1; model; y=0; end;";
    official(native, true, None);
    assert!(ModelLocals::collect(&parse(native)).definitions.is_empty());
    let source = include_str!("fixtures/p_hank/quiet_cross_tree_local_use.mod");
    let model = parse(source);
    assert!(errors(source).is_empty());
    let facts = ModelLocals::collect(&model);
    assert_eq!(facts.definitions.len(), 1);
    assert!(facts
        .uses
        .iter()
        .any(|read| read.dimension.is_none() && read.definition.is_none()));
}

#[test]
fn generic_name_and_type_codes_reach_both_local_forms_at_check() {
    for (code, source, needle) in [
        ("E020", "var y; model; #z=missing; y=z; end;", "Unknown symbol: missing"),
        ("E024", "var y; varexo_det e; model; #z=e(-1); y=z; end;", "Exogenous deterministic variable e cannot be given a lead or a lag."),
        ("E030", "var z y; model_local_variable z; model; y=0; z=0; end;", "Symbol z declared twice with different types!"),
        ("W031", "model_local_variable z; model_local_variable z; var y; model; y=0; end;", "Symbol z declared twice."),
        ("E240", "var y; model_local_variable z; model; y=0; end; forecast z;", "Variable z is not one of"),
        ("E240", "var y; model; #z=1; y=z; end; forecast z;", "Variable z is not one of"),
        ("E253", "var y; model_local_variable z; model; #z=1; y=0; end; ramsey_model; planner_objective z;", "Model local variable z cannot be used in 'planner_objective'."),
        ("E282", "var y; parameters p; model_local_variable z; p=z; model; y=0; end;", "Variable z not allowed outside model declaration."),
        ("E282", "var y; parameters p; model; #z=1; y=z; end; p=z;", "Variable z not allowed outside model declaration."),
        ("E280", "var y; external_function(name=f,nargs=1); model; #z=f; y=0; end;", "cannot be used like a variable"),
        ("E281", "var y; parameters q; q=p; model; #z=p; y=0; end;", "not allowed inside model declaration"),
        ("E294", "var y; epilogue; q=y; end; model; #z=q; y=0; end;", "Symbol 'q' cannot be used outside the epilogue block."),
        ("E426", "var y gone; var_remove gone; model; #z=gone; y=0; end;", "Variable 'gone' can no longer be used since it has been excluded"),
        ("E481", "var y; model_local_variable z; model; y=0; end; steady_state_model; z=1; end;", "z has incorrect type"),
        ("E317", "var y; model_local_variable z; model; y=0; end; ramsey_model(instruments=(z));", "z is not endogenous."),
    ] {
        official(source, code.starts_with('W'), Some(needle));
        assert!(analyze(&parse(source)).iter().any(|row| row.code == code), "{code}: {source}: {:?}", analyze(&parse(source)));
    }
}

#[test]
fn local_rename_names_use_both_pinned_symbol_contexts() {
    for name in ["helper", "model", "alpha", "name", "bind"] {
        assert!(dygnosis::model_locals::is_local_name(name), "{name}");
    }
    for name in [
        "end", "exp", "datafile", "values", "diff", "nan", "z(0)", "1z",
    ] {
        assert!(!dygnosis::model_locals::is_local_name(name), "{name}");
    }
}

#[test]
fn scope_cursor_excludes_closers_and_next_statements_but_keeps_partial_body() {
    for source in [
        "var y; model; #z=1; y=z; end; steady;",
        "var y; model; #z=1; y=z; end",
        "var y; model; #z=1; #bad =",
    ] {
        let model = parse(source);
        let block = model
            .statements
            .iter()
            .find(|row| row.name == "model")
            .unwrap();
        assert_eq!(scope_at_order(&model, block.opener_range.end), Some(None));
        if source.ends_with("#bad =") {
            assert_eq!(scope_at_order(&model, block.token_range.end), Some(None));
        }
        for (order, token) in model.expanded_tokens.iter().enumerate() {
            if token.text(&model.source).eq_ignore_ascii_case("end")
                || (block.complete && order >= block.token_range.end)
            {
                assert_eq!(scope_at_order(&model, order), None, "{source}: {order}");
            }
        }
    }
}

#[test]
fn later_explicit_metadata_keeps_the_definition_and_use_in_one_binding() {
    let source = "var y; model; #z=1; y=z; end; model_local_variable z $Z$;";
    official(source, true, Some("Symbol z declared twice."));
    let model = parse(source);
    let facts = ModelLocals::collect(&model);
    assert_eq!(facts.declarations.len(), 1);
    assert_eq!(facts.definitions.len(), 1);
    assert_eq!(facts.uses.len(), 1);
    assert_eq!(facts.definitions[0].declaration, Some(0));
    assert_eq!(facts.uses[0].declaration, Some(0));
    assert_eq!(facts.uses[0].definition, Some(0));
    assert_eq!(facts.declarations[0].tex_name.as_deref(), Some("Z"));
    assert!(facts
        .available(None, facts.definitions[0].parse_order)
        .is_empty());
    let available = facts.available(None, facts.uses[0].parse_order);
    assert_eq!(available.len(), 1);
    assert_eq!(available[0].declaration, Some(0));
    assert_eq!(available[0].definition, Some(0));

    let declaration_only = parse("var y; model; y=0; end; model_local_variable z;");
    let facts = ModelLocals::collect(&declaration_only);
    let order = facts.declarations[0].parse_order;
    assert!(facts.available(None, order - 1).is_empty());
    assert_eq!(facts.available(None, order).len(), 1);
}
