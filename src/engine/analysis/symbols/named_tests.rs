//! Typed identity regressions exercise source selection and complete edits.
use super::*;
use std::collections::BTreeMap;

fn view(text: &str) -> SemanticModel {
    let mut workspace =
        Workspace::overlay_documents(&BTreeMap::from([("root.mod".into(), text.into())]));
    SemanticModel::capture(&mut workspace, "root.mod", "revision".into()).unwrap()
}

fn choose(view: &SemanticModel, name: &str, kind: &str) -> Binding {
    let candidates: Vec<_> = view
        .bindings()
        .into_iter()
        .filter(|row| row.name == name && row.kind == kind)
        .collect();
    assert_eq!(candidates.len(), 1, "{name}/{kind}: {candidates:?}");
    candidates[0].clone()
}

fn edit(view: &SemanticModel, binding: &Binding, replacement: &str) -> String {
    let proposal = rename(std::slice::from_ref(view), 0, binding, replacement).unwrap();
    let mut text = view.sources["root.mod"].clone();
    for row in proposal.edits.iter().rev() {
        text.replace_range(
            row.span.start as usize..row.span.end as usize,
            &row.replacement,
        );
    }
    text
}

// An equal ordinary spelling must neither hide a dimension nor contribute its
// assignments/timing to the dimension's binding. Source positions disambiguate.
#[test]
fn dimensions_have_typed_sites_and_namespace_safe_rename() {
    let text = "heterogeneity_dimension h;parameters h;h=0.5;var(heterogeneity=h) x;model(heterogeneity=h);x=x(-1);end;";
    let model = view(text);
    let dimension = choose(&model, "h", "heterogeneity_dimension");
    let ordinary = choose(&model, "h", "parameters");
    assert!(ordinary.is_ordinary());
    assert!(!dimension.is_ordinary());
    assert_eq!(
        model.select(Selector::Name("h")).unwrap_err().reason.code,
        "AMBIGUOUS_SYMBOL"
    );
    let sites = model.references(&dimension, true);
    assert_eq!(sites.len(), 3);
    for site in sites {
        let (file, span) = site.written.unwrap();
        assert_eq!(
            model
                .select(Selector::Target {
                    file: &file,
                    byte: span.start
                })
                .unwrap()
                .unwrap()
                .id,
            dimension.id
        );
    }
    let changed = edit(&model, &dimension, "households");
    assert!(changed.contains("heterogeneity_dimension households;parameters h;h=0.5"));
    assert!(changed.contains("var(heterogeneity=households)"));
    let changed = edit(&model, &ordinary, "rho");
    assert!(changed.contains("heterogeneity_dimension h;parameters rho;rho=0.5"));
    assert!(changed.contains("var(heterogeneity=h)"));
}

// Named model arguments, auxiliary links and PAC target information are
// separate typed slots, rather than ordinary uses with equal source text.
#[test]
fn named_model_definitions_and_operators_share_exact_links() {
    let text = "var y;varexo e;parameters p;p=.9;model;[name='Y'] y=.5*y(-1)+e+var_expectation(ve)+pac_expectation(pm);end;var_model(model_name=vm,eqtags=['Y']);var_expectation_model(model_name=ve,variable=y,auxiliary_model_name=vm,horizon=1);pac_model(model_name=pm,auxiliary_model_name=vm,discount=p);pac_target_info(pm);target y;end;";
    let model = view(text);
    for (name, kind, sites) in [
        ("vm", "var_model", 3),
        ("ve", "var_expectation_model", 2),
        ("pm", "pac_model", 3),
    ] {
        let binding = choose(&model, name, kind);
        assert_eq!(model.references(&binding, true).len(), sites);
        assert!(!binding.is_ordinary());
        for site in model.references(&binding, true) {
            let (file, span) = site.written.unwrap();
            assert_eq!(
                model
                    .select(Selector::Target {
                        file: &file,
                        byte: span.start
                    })
                    .unwrap()
                    .unwrap()
                    .id,
                binding.id
            );
        }
        let renamed = edit(&model, &binding, &format!("{name}_new"));
        assert!(renamed.contains(&format!("model_name={name}_new")));
    }
}

// The two official auxiliary lookup priorities must survive query and Rename.
#[test]
fn var_and_trend_model_names_keep_distinct_lookup_priority() {
    let text = "var y;parameters p;p=.9;model;[name='Y']y=.5*y(-1);end;var_model(model_name=same,eqtags=['Y']);trend_component_model(model_name=same,eqtags=['Y'],targets=['Y']);var_expectation_model(model_name=ve,variable=y,auxiliary_model_name=same,horizon=1);pac_model(model_name=pm,auxiliary_model_name=same,discount=p);";
    let model = view(text);
    let var = choose(&model, "same", "var_model");
    let trend = choose(&model, "same", "trend_component_model");
    assert_eq!(model.references(&var, true).len(), 2);
    assert_eq!(model.references(&trend, true).len(), 2);
    let changed = edit(&model, &var, "forecast_var");
    assert!(changed.contains(
        "var_expectation_model(model_name=ve,variable=y,auxiliary_model_name=forecast_var"
    ));
    assert!(changed.contains("pac_model(model_name=pm,auxiliary_model_name=same"));
}

// Database namespace edits have an external effect. Members remain distinct
// from an ordinary model binding, and repeated member reads remain visible.
#[test]
fn databases_and_members_are_known_with_specific_external_refusals() {
    let model = view(
        "var y;varexo e;database db;model;y=e;end;shock_paths;var e;periods 1,2;values db.foreign,db.foreign;end;",
    );
    let database = choose(&model, "db", "database");
    assert_eq!(model.references(&database, true).len(), 3);
    assert_eq!(
        rename(std::slice::from_ref(&model), 0, &database, "other")
            .unwrap_err()
            .code,
        "EXTERNAL_NAMESPACE"
    );
    let member = choose(&model, "foreign", "database_variable");
    assert_eq!(model.references(&member, true).len(), 2);
    assert!(!member.is_ordinary());
    assert_eq!(
        rename(std::slice::from_ref(&model), 0, &member, "other")
            .unwrap_err()
            .code,
        "EXTERNAL_DATABASE_MEMBER"
    );
}

// Qualified callees keep a complete declaration/call range, with a refusal
// that names the unowned implementation rather than claiming no binding.
#[test]
fn external_functions_keep_qualified_callees_and_derivative_names() {
    let text = "external_function(name=pkg.helper,nargs=1,first_deriv_provided=pkg.deriv);var y;model;y=pkg.helper(y);end;";
    let model = view(text);
    let function = choose(&model, "pkg.helper", "external_function");
    let sites = model.references(&function, true);
    assert_eq!(sites.len(), 2);
    for site in sites {
        let (file, span) = site.written.unwrap();
        assert_eq!(&text[span.start as usize..span.end as usize], "pkg.helper");
        assert_eq!(
            model
                .select(Selector::Target {
                    file: &file,
                    byte: span.end - 1
                })
                .unwrap()
                .unwrap()
                .id,
            function.id
        );
    }
    assert_eq!(
        rename(std::slice::from_ref(&model), 0, &function, "other")
            .unwrap_err()
            .code,
        "EXTERNAL_IMPLEMENTATION"
    );
    assert_eq!(
        model
            .references(&choose(&model, "pkg.deriv", "external_function"), true)
            .len(),
        1
    );
    let quoted_text = "external_function(name='ext',nargs=1,first_deriv_provided='deriv');var y;model;y=ext(y);end;";
    let quoted = view(quoted_text);
    for (name, count) in [("ext", 2), ("deriv", 1)] {
        let binding = choose(&quoted, name, "external_function");
        let sites = quoted.references(&binding, true);
        assert_eq!(sites.len(), count);
        for site in sites {
            let (file, span) = site.written.unwrap();
            assert_eq!(&quoted_text[span.start as usize..span.end as usize], name);
            assert_eq!(
                quoted
                    .select(Selector::Target {
                        file: &file,
                        byte: span.start
                    })
                    .unwrap()
                    .unwrap()
                    .id,
                binding.id
            );
        }
    }
}

// Trend declarations keep their TeX metadata and can use complete internal
// Rename without acquiring the external-function edit rule.
#[test]
fn trend_names_keep_metadata_and_internal_rename() {
    let model = view(
        "parameters g;g=1.01;trend_var(growth_factor=g) A $A_t$;var(deflator=A) y;model;y=0;end;",
    );
    let binding = choose(&model, "A", "trend_var");
    assert!(!binding.is_ordinary());
    assert_eq!(
        model
            .model
            .written_declarations
            .iter()
            .find(|row| row.declaration.name == model.model.trend_vars[0].name)
            .unwrap()
            .declaration
            .tex_name
            .as_deref(),
        Some("A_t")
    );
    let changed = edit(&model, &binding, "trend_level");
    assert!(changed.contains("trend_level $A_t$"));
    assert!(changed.contains("deflator=trend_level"));
}

// Subsample labels are keyed by their head and declaration epoch. Copies
// inherit one written definition; Rename closes over all copied uses.
#[test]
fn subsamples_keep_head_namespaces_and_copy_dependency_closure() {
    let text = "parameters a b;a=.5;b=.5;a.subsamples(short=2000Q1:2001Q1);a.short.options(init=.4);b.subsamples=a.subsamples;b.short.options(init=.6);";
    let model = view(text);
    let candidates = model
        .select(Selector::Name("short"))
        .unwrap_err()
        .candidates;
    assert_eq!(candidates.len(), 2);
    let source = candidates
        .iter()
        .find(|row| row.namespace.as_deref() == Some("subsamples(a)"))
        .unwrap();
    let copied = candidates
        .iter()
        .find(|row| row.namespace.as_deref() == Some("subsamples(b)"))
        .unwrap();
    assert_eq!(source.declaration, copied.declaration);
    let declaration = source.declaration.as_ref().unwrap();
    assert_eq!(
        model
            .select(Selector::Target {
                file: &declaration.0,
                byte: declaration.1.start
            })
            .unwrap()
            .unwrap()
            .id,
        source.id
    );
    let changed = edit(&model, copied, "early");
    assert!(changed.contains("subsamples(early="));
    assert!(changed.contains("a.early.options"));
    assert!(changed.contains("b.early.options"));
    assert!(!changed.contains("short"));
}

// A later declaration cannot bind a previously unresolved copy operand.
#[test]
fn subsample_copy_operands_do_not_acquire_later_definitions() {
    let model = view(
        "parameters a b;a=.5;b=.5;a.early.options=b.early.options;a.subsamples(early=2000Q1:2001Q1);",
    );
    let candidates: Vec<_> = model
        .bindings()
        .into_iter()
        .filter(|row| row.name == "early")
        .collect();
    let unresolved = candidates
        .iter()
        .find(|row| row.declaration.is_none())
        .unwrap();
    assert_eq!(
        rename(std::slice::from_ref(&model), 0, unresolved, "later")
            .unwrap_err()
            .code,
        "UNRESOLVED_SUBSAMPLE"
    );
}

// Repeated literal uses preserve execution identity and one written edit;
// generated declaration names remain read-only when literal proof is absent.
#[test]
fn named_macro_uses_deduplicate_edits_and_generated_names_refuse() {
    let text = "var y;model;[name='Y']y=.5*y(-1);end;var_model(model_name=vm,eqtags=['Y']);\n@#for i in 1:2\nvar_expectation_model(model_name=ve@{i},variable=y,auxiliary_model_name=vm,horizon=1);\n@#endfor\n";
    let model = view(text);
    let binding = choose(&model, "vm", "var_model");
    assert_eq!(model.references(&binding, true).len(), 3);
    assert_eq!(
        rename(std::slice::from_ref(&model), 0, &binding, "auxiliary")
            .unwrap()
            .edits
            .len(),
        2
    );
    let generated = choose(&model, "ve1", "var_expectation_model");
    assert_eq!(
        rename(std::slice::from_ref(&model), 0, &generated, "other")
            .unwrap_err()
            .code,
        "UNVERIFIED_TARGET"
    );
}

// Guards recovery tokens becoming accepted named declarations before the
// owning symbol-list/command callback has consumed its required terminator.
#[test]
fn refused_named_heads_have_no_accepted_declaration() {
    let base = "var y;parameters p;p=.9;model;[name='Y']y=0;end;";
    for tail in [
        "heterogeneity_dimension ghost",
        "heterogeneity_dimension ghost,;",
        "heterogeneity_dimension ghost 1;",
        "database ghost",
        "database ghost,;",
        "database ghost=1;",
        "var_model(model_name=ghost,",
        "var_model(model_name=ghost,eqtags=['Y'])",
        "trend_component_model(model_name=ghost,eqtags=['Y'],targets=['Y'],);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1",
        "pac_model(model_name=ghost,discount=p);",
    ] {
        // This last complete command is the positive control for the same
        // declaration spelling; all preceding forms lack a grammar callback.
        let model = view(&format!("{base}{tail}"));
        let selected = model.select(Selector::Name("ghost")).unwrap();
        assert_eq!(
            selected.is_some(),
            tail == "pac_model(model_name=ghost,discount=p);",
            "{tail}"
        );
    }
    for tail in [
        "heterogeneity_dimension ghost;",
        "database ghost;",
        "var_model(model_name=ghost,eqtags=['Y']);",
        "trend_component_model(model_name=ghost,eqtags=['Y'],targets=['Y']);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1);",
    ] {
        let model = view(&format!("{base}{tail}"));
        assert!(
            model
                .select(Selector::Name("ghost"))
                .unwrap()
                .unwrap()
                .declaration
                .is_some(),
            "{tail}"
        );
    }
    let earlier =
        view("heterogeneity_dimension kept;database kept_db;heterogeneity_dimension ghost");
    assert!(earlier.select(Selector::Name("kept")).unwrap().is_some());
    assert!(earlier.select(Selector::Name("kept_db")).unwrap().is_some());
    assert!(earlier.select(Selector::Name("ghost")).unwrap().is_none());
}

// Guards malformed operators publishing their model argument, while preserving
// an operator callback that completed before a later enclosing parse refusal.
#[test]
fn named_operator_and_opener_sites_follow_their_own_callbacks() {
    for operator in [
        "var_expectation",
        "pac_expectation",
        "pac_target_nonstationary",
    ] {
        for arguments in ["ghost,1)", "ghost", "ghost;", "ghost+1)"] {
            let model = view(&format!("var y;model;y={operator}({arguments};end;"));
            assert!(
                model.select(Selector::Name("ghost")).unwrap().is_none(),
                "{operator}({arguments}"
            );
            assert!(model.model.named_model_operators.is_empty());
        }
        let model = view(&format!("var y;model;y={operator}(ghost)+;end;"));
        let binding = model.select(Selector::Name("ghost")).unwrap().unwrap();
        assert_eq!(model.references(&binding, false).len(), 1);
    }
    let model = view("var_expectation_model(model_name=outer,expression=pac_expectation(inner),");
    assert!(model.select(Selector::Name("outer")).unwrap().is_none());
    assert!(model.select(Selector::Name("inner")).unwrap().is_some());
    for (text, expected) in [
        ("pac_target_info(ghost", false),
        ("pac_target_info(ghost)", false),
        ("pac_target_info(ghost);target", true),
    ] {
        assert_eq!(
            view(text)
                .select(Selector::Name("ghost"))
                .unwrap()
                .is_some(),
            expected,
            "{text}"
        );
    }
    for (tail, count) in [
        ("var(heterogeneity=h", 1),
        ("var(heterogeneity=h) x", 1),
        ("var(heterogeneity=h) x;", 2),
        ("model(heterogeneity=h", 1),
        ("model(heterogeneity=h)", 2),
        ("shocks(heterogeneity=h);var e;stderr 1;", 1),
        ("shocks(heterogeneity=h);var e;stderr 1;end;", 2),
    ] {
        let model = view(&format!("heterogeneity_dimension h;varexo e;{tail}"));
        let binding = choose(&model, "h", "heterogeneity_dimension");
        assert_eq!(model.references(&binding, true).len(), count, "{tail}");
    }
}

// Guards changing an external database lookup through an otherwise ordinary
// binding. Implicit data-column names and native lookalikes stay outside this rule.
#[test]
fn ordinary_database_field_rename_refuses_the_whole_edit() {
    let base = "var y;varexo e;database db;model;y=.5*y(-1)+e;end;";
    for tail in [
        "",
        "verbatim;db.y = y;end;",
        "data(file='observations.csv');",
        "shock_paths;var e;periods 1;values init.y;end;",
    ] {
        let model = view(&format!("{base}{tail}"));
        let binding = choose(&model, "y", "var");
        assert!(edit(&model, &binding, "z").contains("model;z=.5*z(-1)+e;end;"));
    }
    for tail in [
        "shock_paths;var e;periods 1;values db.y;end;",
        "shock_paths;var e;periods 1;values 2*db.y;end;",
    ] {
        let model = view(&format!("{base}{tail}"));
        let binding = choose(&model, "y", "var");
        assert!(model.references(&binding, false).iter().any(|site| {
            site.written.as_ref().is_some_and(|(_, span)| {
                model.sources["root.mod"].get(span.start as usize..span.end as usize) == Some("y")
                    && model.sources["root.mod"][..span.start as usize].ends_with("db.")
            })
        }));
        let refused = rename(std::slice::from_ref(&model), 0, &binding, "z").unwrap_err();
        assert_eq!(refused.code, "EXTERNAL_DATABASE_MEMBER");
        assert!(refused.message.contains("column"));
    }
}

// The installed pin must accept/refuse the same owning productions. Semantic
// stage checks stay separate: these probes stop at json=parse.
#[test]
fn named_callback_shapes_match_the_pinned_parse_stage() {
    use crate::test_support::preprocessor::{JsonStage, find_preprocessor, run_preprocessor};
    use std::time::Duration;
    let Some(binary) = find_preprocessor(None) else {
        return;
    };
    let base = "var y;parameters p;p=.9;model;[name='Y']y=0;end;";
    for (tail, accepted) in [
        ("heterogeneity_dimension ghost;", true),
        ("heterogeneity_dimension ghost other,third;", true),
        ("heterogeneity_dimension ghost", false),
        ("heterogeneity_dimension ghost,;", false),
        ("heterogeneity_dimension ghost 1;", false),
        ("database ghost;", true),
        ("database ghost other,third;", true),
        ("database ghost", false),
        ("database ghost,;", false),
        ("database ghost=1;", false),
        ("var_model(model_name=ghost,eqtags=['Y']);", true),
        ("var_model(model_name=ghost,", false),
        ("var_model(model_name=ghost,eqtags=['Y'])", false),
        (
            "trend_component_model(model_name=ghost,eqtags=['Y'],targets=['Y']);",
            true,
        ),
        (
            "trend_component_model(model_name=ghost,eqtags=['Y'],targets=['Y'],);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1);",
            true,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1",
            false,
        ),
        ("pac_model(model_name=ghost,discount=p);", true),
        ("pac_model(model_name=ghost,discount=p", false),
    ] {
        let source = format!("{base}{tail}");
        let official = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Parse,
        );
        assert_eq!(
            official.success, accepted,
            "{tail}: {} {}",
            official.raw_stdout, official.raw_stderr
        );
        let parsed = crate::engine::language::parser::parse(&source);
        let has_syntax = crate::engine::diagnostics::analyze(&parsed)
            .iter()
            .any(|diagnostic| diagnostic.code == "E001");
        assert_eq!(has_syntax, !accepted, "{tail}");
    }
    for operator in [
        "var_expectation",
        "pac_expectation",
        "pac_target_nonstationary",
    ] {
        for (argument, accepted) in [
            ("ghost)", true),
            ("ghost,1)", false),
            ("ghost", false),
            ("ghost+1)", false),
        ] {
            let source = format!("var y;model;y={operator}({argument};end;");
            let official = run_preprocessor(
                &source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Parse,
            );
            assert_eq!(
                official.success, accepted,
                "{operator}({argument}: {} {}",
                official.raw_stdout, official.raw_stderr
            );
        }
    }
}

// Guards complete grammar being mistaken for a successful driver callback.
// Required options, type checks, duplicate options and table insertion all run
// before a model name becomes an accepted definition.
#[test]
fn named_model_bindings_require_successful_semantic_parse_callbacks() {
    let base = "var y;parameters p;p=.9;model;[name='Y']y=0;end;";
    for command in [
        "var_model(model_name=ghost);",
        "var_model(model_name=ghost,model_name=ghost,eqtags=['Y']);",
        "trend_component_model(model_name=ghost,eqtags=['Y']);",
        "trend_component_model(model_name=ghost,targets=['Y']);",
        "var_expectation_model(model_name=ghost,auxiliary_model_name=aux,horizon=1);",
        "var_expectation_model(model_name=ghost,variable=y,expression=y,auxiliary_model_name=aux,horizon=1);",
        "var_expectation_model(model_name=ghost,variable=absent,auxiliary_model_name=aux,horizon=1);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1,discount=p+p);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1,time_shift=1);",
        "pac_model(model_name=ghost);",
        "pac_model(model_name=ghost,discount=y);",
        "pac_model(model_name=ghost,discount=absent);",
    ] {
        let model = view(&format!("{base}{command}"));
        assert!(!model.complete, "{command}");
        assert!(
            model.select(Selector::Name("ghost")).unwrap().is_none(),
            "{command}"
        );
        assert_eq!(
            model.model.semi_structural_commands.len(),
            1,
            "diagnostic receipt: {command}"
        );
    }
    for (kind, command) in [
        ("var_model", "var_model(model_name=ghost,eqtags=['Y']);"),
        (
            "trend_component_model",
            "trend_component_model(model_name=ghost,eqtags=['Y'],targets=['Y']);",
        ),
        (
            "var_expectation_model",
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1);",
        ),
        ("pac_model", "pac_model(model_name=ghost,discount=p);"),
    ] {
        let valid = view(&format!("{base}{command}"));
        assert!(valid.complete, "{command}");
        assert!(choose(&valid, "ghost", kind).declaration.is_some());
        let duplicate = view(&format!("{base}{command}{command}"));
        assert!(!duplicate.complete);
        let binding = choose(&duplicate, "ghost", kind);
        assert_eq!(
            duplicate.references(&binding, true).len(),
            1,
            "duplicate callback: {command}"
        );
    }
    let nested = view(&format!(
        "{base}var_expectation_model(model_name=outer,expression=pac_expectation(inner),auxiliary_model_name=aux,horizon=1,time_shift=1);"
    ));
    assert!(nested.select(Selector::Name("outer")).unwrap().is_none());
    assert!(nested.select(Selector::Name("inner")).unwrap().is_some());
}

// Guards partial prior/options heads and copy operands becoming accepted uses,
// and refused subsample declarations updating the label registry.
#[test]
fn subsample_labels_and_ordinary_heads_require_their_own_completed_callback() {
    let base = "parameters a b;a=.5;b=.5;var y;varexo e;a.subsamples(short=2000Q1:2001Q1);";
    for tail in [
        "a.short.options(",
        "a.short.options(init=.4)",
        "a.short.options(init=.4)garbage;",
        "a.short.prior(",
        "a.short.prior(shape=normal,mean=.5,stdev=.1)",
        "a.short.options=b.short.options",
        "a.short.prior=b.short.prior",
        "a.short.options=y.short.options;",
        "std(a).short.options(init=.4);",
    ] {
        let model = view(&format!("{base}{tail}"));
        let short = choose(&model, "short", "named_subsample");
        assert_eq!(model.references(&short, true).len(), 1, "{tail}");
        let binding = choose(&model, "a", "parameters");
        assert_eq!(model.references(&binding, true).len(), 3, "{tail}");
        assert!(!model.complete, "{tail}");
    }
    for tail in [
        "a.short.options(init=.4);",
        "a.short.prior(shape=normal,mean=.5,stdev=.1);",
        "a.short.options=b.short.options;",
        "a.short.prior=b.short.prior;",
    ] {
        let model = view(&format!("{base}{tail}"));
        assert!(model.complete, "{tail}");
        let source = model
            .bindings()
            .into_iter()
            .find(|row| row.name == "short" && row.namespace.as_deref() == Some("subsamples(a)"))
            .unwrap();
        assert_eq!(model.references(&source, true).len(), 2, "{tail}");
    }
    for tail in [
        "b.subsamples(ghost=2000Q1:2001Q1)",
        "b.subsamples(ghost=2000Q1:2001Q1,);",
        "b.subsamples(ghost=2000Q1:2001Q1,ghost=2002Q1:2003Q1);",
        "b.subsamples=absent.subsamples;",
        "b.subsamples=a.subsamples",
    ] {
        let model = view(&format!("{base}{tail}"));
        assert!(
            model.select(Selector::Name("ghost")).unwrap().is_none(),
            "{tail}"
        );
        assert!(!model.complete, "{tail}");
    }
    for text in [
        "heterogeneity_dimension d,d,ghost;",
        "database db,db,ghost;",
    ] {
        let model = view(text);
        assert!(!model.complete);
        assert!(model.select(Selector::Name("ghost")).unwrap().is_none());
        let first = if text.starts_with("database") {
            "db"
        } else {
            "d"
        };
        let binding = model.select(Selector::Name(first)).unwrap().unwrap();
        assert_eq!(model.references(&binding, true).len(), 1);
    }
}

// Macro copies with the same written range still have separate parser owners;
// one invalid expanded callback must not erase a successful earlier one.
#[test]
fn callback_owners_distinguish_macro_copies_and_forbidden_operator_contexts() {
    let source = "var y;parameters p;p=.9;model;[name='Y']y=0;end;\n@#for i in 1:2\n@#if i == 1\n@#define name = \"kept\"\n@#define opts = \"\"\n@#else\n@#define name = \"ghost\"\n@#define opts = \" ,model_name=ghost\"\n@#endif\nvar_model(model_name=@{name},eqtags=['Y']@{opts});\n@#endfor\n";
    let model = view(source);
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    assert!(model.select(Selector::Name("ghost")).unwrap().is_none());
    assert!(!model.complete);
    for text in [
        "parameters p;p=pac_expectation(ghost);",
        "var y;epilogue;z=pac_expectation(ghost);end;",
        "var y;occbin_constraints;name 'c';bind pac_expectation(ghost)>0;end;",
    ] {
        let model = view(text);
        assert!(
            model.select(Selector::Name("ghost")).unwrap().is_none(),
            "{text}"
        );
        assert!(!model.complete, "{text}");
    }
}

// Exact callback frontiers preserve successful nested actions before refusal
// and withhold named actions that the official parser can no longer execute.
#[test]
fn named_callback_frontier_keeps_only_completed_prefix_actions() {
    let base = "var y;parameters p;p=.9;model;[name='Y']y=0;end;";
    let model = view(&format!(
        "{base}var_expectation_model(model_name=outer,expression=pac_expectation(kept),auxiliary_model_name=aux,horizon=1,time_shift=1);database later;"
    ));
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    assert!(model.select(Selector::Name("outer")).unwrap().is_none());
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
    let model = view(&format!(
        "{base}var_expectation_model(model_name=outer,model_name=outer,expression=pac_expectation(later),auxiliary_model_name=aux,horizon=1);"
    ));
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
}

// Semantic callback refusal and grammar completion are separate facts. Probe
// all four model families at Parse, including accepted later-stage lookups.
#[test]
fn named_semantic_callbacks_match_the_pinned_parse_stage() {
    use crate::test_support::preprocessor::{JsonStage, find_preprocessor, run_preprocessor};
    use std::time::Duration;
    let Some(binary) = find_preprocessor(None) else {
        return;
    };
    let base = "var y;parameters p;p=.9;model;[name='Y']y=0;end;";
    for (command, accepted) in [
        ("var_model(model_name=ghost);", false),
        ("var_model(model_name=ghost,eqtags=['later_tag']);", true),
        (
            "trend_component_model(model_name=ghost,eqtags=['Y']);",
            false,
        ),
        (
            "trend_component_model(model_name=ghost,eqtags=['later_tag'],targets=['later_tag']);",
            true,
        ),
        (
            "var_expectation_model(model_name=ghost,auxiliary_model_name=aux,horizon=1);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,expression=y,auxiliary_model_name=aux,horizon=1);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=absent,auxiliary_model_name=aux,horizon=1);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1,discount=p+p);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1,time_shift=1);",
            false,
        ),
        (
            "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=later_model,horizon=1);",
            true,
        ),
        ("pac_model(model_name=ghost);", false),
        ("pac_model(model_name=ghost,discount=y);", false),
        ("pac_model(model_name=ghost,discount=absent);", false),
        (
            "pac_model(model_name=ghost,discount=p,auxiliary_model_name=later_model);",
            true,
        ),
    ] {
        let source = format!("{base}{command}");
        let official = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Parse,
        );
        assert_eq!(
            official.success, accepted,
            "{command}: {} {}",
            official.raw_stdout, official.raw_stderr
        );
        let model = view(&source);
        assert_eq!(
            model.select(Selector::Name("ghost")).unwrap().is_some(),
            accepted,
            "{command}"
        );
        assert_eq!(model.complete, accepted, "{command}");
    }
    for tail in [
        "a.short.options(",
        "a.short.options(init=.4)",
        "a.short.options=b.short.options",
        "b.subsamples(ghost=2000Q1:2001Q1,ghost=2002Q1:2003Q1);",
        "std(a).short.options(init=.4);",
    ] {
        let source = format!("parameters a b;a.subsamples(short=2000Q1:2001Q1);{tail}");
        let official = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Parse,
        );
        assert!(
            !official.success,
            "{tail}: {} {}",
            official.raw_stdout, official.raw_stderr
        );
    }
}

// An earlier row callback can refuse before end_heterogeneous_shocks looks up
// its dimension. Check-pass shock types must not block that Parse lookup.
#[test]
fn heterogeneous_shock_dimension_use_requires_successful_row_callbacks() {
    let base = "heterogeneity_dimension h;varexo(heterogeneity=h) e u;";
    for (tail, references, complete) in [
        ("shocks(heterogeneity=h);var e;stderr 1;end;", 3, true),
        (
            "shocks(heterogeneity=h);var e;stderr 1;var e;stderr 2;end;",
            2,
            false,
        ),
        (
            "shocks(heterogeneity=h);var e,u=.2;corr u,e=.3;end;",
            2,
            false,
        ),
        (
            "shocks(heterogeneity=h);var e;stderr 1;end;shocks(heterogeneity=h);var e;stderr 2;end;",
            4,
            true,
        ),
    ] {
        let model = view(&format!("{base}{tail}"));
        let dimension = choose(&model, "h", "heterogeneity_dimension");
        assert_eq!(
            model.references(&dimension, true).len(),
            references,
            "{tail}"
        );
        assert_eq!(model.complete, complete, "{tail}");
    }
    let model =
        view("heterogeneity_dimension h;parameters p;shocks(heterogeneity=h);var p;stderr 1;end;");
    assert_eq!(
        model
            .references(&choose(&model, "h", "heterogeneity_dimension"), true)
            .len(),
        2
    );
    assert!(model.complete, "shock symbol type is checked after Parse");
    let prior_lookup = view("heterogeneity_dimension h;var x;parameters(heterogeneity=h) x;");
    assert_eq!(
        prior_lookup
            .references(&choose(&prior_lookup, "h", "heterogeneity_dimension"), true)
            .len(),
        2
    );
    assert!(!prior_lookup.complete);
    let model = view("heterogeneity_dimension h;parameters(heterogeneity=h) dsge_prior_weight;");
    assert_eq!(
        model
            .references(&choose(&model, "h", "heterogeneity_dimension"), true)
            .len(),
        1
    );
    assert!(!model.complete);
}

// Repeated written ranges do not identify callback ownership: a later refused
// copy cannot erase the first use or replay its ordinary head as a second use.
#[test]
fn dotted_macro_callback_identity_survives_duplicate_options_and_kind_changes() {
    let source = "parameters a;\na.subsamples(short=2000Q1:2001Q1);\n@#for i in 1:2\n@#if i == 1\n@#define extra = \"\"\n@#else\n@#define extra = \",init=.5\"\n@#endif\na.short.options(init=.4@{extra});\n@#endfor\n";
    let model = view(source);
    assert_eq!(
        model
            .references(&choose(&model, "short", "named_subsample"), true)
            .len(),
        2
    );
    assert_eq!(
        model
            .references(&choose(&model, "a", "parameters"), true)
            .len(),
        3
    );
    assert!(!model.complete);
    let source = "parameters a;\n@#for i in 1:2\n@#if i == 1\n@#define command = \"options(init=.4)\"\n@#else\n@#define command = \"subsamples(short=2000Q1:2001Q1)\"\n@#endif\na.@{command};\n@#endfor\n";
    let model = view(source);
    assert!(model.complete);
    assert_eq!(
        model
            .references(&choose(&model, "short", "named_subsample"), true)
            .len(),
        1
    );
}

// Ordinary semantic Parse failures stop later named callbacks; unrelated
// Check/Transform failures leave Parse-complete accepted actions available.
#[test]
fn ordinary_semantic_parse_frontier_withholds_only_later_named_callbacks() {
    use crate::test_support::preprocessor::{JsonStage, find_preprocessor, run_preprocessor};
    use std::time::Duration;
    for (source, accepted) in [
        ("var y;y=1;database later;", false),
        ("var y;model;y=absent;end;database later;", false),
        ("var y;model;y=0;end;database later;", true),
        (
            "var y;varexo e;model;y=STEADY_STATE(e);end;database later;",
            false,
        ),
        (
            "var y;varexo e;model;#v=e;y=STEADY_STATE(v);end;database later;",
            false,
        ),
        (
            "var y;varexo e;model;y=STEADY_STATE(0*e);end;database later;",
            true,
        ),
        (
            "varexo e;shocks(learnt_in=0);var e;periods 1;values 1;end;database later;",
            false,
        ),
        ("var y;endval(learnt_in=0);y=0;end;database later;", false),
        (
            "parameters p;shocks;var p;periods 1;values 1;end;database later;",
            false,
        ),
        (
            "varexo e;shock_paths;var e;periods 1;values self.e;end;database later;",
            true,
        ),
        (
            "parameters p;var y;var_expectation_model(model_name=m,expression=absent,auxiliary_model_name=aux,horizon=1);database later;",
            true,
        ),
        (
            "parameters p;pac_model(model_name=m,discount=p,growth=absent);database later;",
            true,
        ),
    ] {
        let model = view(source);
        assert_eq!(
            model.select(Selector::Name("later")).unwrap().is_some(),
            accepted,
            "{source}"
        );
        assert_eq!(model.complete, accepted, "{source}");
        if let Some(binary) = find_preprocessor(None) {
            let official = run_preprocessor(
                source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Parse,
            );
            assert_eq!(
                official.success, accepted,
                "{source}: {} {}",
                official.raw_stdout, official.raw_stderr
            );
        }
    }
    let source = "var y;\n@#for i in 1:2\n@#if i == 1\n@#define command = \"database kept\"\n@#else\n@#define command = \"y=1\"\n@#endif\n@{command};\n@#endfor\ndatabase later;\n";
    let model = view(source);
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
    assert!(!model.complete);
}

// Recovery call nodes are useful to syntax hover, but cannot publish accepted
// named-operator callees or call references after a refused owning callback.
#[test]
fn named_operator_callee_queries_use_the_same_accepted_callback_gate() {
    for (name, valid, refused) in [
        ("log", "log(y)", "log(y,1)"),
        ("EXPECTATION", "EXPECTATION(-1)(y)", "EXPECTATION(-1)(y,1)"),
        ("STEADY_STATE", "STEADY_STATE(y)", "STEADY_STATE(y,1)"),
    ] {
        let good = view(&format!("var y;model;y={valid};end;"));
        assert_eq!(
            good.references(&choose(&good, name, "builtin_function"), true)
                .len(),
            1
        );
        let bad = view(&format!("var y;model;y={refused};end;"));
        assert!(
            bad.select(Selector::Name(name)).unwrap().is_none(),
            "{refused}"
        );
    }

    for operator in [
        "pac_expectation",
        "var_expectation",
        "pac_target_nonstationary",
    ] {
        let valid = view(&format!("var y;model;y={operator}(ghost);end;"));
        let binding = choose(&valid, operator, "builtin_function");
        assert_eq!(valid.references(&binding, true).len(), 1);
        for source in [
            format!("var y;model;y={operator}(ghost,1);end;"),
            format!("var y;epilogue;z={operator}(ghost);end;"),
        ] {
            let model = view(&source);
            assert!(
                model.select(Selector::Name(operator)).unwrap().is_none(),
                "{source}"
            );
        }
    }
}

// Different diagnostic presentation priorities cannot hide an earlier Parse
// action; later type changes cannot create an earlier heterogeneous read.
#[test]
fn parse_frontier_uses_all_candidates_and_captured_constructor_types() {
    let source = "heterogeneity_dimension d;var(heterogeneity=d)x;epilogue;a=x;end;var_model(model_name=late,eqtags=['e']);planner_objective x;";
    let model = view(source);
    assert!(model.select(Selector::Name("late")).unwrap().is_none());
    let cause = crate::engine::diagnostics::parse_preflight::first_refusal(&model.model).unwrap();
    assert!(cause.cause.contains("epilogue"));
    let source =
        "var x;epilogue;z=x;end;database kept;heterogeneity_dimension h;var(heterogeneity=h) x;";
    let model = view(source);
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    let cause = crate::engine::diagnostics::parse_preflight::first_refusal(&model.model).unwrap();
    assert!(!cause.cause.contains("epilogue"));
    for source in [
        "var y;initval;end=0;end;database later;",
        "parameters p;p=\"x\";database later;",
        "parameters a;a.options(init=.4) trailing;database later;",
        "parameters p;std(p).prior(shape=normal,mean=0,stdev=1);database later;",
    ] {
        let model = view(source);
        assert!(
            model.select(Selector::Name("later")).unwrap().is_none(),
            "{source}"
        );
        let cause =
            crate::engine::diagnostics::parse_preflight::first_refusal(&model.model).unwrap();
        assert!(!cause.cause.is_empty(), "{source}");
    }
}

// A VAREXP variable is an ordinary lookup, distinct from its model_name link;
// it is present in References and the complete Rename relation.
#[test]
fn var_expectation_variable_option_has_an_ordinary_lookup_site() {
    let source = "var y;model;[name='Y']y=0;end;var_expectation_model(model_name=m,variable=y,auxiliary_model_name=aux,horizon=1);";
    let model = view(source);
    let y = choose(&model, "y", "var");
    assert_eq!(model.references(&y, true).len(), 3);
    assert!(edit(&model, &y, "z").contains("variable=z"));
    let partial = view("var y;var_expectation_model(model_name=m,variable=y,");
    assert_eq!(
        partial
            .references(&choose(&partial, "y", "var"), true)
            .len(),
        1
    );
}

// Accepted symbol history stops at the first Parse action. A later recovery
// declaration cannot change an earlier binding's type or add a new binding.
#[test]
fn ordinary_and_local_queries_use_the_same_accepted_parse_prefix() {
    let model = view(
        "parameters p;std(p).prior(shape=normal,mean=0,stdev=1);var z;model;#late=1;z=late;end;",
    );
    assert!(model.select(Selector::Name("z")).unwrap().is_none());
    assert!(model.select(Selector::Name("late")).unwrap().is_none());
    let model = view("var y;parameters y;model;y=0;end;");
    let y = choose(&model, "y", "var");
    assert_eq!(model.references(&y, true).len(), 1);
    let model = view("var y;parameters kept y;database later;");
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    assert_eq!(choose(&model, "y", "var").kind, "var");
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
}

// A valid inner call remains accepted at a later syntax refusal's lookahead;
// builtin case variants resolve to one language identity.
#[test]
fn completed_call_receipts_keep_case_and_lookahead_boundaries() {
    let model = view("var y;model;y=log(y)+LOG(y);end;");
    let log = model.select(Selector::Name("LOG")).unwrap().unwrap();
    assert_eq!(model.references(&log, true).len(), 2);
    let model = view("var y;model;y=log(y) end;");
    let log = model.select(Selector::Name("log")).unwrap().unwrap();
    assert_eq!(model.references(&log, true).len(), 1);
    assert!(model.model.equations.is_empty());
    assert!(model.model.written_equations.is_empty());
}

// SUM checks constructed arguments and every nested callback. A raw outer
// form cannot hide an earlier inner refusal or reject Parse simplification.
#[test]
fn nested_sum_callbacks_use_constructed_argument_and_execution_order() {
    let model = view("var y;model;y=SUM(SUM(y)+pac_expectation(ghost));end;");
    assert!(model.select(Selector::Name("ghost")).unwrap().is_none());
    let cause = crate::engine::diagnostics::parse_preflight::first_refusal(&model.model).unwrap();
    assert!(cause.cause.contains("heterogeneous endogenous"));
    for argument in ["x+0", "1*x", "x-x+x"] {
        let model = view(&format!(
            "heterogeneity_dimension h;var(heterogeneity=h)x;var y;model;y=SUM({argument});end;database kept;"
        ));
        assert!(model.complete, "{argument}");
        assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    }
    let model = view(
        "heterogeneity_dimension h;var(heterogeneity=h)x;var y;model;y=SUM(x(-1)+0);end;database later;",
    );
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
}

// Endogenous tags declare or change the symbol only after a complete equation.
// OccBin generated parameters enter the type table but have no written name.
#[test]
fn equation_tag_callbacks_update_symbol_types_without_fabricated_source() {
    for source in [
        "model;[endogenous='y']y=0;end;database kept;",
        "parameters y;model;[endogenous='y']y=0;end;database kept;",
    ] {
        let model = view(source);
        assert!(model.complete, "{source}");
        assert_eq!(choose(&model, "y", "var").kind, "var");
        assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    }
    let model = view("model;[endogenous='ghost']ghost=0 end;database later;");
    assert!(model.select(Selector::Name("ghost")).unwrap().is_none());
    let model = view("var y;model;[name='e',bind='c']y=0;end;var occbin_c_bind;database later;");
    let generated = choose(&model, "occbin_c_bind", "parameters");
    assert!(generated.declaration.is_none());
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
    assert!(
        crate::engine::diagnostics::parse_preflight::first_refusal(&model.model)
            .unwrap()
            .cause
            .contains("different types")
    );
    let model = view(
        "heterogeneity_dimension h;varexo(heterogeneity=h)e;var y;model;y=STEADY_STATE(e);end;database kept;",
    );
    assert!(model.complete);
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
}

// Every direct grammar refusal keeps its actual callback and cause. A later
// unknown external call must preserve its successful nested operator callback.
#[test]
fn shape_refusal_receipts_preserve_prior_callbacks_and_explain_the_stop() {
    let model = view("var y;model;y=foo(pac_expectation(kept));end;database later;");
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    assert!(model.select(Selector::Name("foo")).unwrap().is_none());
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
    let cause = crate::engine::diagnostics::parse_preflight::first_refusal(&model.model).unwrap();
    assert!(cause.cause.contains("external function"));
    let source = "@#for i in 1:2\n@#if i == 1\n@#define command = \"database kept\"\n@#else\n@#define command = \"stoch_simul(irf=-1)\"\n@#endif\n@{command};\n@#endfor\ndatabase later;";
    let model = view(source);
    assert!(model.select(Selector::Name("kept")).unwrap().is_some());
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
    assert!(crate::engine::diagnostics::parse_preflight::first_refusal(&model.model).is_some());
    let model = view("svar_identification;exclusion lag 1;end;database later;");
    assert!(model.select(Selector::Name("later")).unwrap().is_none());
    assert!(
        !crate::engine::diagnostics::parse_preflight::first_refusal(&model.model)
            .unwrap()
            .cause
            .is_empty()
    );
}
