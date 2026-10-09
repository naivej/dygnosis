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
