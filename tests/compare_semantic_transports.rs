//! The same retained row shapes and field limits survive each compare adapter.

use std::collections::BTreeMap;

use dygnosis::compare_snapshots::{
    capture_git_snapshot, compare_captured_snapshots, CapturedSnapshot, GitSnapshotInput,
    ManifestEntry, SnapshotCapture, SnapshotCoordinates, SourceFact,
};
use dygnosis::dynare_compare_models;
use dygnosis::server::{new_service, Backend};
use serde_json::{json, Value};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const BASE: &str = "var y c x v; varexo e u; parameters a b; a=.5; b=.9; model; y=a*y(-1)+e; c=y; x=c+u; v=x; end;\n";

// Guards counted instruction context repeating a command or an added block
// already represented by its retained options or body facts.
#[test]
fn represented_instructions_do_not_add_redundant_context_rows() {
    let base = "var y c; parameters a; a=.5; model; y=a; c=y; end;\n";
    for (name, instruction, count) in [
        (
            "estimation",
            "estimation(datafile='observations.csv', nobs=120, order=1);",
            1,
        ),
        (
            "estimated_params",
            "estimated_params; a, beta_pdf, 0.6, 0.1; end;",
            1,
        ),
        (
            "steady_state_model",
            "steady_state_model; y=0; c=0; end;",
            2,
        ),
        ("estimated_params", "estimated_params; end;", 1),
        (
            "estimated_params",
            "estimated_params(overwrite); a, beta_pdf, 0.6, 0.1; end;",
            2,
        ),
        ("initval", "initval(all_values_required); y=0; c=0; end;", 3),
    ] {
        let next = format!("{base}{instruction}");
        for (before, after) in [(base, next.as_str()), (next.as_str(), base)] {
            let result = dynare_compare_models(before, after, None, None, None, None, None);
            let rows: Vec<_> = result["semantic"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| {
                    ["before", "after"]
                        .iter()
                        .any(|side| row[side]["context"]["name"] == name)
                })
                .collect();
            assert_eq!(rows.len(), count, "{name}: {rows:?}");
        }
    }
}

// Guards residual-only option edits reaching the same command owner while
// retaining named data options and preserving token-level highlights.
#[test]
fn estimation_option_changes_have_one_complete_written_command_owner() {
    let before = format!("{BASE}estimation(datafile='observations.csv', nobs=120, order=1);");
    for (from, to) in [("order=1", "order=2"), ("observations.csv", "new.csv")] {
        let after = before.replace(from, to);
        let result = dynare_compare_models(&before, &after, None, None, None, None, None);
        let rows: Vec<_> = result["semantic"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| {
                ["before", "after"]
                    .iter()
                    .any(|side| row[side]["context"]["name"] == "estimation")
            })
            .collect();
        assert_eq!(rows.len(), 1, "{from}: {rows:?}");
        let text = rows[0]["expressions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|expression| expression["field"] == "statement_text")
            .unwrap();
        assert!(text["after"]["text"].as_str().unwrap().contains(to));
        assert!(rows[0]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "data_options"));
        if from == "order=1" {
            let edits: String = text["after"]["runs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|run| run["role"] == "added")
                .map(|run| run["text"].as_str().unwrap())
                .collect();
            assert_eq!(edits, "2");
        }
    }
}

// Guards declaration presence independently from the membership of each name.
#[test]
fn observable_keywords_follow_the_accepted_declaration_correspondence() {
    for (keyword, names) in [("varobs", ["y", "c"]), ("varexobs", ["e", "u"])] {
        let complete = format!("{BASE}{keyword} {} {};", names[0], names[1]);
        let existing = format!("{BASE}{keyword} {};", names[0]);
        for (before, after, active, role) in [
            (BASE, complete.as_str(), "after", "added"),
            (complete.as_str(), BASE, "before", "removed"),
            (existing.as_str(), complete.as_str(), "after", "unchanged"),
            (complete.as_str(), existing.as_str(), "before", "unchanged"),
        ] {
            let result = dynare_compare_models(before, after, None, None, None, None, None);
            let rows = result["semantic"]["rows"].as_array().unwrap();
            let row = rows
                .iter()
                .find(|row| row["family"] == "observables" && row["name"] == names[1])
                .unwrap();
            let text = row["expressions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|expression| expression["field"] == "statement_text")
                .unwrap();
            let run = &text[active]["runs"][0];
            assert!(run["text"].as_str().unwrap().starts_with(keyword));
            assert_eq!(run["role"], role, "{keyword}: {text}");
        }
    }
}

// Guards a multi-name observable list producing one owner per name and no
// additional Commands owner for the already represented declaration keyword.
#[test]
fn observable_lists_have_only_their_individual_name_owners() {
    for (keyword, names) in [("varobs", ["y", "c"]), ("varexobs", ["e", "u"])] {
        let next = format!("{BASE}{keyword} {} {};", names[0], names[1]);
        for (before, after, change) in [
            (BASE, next.as_str(), "added"),
            (next.as_str(), BASE, "removed"),
        ] {
            let result = dynare_compare_models(before, after, None, None, None, None, None);
            let rows: Vec<_> = result["semantic"]["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| {
                    ["before", "after"]
                        .iter()
                        .any(|side| row[side]["context"]["name"] == keyword)
                })
                .collect();
            assert_eq!(rows.len(), names.len(), "{keyword} {change}: {rows:?}");
            assert!(rows
                .iter()
                .all(|row| row["family"] == "observables" && row["change"] == change));
            assert_eq!(
                rows.iter()
                    .map(|row| row["name"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                names
            );
        }
    }
}

// Guards the consumed change-type contract while retaining independent moment
// occurrences and cancelling the unchanged product moment.
#[test]
fn uncertain_moments_use_only_added_and_removed_change_types() {
    let before = format!("{BASE}matched_moments; y; c*y; y*y(-1); end;");
    let after = format!("{BASE}matched_moments; y*y; c*y; y*y(-2); c*c(-1); end;");
    let result = dynare_compare_models(&before, &after, None, None, None, None, None);
    let rows: Vec<_> = result["semantic"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["family"] == "moments")
        .collect();
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows.iter()
            .filter(|row| row["change"] == "removed" && row["after"].is_null())
            .count(),
        2
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row["change"] == "added" && row["before"].is_null())
            .count(),
        3
    );
    assert!(rows.iter().all(
        |row| row["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "expression"
                && field["before"]["value"]["value"] != "c*y"
                && field["after"]["value"]["value"] != "c*y")
    ));
}

fn captured(id: &str, text: &str) -> CapturedSnapshot {
    let input = GitSnapshotInput {
        input_id: id.into(),
        root_file: "root.mod".into(),
        repository_uri: "file:///transport-agreement".into(),
        commit: if id == "before" {
            "a".repeat(40)
        } else {
            "b".repeat(40)
        },
        requested_ref: None,
        search_paths: Vec::new(),
        manifest: BTreeMap::from([(
            "root.mod".into(),
            ManifestEntry {
                mode: "100644".into(),
                object_id: "c".repeat(40),
            },
        )]),
        sources: BTreeMap::from([("root.mod".into(), SourceFact::Text { text: text.into() })]),
    };
    match capture_git_snapshot(&input) {
        SnapshotCapture::Ready(snapshot) => *snapshot,
        _ => panic!("complete fixture refused"),
    }
}

async fn current(backend: &Backend, old: &str, new: &str) -> Value {
    let before = Url::parse("untitled:transport-before.mod").unwrap();
    let after = Url::parse("untitled:transport-after.mod").unwrap();
    for (uri, text) in [(&before, old), (&after, new)] {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: text.into(),
                },
            })
            .await;
    }
    backend
        .execute_command(ExecuteCommandParams {
            command: "dynare/compareModels".into(),
            arguments: vec![json!({"uri_a":before,"uri_b":after})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn retained_families_and_prior_shapes_agree_across_current_and_snapshot_coordinates() {
    let cases = [
        ("parameters", "a=.5;", "a=.6;"),
        ("symbols", "var(log) z;", "var z;"),
        (
            "equations",
            "model; [name='extra'] v=x+1; end;",
            "model; [name='extra'] v=x+2; end;",
        ),
        (
            "regime_equations",
            "model; [name='policy',bind='ELB'] y=1; [name='policy',relax='ELB'] y=y(-1); end;",
            "model; [name='policy',bind='ELB'] y=0; [name='policy',relax='ELB'] y=y(-1); end;",
        ),
        (
            "shocks",
            "shocks; var e; stderr 1; end;",
            "shocks; var e; stderr 2; end;",
        ),
        ("locals", "model; #q=1; end;", "model; #q=2; end;"),
        (
            "steady_state",
            "steady_state_model; y=1; end;",
            "steady_state_model; y=2; end;",
        ),
        (
            "initialization",
            "histval; y(-1)=1; end;",
            "histval; y(-2)=1; end;",
        ),
        (
            "priors",
            "estimated_params; a,,0,1,normal_pdf,,.1,,,1; end;",
            "estimated_params; a,,0,1,normal_pdf,,.2,,,2; end;",
        ),
        (
            "dotted_prior",
            "a.prior(shape=normal,mean=0.5,stdev=0.1);",
            "a.prior(shape=normal,mean=0.6,stdev=0.1);",
        ),
        (
            "joint_prior",
            "[a,b].prior(shape=normal,mean=[0.5,0.9],variance=[[1,0],[0,1]]);",
            "[a,b].prior(shape=normal,mean=[0.6,0.9],variance=[[1,0],[0,1]]);",
        ),
        ("prior_copy", "a.prior=b.prior;", "a.prior=a.prior;"),
        (
            "prior_init",
            "estimated_params_init; a,.5; end;",
            "estimated_params_init(use_calibration); a,.6; end;",
        ),
        (
            "prior_bounds",
            "estimated_params_bounds; a,0,1; end;",
            "estimated_params_bounds; a,0,2; end;",
        ),
        (
            "prior_remove",
            "estimated_params_remove; a; end;",
            "estimated_params_remove; b; end;",
        ),
        (
            "prior_options",
            "a.options(init=.5);",
            "a.options(init=.6);",
        ),
        ("commands", "stoch_simul(order=1);", "stoch_simul(order=2);"),
        ("observables", "varobs y;", "varobs c;"),
        ("dates", "set_time(2000Q1);", "set_time(2000Q2);"),
        (
            "data",
            "data(file='a.csv',nobs=10);",
            "data(file='b.csv',nobs=10);",
        ),
        (
            "subsamples",
            "a.subsamples(s=2000Q1:2001Q1);",
            "a.subsamples(s=2000Q1:2001Q2);",
        ),
        (
            "occbin",
            "occbin_constraints; name 'ELB'; bind y<0; relax y>1; end;",
            "occbin_constraints; name 'ELB'; bind y<.1; relax y>1; end;",
        ),
        ("policy", "planner_objective y^2;", "planner_objective y^3;"),
        (
            "policy_weights",
            "optim_weights; y,c 1; end;",
            "optim_weights; y,c 2; end;",
        ),
        (
            "semi_structural",
            "var_model(model_name=aux,eqtags=['y','c']);",
            "var_model(model_name=aux,eqtags=['c','y']);",
        ),
        (
            "pac",
            "pac_target_info(pac); target v; component y; kind ll; auxname ya; end;",
            "pac_target_info(pac); target v; component y; kind dl; auxname ya; end;",
        ),
        (
            "moments",
            "matched_moments; y*y(-1); end;",
            "matched_moments; y*y(-2); end;",
        ),
        (
            "irf_weights",
            "matched_irfs_weights; y(1),e,c(2),u,.5; end;",
            "matched_irfs_weights; y(1),e,c(2),u,.6; end;",
        ),
        (
            "ms_sbvar",
            "sbvar(coefficient_prior_hyperparameters=[1,2,3]);",
            "sbvar(coefficient_prior_hyperparameters=[1,3,2]);",
        ),
        (
            "identification",
            "svar_identification; exclusion lag 0; equation 1,y,c; end;",
            "svar_identification; exclusion lag 1; equation 1,y,c; end;",
        ),
        (
            "heterogeneity",
            "heterogeneity_compute_steady_state(filename='a.mat');",
            "heterogeneity_compute_steady_state(filename='b.mat');",
        ),
        (
            "external_functions",
            "external_function(name=f);",
            "external_function(name=f,nargs=1);",
        ),
        (
            "trends",
            "trend_var(growth_factor=1.02) A;",
            "trend_var(growth_factor=1.03) A;",
        ),
        (
            "operations",
            "epilogue; output=1; end;",
            "epilogue; output=2; end;",
        ),
        (
            "filter",
            "filter_initial_state; y(-1)=1; end;",
            "filter_initial_state; y(-1)=2; end;",
        ),
        (
            "homotopy",
            "homotopy_setup; a,0,1; end;",
            "homotopy_setup; b,0,1; end;",
        ),
        ("macro_context", "@#define n=1\n", "@#define n=2\n"),
    ];
    let (service, _socket) = new_service();
    for (label, old, new) in cases {
        let old = format!("{BASE}{old}");
        let new = format!("{BASE}{new}");
        let supplied = dynare_compare_models(&old, &new, None, None, None, None, None);
        assert!(
            supplied["semantic"]["rows"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty()),
            "no compared row for {label}"
        );
        let live = current(service.inner(), &old, &new).await;
        for (name, result) in [
            ("current_lsp", live),
            (
                "snapshot_lsp",
                compare_captured_snapshots(
                    captured("before", &old),
                    captured("after", &new),
                    SnapshotCoordinates::Lsp,
                )["diff"]
                    .clone(),
            ),
            (
                "snapshot_mcp",
                compare_captured_snapshots(
                    captured("before", &old),
                    captured("after", &new),
                    SnapshotCoordinates::Mcp,
                )["diff"]
                    .clone(),
            ),
        ] {
            assert_eq!(
                result["semantic"], supplied["semantic"],
                "{label} via {name}"
            );
            assert_eq!(
                result["coverage"]["families"], supplied["coverage"]["families"],
                "{label} coverage via {name}"
            );
        }
    }
}

#[tokio::test]
async fn equation_surgery_written_source_is_exact_across_compare_adapters() {
    let old = "var c; model; [name='Consumption'] c=.8; end;";
    let instruction = "model_replace( 'Consumption' );\n  // retain written spelling and spacing\n  [name='Consumption'] c = .8;\nend;";
    let new = format!("var c; model; [name='Consumption'] c=1; end;\n{instruction}");
    let supplied = dynare_compare_models(old, &new, None, None, None, None, None);
    let (service, _socket) = new_service();
    for (adapter, result) in [
        ("supplied MCP", supplied),
        ("current LSP", current(service.inner(), old, &new).await),
        (
            "snapshot LSP",
            compare_captured_snapshots(
                captured("before", old),
                captured("after", &new),
                SnapshotCoordinates::Lsp,
            )["diff"]
                .clone(),
        ),
        (
            "snapshot MCP",
            compare_captured_snapshots(
                captured("before", old),
                captured("after", &new),
                SnapshotCoordinates::Mcp,
            )["diff"]
                .clone(),
        ),
    ] {
        let rows = result["semantic"]["rows"].as_array().unwrap();
        let operation = rows
            .iter()
            .find(|row| row["family"] == "operations" && row["name"] == "model_replace")
            .unwrap();
        let text = operation["expressions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|expression| expression["field"] == "statement_text");
        assert_eq!(
            text.map(|expression| expression["after"]["text"].as_str().unwrap()),
            Some(instruction),
            "{adapter} must display the captured written operation, not its internal records"
        );
        assert!(
            rows.iter().all(|row| row["family"] != "commands"),
            "{adapter} has a duplicate replacement Commands row"
        );
    }
    let spaced = new.replace("c = .8;", "c   =   .8;");
    let source_only = dynare_compare_models(&new, &spaced, None, None, None, None, None);
    assert!(
        source_only["semantic"]["rows"]
            .as_array()
            .unwrap()
            .is_empty(),
        "display context must not count a formatting edit as a model change"
    );
    assert!(!source_only["source_changes"]["files"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn forecast_path_written_context_omits_unchanged_variable_paths() {
    let base = "var y pi; model; y=0; pi=0; end;\nconditional_forecast_paths;\nvar y;\nperiods 1 2 3;\nvalues 0.1 0.25 0.1;\nvar pi;\nperiods 1 2 3;\nvalues 0.5 0.5 0.5;\nend;";
    let after = base.replace("values 0.1 0.25 0.1;", "values 0.2 0.25 0.1;");
    let result = dynare_compare_models(base, &after, None, None, None, None, None);
    let rows = result["semantic"]["rows"].as_array().unwrap();
    let path = rows
        .iter()
        .find(|row| row["family"] == "ms_sbvar" && row["name"] == "y")
        .unwrap();
    let text = path["expressions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|expression| expression["field"] == "statement_text");
    assert_eq!(
        text.map(|expression| expression["after"]["text"].as_str().unwrap()),
        Some("conditional_forecast_paths;\nvar y;\nperiods 1 2 3;\nvalues 0.2 0.25 0.1;\nend;")
    );
    assert!(rows.iter().all(|row| row["name"] != "pi"));
}

// Guards period-specific pairing and written state instructions without unchanged assignments.
#[test]
fn history_periods_pair_and_show_only_changed_written_assignments() {
    let before = "var y c;\nhistval;\ny(0) = 0.1;\ny(-1) = 0.05;\ny(-2) = 0.3;\nc(0) = 0;\nend;";
    let after = before.replace("0.1;", "0.2;").replace("0.05;", "-0.05;");
    let result = dynare_compare_models(before, &after, None, None, None, None, None);
    let rows: Vec<_> = result["semantic"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["family"] == "steady_state")
        .collect();
    assert_eq!(
        rows.len(),
        2,
        "changed periods must each have one paired owner: {rows:?}"
    );
    for (row, target, old, new) in [
        (&rows[0], "y(0)", "0.1", "0.2"),
        (&rows[1], "y(-1)", "0.05", "-0.05"),
    ] {
        assert_eq!(
            row["change"], "changed",
            "{target} must pair by name and period"
        );
        let text = row["expressions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|expression| expression["field"] == "statement_text")
            .expect("written history instruction");
        assert_eq!(
            text["before"]["text"],
            format!("histval;\n{target} = {old};\nend;")
        );
        assert_eq!(
            text["after"]["text"],
            format!("histval;\n{target} = {new};\nend;")
        );
        let highlighted: String = text["after"]["runs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|run| run["role"] == "added")
            .map(|run| run["text"].as_str().unwrap())
            .collect();
        assert!(!highlighted.contains(target) && !highlighted.contains("histval"));
    }
}

// Guards block-body selection at the shared adapter seam, including multi-output instructions.
#[test]
fn retained_block_cards_keep_written_syntax_and_omit_unchanged_siblings() {
    for (block, changed, unchanged, family) in [
        ("initval", "y = 0.1;", "c = 0;", "steady_state"),
        ("endval", "y = 0.1;", "c = 0;", "steady_state"),
        (
            "steady_state_model",
            "[y, x] = pair(0.1);",
            "c = 0;",
            "steady_state",
        ),
        (
            "filter_initial_state",
            "y(0) = 0.1;",
            "y(-1) = 0;",
            "operations",
        ),
        ("epilogue", "q = y + 0.1;", "p = c;", "operations"),
        ("optim_weights", "y 0.1;", "c 1;", "policy"),
        (
            "estimated_params",
            "a, normal_pdf, 0.1, 1;",
            "b, normal_pdf, 0.5, 1;",
            "priors",
        ),
    ] {
        let before = format!("{BASE}{block};\n{changed}\n{unchanged}\nend;");
        let after = before.replace("0.1", "0.2");
        let result = dynare_compare_models(&before, &after, None, None, None, None, None);
        let rows: Vec<_> = result["semantic"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["family"] == family)
            .collect();
        assert_eq!(rows.len(), 1, "{block}: {rows:?}");
        let text = rows[0]["expressions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|expression| expression["field"] == "statement_text")
            .unwrap_or_else(|| panic!("{block} has no written instruction: {:?}", rows[0]));
        assert_eq!(
            text["after"]["text"],
            format!("{block};\n{}\nend;", changed.replace("0.1", "0.2")),
            "{block}"
        );
    }
}

// Guards block creation/removal highlights without coloring an existing parent for an added child.
#[test]
fn written_block_boundaries_follow_parent_presence() {
    for (block, added, existing, family) in [
        ("initval", "y = 0.1;", "c = 0;", "steady_state"),
        ("endval", "y = 0.1;", "c = 0;", "steady_state"),
        ("histval", "y(0) = 0.1;", "c(0) = 0;", "steady_state"),
        ("steady_state_model", "y = 0.1;", "c = 0;", "steady_state"),
        (
            "filter_initial_state",
            "y(0) = 0.1;",
            "c(0) = 0;",
            "operations",
        ),
        ("epilogue", "q = y + 0.1;", "p = c;", "operations"),
        ("optim_weights", "y 0.1;", "c 1;", "policy"),
        (
            "estimated_params",
            "a, normal_pdf, 0.1, 1;",
            "b, normal_pdf, 0.5, 1;",
            "priors",
        ),
        ("shocks", "var e; stderr 0.1;", "var u; stderr 1;", "shocks"),
        (
            "conditional_forecast_paths",
            "var y; periods 1; values 0.1;",
            "var c; periods 1; values 0;",
            "ms_sbvar",
        ),
    ] {
        for parent_exists in [false, true] {
            let old = if parent_exists {
                format!("{BASE}{block};\n{existing}\nend;")
            } else {
                BASE.to_owned()
            };
            let new = format!(
                "{BASE}{block};\n{added}\n{}end;",
                if parent_exists {
                    format!("{existing}\n")
                } else {
                    String::new()
                }
            );
            for (before, after, side, role) in [
                (&old, &new, "after", "added"),
                (&new, &old, "before", "removed"),
            ] {
                let result = dynare_compare_models(before, after, None, None, None, None, None);
                let texts: Vec<_> = result["semantic"]["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|row| row["family"] == family)
                    .flat_map(|row| row["expressions"].as_array().unwrap())
                    .filter(|text| {
                        text["field"] == "statement_text"
                            && text[side]["text"]
                                .as_str()
                                .is_some_and(|text| text.contains(added))
                    })
                    .collect();
                assert!(!texts.is_empty(), "{block} has no written instruction");
                for text in texts {
                    let highlighted: String = text[side]["runs"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|run| run["role"] == role)
                        .map(|run| run["text"].as_str().unwrap())
                        .collect();
                    assert_eq!(
                        highlighted.contains(block),
                        !parent_exists,
                        "{block} opener, existing={parent_exists}: {highlighted}"
                    );
                    assert_eq!(
                        highlighted.contains("end;"),
                        !parent_exists,
                        "{block} closer, existing={parent_exists}: {highlighted}"
                    );
                    assert!(highlighted.contains(added), "{block} body: {highlighted}");
                }
            }
        }
    }
}

// Guards the added blocks in the moments showcase, including block-level fact owners.
#[test]
fn new_moment_block_cards_highlight_their_whole_definition() {
    let before = format!("{BASE}matched_moments;\ny;\nc*y;\nend;");
    let after = format!("{before}\nsteady_state_model; y=0; c=0; end;\nshocks; var e; stderr 0.02; end;\nestimated_params; a, beta_pdf, 0.6, 0.1; end;\nmatched_irfs; var y; varexo e; periods 1 2 3; values 0.02 0.015 0.01; end;\nmatched_irfs_weights; y(1), e, c(2), e, 0.75; end;");
    for (old, new, side, role) in [
        (&before, &after, "after", "added"),
        (&after, &before, "before", "removed"),
    ] {
        let result = dynare_compare_models(old, new, None, None, None, None, None);
        let texts: Vec<_> = result["semantic"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|row| row["expressions"].as_array().unwrap())
            .filter(|text| text["field"] == "statement_text")
            .filter_map(|text| text[side].as_object())
            .collect();
        for block in [
            "steady_state_model",
            "shocks",
            "estimated_params",
            "matched_irfs",
            "matched_irfs_weights",
        ] {
            let matching: Vec<_> = texts
                .iter()
                .filter(|text| {
                    text["text"]
                        .as_str()
                        .unwrap()
                        .starts_with(&format!("{block};"))
                })
                .collect();
            assert!(!matching.is_empty(), "{block} has no written instruction");
            for text in matching {
                let highlighted: String = text["runs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|run| run["role"] == role)
                    .map(|run| run["text"].as_str().unwrap())
                    .collect();
                assert_eq!(
                    highlighted,
                    text["text"].as_str().unwrap(),
                    "new {block} must retain every boundary and body token"
                );
            }
        }
    }
}

// Guards changed opener options on an added child whose parent already exists.
#[test]
fn shared_block_options_highlight_without_coloring_unchanged_boundaries() {
    let before = format!("{BASE}shocks;\nvar u; stderr 1;\nend;");
    let after = format!("{BASE}shocks(overwrite);\nvar e; stderr 0.1;\nvar u; stderr 1;\nend;");
    let result = dynare_compare_models(&before, &after, None, None, None, None, None);
    let text = result["semantic"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["family"] == "shocks" && row["name"] == "e")
        .unwrap()["expressions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|text| text["field"] == "statement_text")
        .unwrap();
    let highlighted: String = text["after"]["runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|run| run["role"] == "added")
        .map(|run| run["text"].as_str().unwrap())
        .collect();
    assert!(highlighted.contains("(overwrite)"));
    assert!(highlighted.contains("var e; stderr 0.1;"));
    assert!(
        !highlighted.contains("shocks") && !highlighted.contains("end;"),
        "{highlighted}"
    );
}
