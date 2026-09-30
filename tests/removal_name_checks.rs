use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};
use std::time::Duration;

fn source(site: &str, name: &str, before: bool) -> String {
    let base = include_str!("fixtures/d_surgery/quiet_planner_dropped.mod")
        .replace("\r\n", "\n")
        .replace("planner_objective c^2 + k^2;", "")
        .replace("ramsey_constraints;\nc >= 0;\nend;", "");
    let mut statement = match site {
        "optim_weights" => format!("optim_weights; {name} 1; end;"),
        "planner_objective" => format!("planner_objective {name}^2;"),
        _ => format!("ramsey_constraints; {name}>0; end;"),
    };
    if site != "planner_objective" {
        statement.push_str(" planner_objective k^2;");
    }
    let removal = "model_remove([grp='g']);";
    base.replace(
        removal,
        &if before {
            format!("{statement}\n{removal}")
        } else {
            format!("{removal}\n{statement}")
        },
    )
}

#[test]
fn post_removal_names_use_their_actual_excluded_or_exogenous_kind() {
    for name in ["c", "dummy1"] {
        for site in ["optim_weights", "planner_objective", "ramsey_constraints"] {
            for before in [true, false] {
                let source = source(site, name, before);
                let expected = match (name, site, before) {
                    ("dummy1", "planner_objective", _) => Some("E251"),
                    (_, _, true) => None,
                    (_, "optim_weights", false) => Some("E317"),
                    ("c", _, false) => Some("E426"),
                    _ => Some("E321"),
                };
                let diagnostics = analyze(&parse(&source));
                let errors: Vec<_> = diagnostics
                    .iter()
                    .filter(|row| {
                        matches!(
                            row.code.as_str(),
                            "E317" | "E251" | "E321" | "E426" | "E030"
                        )
                    })
                    .collect();
                assert_eq!(
                    errors.len(),
                    usize::from(expected.is_some()),
                    "{site} {name} {before}: {diagnostics:?}"
                );
                if let Some(code) = expected {
                    assert_eq!(errors[0].code, code, "{diagnostics:?}");
                }
                if let Some(pp) = find_preprocessor(None) {
                    let official = run_preprocessor(
                        &source,
                        &pp,
                        None,
                        Duration::from_secs(30),
                        JsonStage::Check,
                    );
                    assert_eq!(
                        official.success,
                        expected.is_none(),
                        "{site} {name} {before}: {official:?}"
                    );
                    if expected.is_some() {
                        assert!(
                            official.raw_stdout.contains(&errors[0].message),
                            "{official:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn weights_do_not_create_a_future_policy_parameter() {
    for generated in [false, true] {
        let source = format!(
            "var y; model; y=0; end; {} optim_weights; optimal_policy_discount_factor 1; end;",
            if generated {
                "planner_objective y^2; ramsey_model;"
            } else {
                ""
            }
        );
        let diagnostics = analyze(&parse(&source));
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "E317"),
            generated,
            "{diagnostics:?}"
        );
        assert_eq!(
            diagnostics.iter().any(|row| row.code == "E058"),
            !generated,
            "{diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(
                official.raw_stdout.contains(if generated {
                    "optimal_policy_discount_factor is not endogenous."
                } else {
                    "Unknown symbol: optimal_policy_discount_factor."
                }),
                "{official:?}"
            );
        }
    }
}

#[test]
fn var_remove_and_successful_restoration_use_current_kind() {
    for site in ["optim_weights", "planner_objective", "ramsey_constraints"] {
        for restore in [false, true] {
            let statement = match site {
                "optim_weights" => "optim_weights; c 1; end; planner_objective y^2;",
                "planner_objective" => "planner_objective c^2;",
                _ => "ramsey_constraints; c>0; end; planner_objective y^2;",
            };
            let source = format!(
                "var y c; model; y=0; end; var_remove c; {} {statement} ramsey_model;",
                if restore { "change_type(var) c;" } else { "" }
            );
            let diagnostics = analyze(&parse(&source));
            let wanted = if site == "optim_weights" {
                "E317"
            } else {
                "E426"
            };
            assert_eq!(
                diagnostics.iter().any(|row| row.code == wanted),
                !restore,
                "{diagnostics:?}"
            );
            if let Some(pp) = find_preprocessor(None) {
                let official = run_preprocessor(
                    &source,
                    &pp,
                    None,
                    Duration::from_secs(30),
                    JsonStage::Check,
                );
                assert_eq!(official.success, restore, "{site} {restore}: {official:?}");
            }
        }
        let source = source(site, "c", false).replace(
            "model_remove([grp='g']);",
            "model_remove([grp='g']); change_type(var) c;",
        );
        let diagnostics = analyze(&parse(&source));
        assert!(
            diagnostics.iter().any(|row| row.code == "E296"),
            "{diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!official.success, "{official:?}");
            assert!(
                official.raw_stdout.contains(
                    "You cannot modify the type of symbol c after having used it in an expression"
                ),
                "{official:?}"
            );
        }
    }
}

#[test]
fn even_an_unused_tag_association_is_used_by_model_remove() {
    let source = "var y c; model; [name='drop',endogenous='c'] y=1; [name='keep'] y=0; end; model_remove('drop'); change_type(var) c; optim_weights; c 1; end; planner_objective y^2; ramsey_model;";
    let diagnostics = analyze(&parse(source));
    assert!(
        diagnostics.iter().any(|row| row.code == "E296"),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!official.success, "{official:?}");
        assert!(
            official.raw_stdout.contains(
                "You cannot modify the type of symbol c after having used it in an expression"
            ),
            "{official:?}"
        );
    }
}

#[test]
fn competing_refusals_keep_effective_macro_order_and_one_owner() {
    let source = "var y c; model; y=0; end; var_remove c;\n@#for j in 1:2\n@#if j==2\nchange_type(var) missing;\n@#endif\n@#if j==1\nplanner_objective c^2;\n@#endif\n@#endfor\nramsey_model;";
    let diagnostics = analyze(&parse(source));
    assert_eq!(
        diagnostics.iter().filter(|row| row.code == "E426").count(),
        1,
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics
            .iter()
            .any(|row| matches!(row.code.as_str(), "E295" | "E296")),
        "{diagnostics:?}"
    );
    if let Some(pp) = find_preprocessor(None) {
        let official =
            run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            official
                .raw_stdout
                .contains("Variable 'c' can no longer be used"),
            "{official:?}"
        );
    }
    let refused_restore = self::source("planner_objective", "c", false).replace(
        "model_remove([grp='g']);",
        "model_remove([grp='g']); change_type(var) c;",
    );
    let diagnostics = analyze(&parse(&refused_restore));
    assert_eq!(
        diagnostics.iter().filter(|row| row.code == "E296").count(),
        1,
        "{diagnostics:?}"
    );
}
