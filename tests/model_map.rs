use std::collections::{HashMap, HashSet};

use dygnosis::expand::expand_report;
use dygnosis::model::{AssignmentIndex, ExecutionStep, StatementKind};
use dygnosis::model_map::{BLOCK_CATEGORIES, SUPPORTED_BLOCKS};
use dygnosis::{parse, Workspace};

#[test]
fn records_execution_order_and_every_model_opener() {
    let text = "parameters p; p=1; var y; model; #a=p; [name='first'] y=a; end;\n@#for k in 1:2\nmodel; [name='copy'] y=@{k}; end;\n@#endfor\nsteady;";
    let model = parse(text);
    let names: Vec<_> = model
        .statements
        .iter()
        .map(|row| row.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "parameters",
            "p",
            "var",
            "model",
            "model",
            "model",
            "steady"
        ]
    );
    assert_eq!(
        model.statements[1].assignment,
        Some(AssignmentIndex::Parameter(0))
    );
    assert_eq!(model.statements[4].span, model.statements[5].span);
    let report = expand_report(text);
    assert_eq!(
        report
            .model_map
            .equations
            .iter()
            .filter_map(|row| row.number)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(report.model_map.equations[0].local);
    assert_eq!(report.model_map.equations[0].number, None);
    assert_eq!(
        report.model_map.equations[2].source.segments,
        report.model_map.equations[3].source.segments
    );
    assert_ne!(
        report.model_map.equations[2].id,
        report.model_map.equations[3].id
    );
    assert_eq!(
        report.model_map.equations[2].source.origin_frames[0]
            .value
            .as_deref(),
        Some("1")
    );
    assert_eq!(
        report.model_map.equations[3].source.origin_frames[0]
            .value
            .as_deref(),
        Some("2")
    );
}

#[test]
fn registry_covers_every_existing_block_branch_and_variant() {
    let mut seen = HashSet::new();
    for &name in SUPPORTED_BLOCKS {
        let opener = if name == "pac_target_info" {
            "pac_target_info(p)"
        } else {
            name
        };
        let text = format!("{opener};\nend;\n");
        let model = parse(&text);
        let row = model
            .statements
            .iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert_eq!(row.kind, StatementKind::Block, "{name}");
        assert!(row.complete, "{name}");
        seen.insert(row.category.clone().unwrap());
    }
    for (opener, category) in [
        ("model(heterogeneity=hh)", "model.heterogeneous"),
        ("endval(learnt_in=1)", "endval.learnt_in"),
        ("shocks(surprise)", "shocks.surprise"),
        ("shocks(learnt_in=1)", "shocks.learnt_in"),
        ("shocks(heterogeneity=hh)", "shocks.heterogeneous"),
        ("mshocks(learnt_in=1)", "mshocks.learnt_in"),
        ("shock_paths(learnt_in=1)", "shock_paths.learnt_in"),
        (
            "perfect_foresight_controlled_paths(learnt_in=1)",
            "perfect_foresight_controlled_paths.learnt_in",
        ),
    ] {
        let model = parse(&format!("{opener};\nend;\n"));
        assert_eq!(
            model.statements[0].category.as_deref(),
            Some(category),
            "{opener}"
        );
        seen.insert(category.to_string());
    }
    assert_eq!(
        seen,
        BLOCK_CATEGORIES
            .iter()
            .map(|(key, _)| key.to_string())
            .collect()
    );
    // These defaults are consumed by native Settings. Compare the manifest
    // with the parsed categories, without depending on Rust source spelling.
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("editors/vscode/package.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let settings: HashMap<_, _> = manifest["contributes"]["configuration"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|group| group["properties"].as_object().unwrap())
        .filter_map(|(key, value)| {
            let category = key.strip_prefix("dynare.blockTint.")?;
            if value["type"] != "string" {
                return None;
            }
            Some((category, value["default"].as_str().unwrap()))
        })
        .collect();
    assert_eq!(settings, BLOCK_CATEGORIES.iter().copied().collect());
}

#[test]
fn removed_static_local_and_replacement_rows_keep_written_structure() {
    let text = "var y z; model; #a=1; [name='old'] y=a; [static] z=0; [dynamic,name='keep'] z=z(-1); end; model_replace('old'); [name='new'] y=2; end;";
    let report = expand_report(text);
    let rows = &report.model_map.equations;
    assert_eq!(rows.len(), 5);
    assert!(!rows[1].active);
    assert_eq!(rows[1].number, None);
    assert_eq!(rows[2].number, None);
    assert!(rows[2].static_only);
    assert_eq!(rows[3].number, Some(1));
    assert_eq!(rows[4].number, Some(2));
    assert_ne!(rows[3].statement_id, rows[4].statement_id);
}

#[test]
fn dimensions_have_separate_surviving_sequences() {
    let report = expand_report("heterogeneity_dimension hh ff; var Y; var(heterogeneity=hh) c; var(heterogeneity=ff) q; model(heterogeneity=hh); c=0; end; model; Y=0; end; model(heterogeneity=ff); q=0; end; model(heterogeneity=hh); c=1; end;");
    assert_eq!(
        report
            .model_map
            .equations
            .iter()
            .map(|row| (row.dimension.as_deref(), row.number))
            .collect::<Vec<_>>(),
        [
            (Some("hh"), Some(1)),
            (None, Some(1)),
            (Some("ff"), Some(1)),
            (Some("hh"), Some(2))
        ]
    );
}

#[test]
fn declaration_owners_and_opaque_barriers_keep_final_type_queries() {
    let model = parse("var y $Y$ (long_name='Output'); parameters p; change_type(var) p; mystery(); p=2; model; p=y; end;");
    let decl = &model.written_declarations[1];
    assert_eq!(decl.written_kind, "parameters");
    assert_eq!(decl.statement_id, 1);
    assert_eq!(model.final_symbol_kind(decl.declaration.name), Some("var"));
    assert_eq!(
        model.written_declarations[0]
            .declaration
            .long_name
            .as_deref(),
        Some("Output")
    );
    assert_eq!(
        model.written_declarations[0]
            .declaration
            .tex_name
            .as_deref(),
        Some("Y")
    );
    assert!(model
        .execution_steps
        .iter()
        .any(|step| matches!(step, ExecutionStep::Opaque(_))));
    assert!(!model
        .statements
        .iter()
        .any(|statement| statement.name == "mystery"));
    let functions = parse("external_function(name=f,nargs=1,first_deriv_provided=df); model_local_variable a; trend_var(growth_factor=1) T;");
    assert_eq!(
        functions
            .written_declarations
            .iter()
            .map(|row| functions.name(row.declaration.name))
            .collect::<Vec<_>>(),
        ["f", "df", "a", "T"]
    );
    assert_eq!(functions.statements[0].kind, StatementKind::Declaration);
}

#[test]
fn retained_native_scalar_helpers_have_ordered_links_but_calls_stay_opaque() {
    let model = parse("helper=2;\nparameters p;\np=helper+1;\nnative_call();\n");
    assert_eq!(model.statements[0].kind, StatementKind::Assignment);
    assert_eq!(
        model.statements[0].assignment,
        Some(AssignmentIndex::Helper(0))
    );
    assert!(model.statements[0].native);
    assert_eq!(
        model.statements[2].assignment,
        Some(AssignmentIndex::Parameter(0))
    );
    assert!(matches!(
        model.execution_steps.last(),
        Some(ExecutionStep::Opaque(_))
    ));
}

#[test]
fn assignment_and_dotted_command_names_keep_written_symbol_case() {
    let model = parse("parameters Alpha; Alpha=1; Alpha.prior(shape=normal_pdf,mean=0,stdev=1);\n");
    assert_eq!(model.statements[1].name, "Alpha");
    assert_eq!(model.statements[2].kind, StatementKind::Command);
    assert_eq!(model.statements[2].name, "Alpha.prior");
    let model = parse("parameters Alpha;\n@#for j in 1:2\n@#if j == 1\n@#define action = \"Alpha.prior(shape=normal_pdf,mean=0,stdev=1)\"\n@#else\n@#define action = \"data(file=values.prior)\"\n@#endif\n@{action};\n@#endfor\n");
    assert_eq!(
        model
            .statements
            .iter()
            .map(|statement| statement.name.as_str())
            .collect::<Vec<_>>(),
        ["parameters", "Alpha.prior", "data"]
    );
    assert_eq!(model.statements[1].span, model.statements[2].span);
    assert_eq!(model.dotted_statements.len(), 1);
    assert_eq!(model.data_statements.len(), 1);
}

#[test]
fn partial_blocks_do_not_fabricate_closers_or_numbers() {
    let model = parse("var y; model; y=0; initval; y=1;");
    assert!(!model.statements[1].complete);
    assert!(!model.statements[2].complete);
    let partial = expand_report("var y; model; y=0; initval; y=1;");
    assert!(!partial.model_map.complete);
    assert!(partial
        .model_map
        .equations
        .iter()
        .all(|row| row.number.is_none()));
    let report = expand_report("var y; @#if unavailable\nmodel; y=0; end;\n@#endif\n");
    assert!(!report.complete);
    assert!(report
        .model_map
        .equations
        .iter()
        .all(|row| row.number.is_none()));
}

#[test]
fn split_blocks_map_only_verified_segments() {
    let mut workspace = Workspace::new();
    workspace.update_document(
        "C:/dygnosis-map/root.mod",
        "var y; model;\n@#include \"body.inc\"\nend;",
    );
    workspace.update_document("C:/dygnosis-map/body.inc", "y=0;\n");
    let source;
    {
        let report = workspace.expand_report("C:/dygnosis-map/root.mod").unwrap();
        source = report.model_map.statements[1].clone();
        assert_eq!(source.segments.len(), 3);
        assert!(source
            .anchor
            .as_ref()
            .unwrap()
            .file
            .as_ref()
            .unwrap()
            .ends_with("root.mod"));
        assert!(report.model_map.equations[0].source.segments[0]
            .file
            .as_ref()
            .unwrap()
            .ends_with("body.inc"));
    }
    for segment in &source.segments {
        let text = workspace
            .get_source(segment.file.as_deref().unwrap())
            .unwrap();
        assert!(text
            .get(segment.span.start as usize..segment.span.end as usize)
            .is_some());
    }
}

#[test]
fn cross_file_rows_and_type_events_keep_independent_occurrences() {
    let mut workspace = Workspace::new();
    workspace.update_document(
        "C:/dygnosis-map/cross.mod",
        "var y; model; y=\n@#include \"rhs.inc\"\n;end;",
    );
    workspace.update_document("C:/dygnosis-map/rhs.inc", "1\n");
    let row = workspace
        .expand_report("C:/dygnosis-map/cross.mod")
        .unwrap()
        .model_map
        .equations[0]
        .clone();
    assert_eq!(row.source.segments.len(), 2);
    for segment in &row.source.segments {
        let text = workspace
            .get_source(segment.file.as_deref().unwrap())
            .unwrap();
        assert!(text
            .get(segment.span.start as usize..segment.span.end as usize)
            .is_some());
    }
    let text = "@#for k in 1:2\nvar y;\n@#endfor\nmodel; y=0; end;\nnative_function()";
    let report = expand_report(text);
    let events = &report.model_map.type_events;
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].1.segments, events[1].1.segments);
    assert_eq!(events[0].1.origin_frames[0].value.as_deref(), Some("1"));
    assert_eq!(events[1].1.origin_frames[0].value.as_deref(), Some("2"));
    let model = parse(text);
    assert!(matches!(
        model.execution_steps.last(),
        Some(ExecutionStep::Opaque(_))
    ));
}

#[test]
fn mcp_model_info_withholds_partial_include_graph_counts() {
    let root = "C:/dygnosis-model-info/root.mod";
    let child = "C:/dygnosis-model-info/child.inc";
    let text = "@#include \"child.inc\"\nvar y; model; y=0; end;\n";
    let mut files = HashMap::from([(root.to_string(), text.to_string())]);
    for body in [None, Some("@#include \"root.mod\"\n")] {
        if let Some(body) = body {
            files.insert(child.to_string(), body.to_string());
        }
        let info = dygnosis::dynare_model_info(text, Some(root), Some(&files));
        assert_eq!(info["status"], "incomplete");
        assert!(info.get("n_equations").is_none());
        assert!(info.get("endogenous").is_none());
    }
    files.insert(child.to_string(), "parameters p; p=1;\n".to_string());
    let info = dygnosis::dynare_model_info(text, Some(root), Some(&files));
    assert!(info.get("status").is_none());
    assert_eq!(info["n_equations"], 1);
    assert_eq!(info["n_endogenous"], 1);
    assert_eq!(info["parameters"], serde_json::json!(["p"]));
}

#[test]
fn mcp_model_info_preserves_supplied_active_and_content_overlay_routes() {
    let root = "C:/dygnosis-model-info/overlay.mod";
    let child = "C:/dygnosis-model-info/params.inc";
    let content = "@#include \"params.inc\"\nvar y; model; y=p; end;\n";
    let mut files = HashMap::from([
        (root.to_string(), "@#include \"absent.inc\"\n".to_string()),
        (child.to_string(), "parameters p; p=1;\n".to_string()),
    ]);
    let with_key = dygnosis::dynare_model_info(content, Some(root), Some(&files));
    assert_eq!(with_key["n_parameters"], 1);
    assert!(with_key.get("status").is_none());
    files.remove(root);
    // The 0.10.2 model-info route overlays a supplied active name even when
    // that name is absent from the map. An absent active argument is free text.
    let without_key = dygnosis::dynare_model_info(content, Some(root), Some(&files));
    assert_eq!(without_key, with_key);
    let without_active = dygnosis::dynare_model_info(content, None, Some(&files));
    assert_eq!(without_active["status"], "incomplete");
    assert!(without_active.get("n_equations").is_none());
    let plain =
        dygnosis::dynare_model_info("parameters p; var y; model; y=p; end;", None, Some(&files));
    assert_eq!(plain["n_parameters"], 1);
    assert_eq!(plain["n_equations"], 1);
}
