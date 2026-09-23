use dygnosis::model_diff::{compare_models_with_sources, CompareSource};
use dygnosis::{compare_models, parse};

fn compare(before: &str, after: &str) -> serde_json::Value {
    compare_models(&parse(before), &parse(after)).to_json()
}

fn rows(diff: &serde_json::Value) -> &[serde_json::Value] {
    diff["shock_setup_changes"].as_array().expect("shock rows")
}

fn row<'a>(diff: &'a serde_json::Value, form: &str, target: &str) -> &'a serde_json::Value {
    rows(diff)
        .iter()
        .find(|row| row["form"] == form && row["target"] == target)
        .unwrap_or_else(|| panic!("missing {form} {target}: {diff}"))
}

#[test]
fn stochastic_written_forms_and_measurement_error_change() {
    let before = "var y; varexo e u v; varobs y;\nshocks;\nvar e; stderr 0.1;\nvar e,u = 0.2;\nskew e,u,v = 1;\nvar y = 0.01;\nend;\n";
    let after = "var y; varexo e u v; varobs y;\nshocks;\nvar e = 0.01;\ncorr u,e = 0.5;\nskew v,e,u = 2;\nvar y; stderr 0.2;\nend;\n";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 4, "{diff}");
    assert_eq!(
        row(&diff, "stochastic_shock", "e")["before"]["measure"],
        "stderr"
    );
    assert_eq!(
        row(&diff, "stochastic_shock", "e")["after"]["measure"],
        "variance"
    );
    assert_eq!(
        row(&diff, "stochastic_shock", "e,u")["before"]["measure"],
        "covariance"
    );
    assert_eq!(
        row(&diff, "stochastic_shock", "e,u")["after"]["measure"],
        "correlation"
    );
    assert_eq!(
        row(&diff, "stochastic_shock", "e,u,v")["after"]["values"][0],
        "2"
    );
    assert_eq!(
        row(&diff, "stochastic_shock", "y")["after"]["domain"],
        "measurement_error"
    );
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("Shock setup changes"));
}

#[test]
fn stochastic_parentheses_are_written_and_pair_order_is_canonical() {
    let before = "varexo e u v; parameters rho; shocks; var e; stderr (rho); corr e,u = 0.5; skew e,u,v = 1; end;";
    let after = "varexo e u v; parameters rho; shocks; var e; stderr rho; corr u,e = 0.5; skew v,e,u = 1; end;";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    let size = row(&diff, "stochastic_shock", "e");
    assert_eq!(size["before"]["values"][0], "(rho)");
    assert_eq!(size["after"]["values"][0], "rho");
}

#[test]
fn scheduled_form_timing_operation_and_baseline_change() {
    let before =
        "varexo e; parameters rho;\nshocks; var e; periods 2024Q1:2024Q2; values (rho*e); end;";
    let after = "varexo e; parameters rho;\nmshocks(relative_to_initval); var e; periods 2024Q2:2024Q3; values 1.05; end;";
    let diff = compare(before, after);
    let item = row(&diff, "scheduled_shock", "e");
    assert_eq!(item["change"], "changed");
    assert_eq!(item["before"]["block"], "shocks");
    assert_eq!(item["after"]["block"], "mshocks");
    assert_eq!(item["before"]["periods"][0], "2024Q1:2024Q2");
    assert_eq!(item["after"]["periods"][0], "2024Q2:2024Q3");
    assert_eq!(item["before"]["values"][0], "(rho*e)");
    assert_eq!(item["after"]["relative_to_initval"], true);
    let md = diff["markdown"].as_str().unwrap();
    assert!(md.contains("form shocks → mshocks"), "{md}");
    assert!(md.contains("periods 2024Q1:2024Q2 → 2024Q2:2024Q3"), "{md}");
}

#[test]
fn surprise_and_learnt_in_keep_their_written_settings() {
    let surprise_a = "varexo e; shocks(surprise); var e; periods 1:2; values 0.1; end;";
    let surprise_b = "varexo e; shocks(surprise); var e; periods 2:3; values 0.1; end;";
    let surprise = compare(surprise_a, surprise_b);
    assert_eq!(row(&surprise, "surprise_shock", "e")["change"], "changed");

    let learnt_a = "varexo e; shocks(learnt_in=2); var e; periods 3:4; add 0.1; end;";
    let learnt_b = "varexo e; shocks(learnt_in=3); var e; periods 4:5; multiply 2; end;";
    let learnt = compare(learnt_a, learnt_b);
    let item = row(&learnt, "scheduled_shock", "e");
    assert_eq!(item["before"]["learnt_in"]["text"], "2");
    assert_eq!(item["after"]["learnt_in"]["text"], "3");
    assert_eq!(item["before"]["operation"], "add");
    assert_eq!(item["after"]["operation"], "multiply");

    let mshocks_a = "varexo e; mshocks(learnt_in=2024Q1); var e; periods 2024Q2; values 1.1; end;";
    let mshocks_b = "varexo e; mshocks(learnt_in=2024Q2 relative_to_initval); var e; periods 2024Q3; values 1.2; end;";
    let mshocks = compare(mshocks_a, mshocks_b);
    let item = row(&mshocks, "scheduled_shock", "e");
    assert_eq!(item["before"]["learnt_in"]["kind"], "date");
    assert_eq!(item["before"]["learnt_in"]["text"], "2024Q1");
    assert_eq!(item["after"]["learnt_in"]["text"], "2024Q2");
    assert_eq!(item["after"]["relative_to_initval"], true);
}

#[test]
fn heteroskedastic_values_and_scales_are_written_rows() {
    let before = "varexo e; heteroskedastic_shocks; var e; periods 86:87; scales 0.5; end;";
    let after = "varexo e; heteroskedastic_shocks; var e; periods 88; values 0.1; end;";
    let diff = compare(before, after);
    let item = row(&diff, "heteroskedastic_shock", "e");
    assert_eq!(item["before"]["operation"], "scales");
    assert_eq!(item["after"]["operation"], "values");
    assert_eq!(item["after"]["periods"][0], "88");
}

#[test]
fn shock_paths_show_terminal_control_and_expression_changes() {
    let before = "var y; varexo e u; database db;\nshock_paths(learnt_in=2024Q1);\nvar e; periods 2024Q2:end; values db.x;\nexogenize y; periods 2024Q2; values initval.y; endogenize e;\nend;";
    let after = "var y; varexo e u; database db;\nshock_paths(learnt_in=2024Q2);\nvar e; periods 2024Q3:end; values self.e(-1)+db.x;\nexogenize y; periods 2024Q3; values initval.y+1; endogenize u;\nend;";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 2, "{diff}");
    let exo = row(&diff, "shock_path", "e");
    assert_eq!(exo["before"]["periods"][0], "2024Q2:end");
    assert_eq!(exo["after"]["values"][0], "self.e(-1)+db.x");
    let controlled = row(&diff, "shock_path", "y");
    assert_eq!(controlled["before"]["released_exogenous"], "e");
    assert_eq!(controlled["after"]["released_exogenous"], "u");
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("released exogenous e → u"));
}

#[test]
fn endval_and_companion_controlled_paths_change() {
    let before = "var y; varexo e u;\nendval(learnt_in=3); e = 1; y += 0.1; end;\nperfect_foresight_controlled_paths(learnt_in=3); exogenize y; periods 4:5; values 1.1; endogenize e; end;";
    let after = "var y; varexo e u;\nendval(learnt_in=4); e = 2; y *= 1.1; end;\nperfect_foresight_controlled_paths(learnt_in=4); exogenize y; periods 5:6; values 1.2; endogenize u; end;";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 3, "{diff}");
    assert_eq!(row(&diff, "endval", "y")["before"]["operation"], "add");
    assert_eq!(row(&diff, "endval", "y")["after"]["operation"], "multiply");
    assert_eq!(row(&diff, "endval", "e")["after"]["learnt_in"]["text"], "4");
    assert_eq!(
        row(&diff, "controlled_path", "y")["after"]["released_exogenous"],
        "u"
    );
}

#[test]
fn overwrite_changes_status_and_empty_reset_is_visible() {
    let base = "varexo e; shocks; var e; stderr 0.1; end;\n";
    let extended = format!("{base}shocks(overwrite); end;\n");
    let diff = compare(base, &extended);
    assert_eq!(rows(&diff).len(), 2, "{diff}");
    let size = row(&diff, "stochastic_shock", "e");
    assert_eq!(size["before"]["status"], "active");
    assert_eq!(size["after"]["status"], "superseded");
    let reset = row(
        &diff,
        "shock_reset",
        "deterministic schedules and variance/covariance settings",
    );
    assert_eq!(reset["change"], "added");
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("superseded by overwrite"));
}

#[test]
fn shocks_overwrite_preserves_unrelated_skew_and_replaces_same_triple() {
    let base = "varexo e u; shocks; var e = 0.1; skew e = 1; skew u = 2; end;";
    let reset = format!("{base} shocks(overwrite); var e = 0.2; end;");
    let diff = compare(base, &reset);
    assert!(
        rows(&diff).iter().all(|item| { item["role"] != "skew" }),
        "{diff}"
    );
    assert!(
        rows(&diff).iter().any(|item| {
            item["role"] == "size"
                && item["target"] == "e"
                && item["after"]["status"] == "superseded"
        }),
        "{diff}"
    );

    let same_triple = format!("{base} shocks(overwrite); skew e = 3; end;");
    let diff = compare(base, &same_triple);
    assert!(
        rows(&diff).iter().any(|item| {
            item["role"] == "skew"
                && item["target"] == "e,e,e"
                && item["after"]["status"] == "superseded"
        }),
        "{diff}"
    );
    assert!(
        rows(&diff).iter().all(|item| item["target"] != "u,u,u"),
        "{diff}"
    );

    let repeated_triple = format!("{base} shocks; skew e,e,e = 3; end;");
    let diff = compare(base, &repeated_triple);
    assert!(
        rows(&diff).iter().any(|item| {
            item["role"] == "skew"
                && item["target"] == "e,e,e"
                && item["after"]["status"] == "superseded"
        }),
        "{diff}"
    );
}

#[test]
fn single_and_triple_skew_forms_pair_on_the_same_tensor_entry() {
    let before = "varexo e; shocks; skew e = 1; end;";
    let after = "varexo e; shocks; skew e,e,e = 1; end;";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    let item = row(&diff, "stochastic_shock", "e,e,e");
    assert_eq!(item["change"], "changed");
    assert_eq!(item["before"]["measure"], "skewness");
    assert_eq!(item["after"]["measure"], "co_skewness");
    assert_eq!(item["before"]["written_target"], "e");
    assert_eq!(item["after"]["written_target"], "e,e,e");
}

#[test]
fn surprise_and_heteroskedastic_overwrite_do_not_cross_stores() {
    let before = "varexo e u; shocks(surprise); var e; periods 1; values 0.1; end; heteroskedastic_shocks; var u; periods 2; scales 0.5; end;";
    let after = format!(
        "{before} shocks(surprise,overwrite); var e; periods 3; values 0.2; end; heteroskedastic_shocks(overwrite); var u; periods 4; values 0.1; end;"
    );
    let diff = compare(before, &after);
    assert!(rows(&diff).iter().any(|item| {
        item["form"] == "surprise_shock"
            && item["after"]["periods"][0] == "1"
            && item["after"]["status"] == "superseded"
    }));
    assert!(rows(&diff).iter().any(|item| {
        item["form"] == "heteroskedastic_shock"
            && item["after"]["periods"][0] == "2"
            && item["after"]["status"] == "superseded"
    }));
}

#[test]
fn mshocks_overwrite_keeps_stochastic_size_active() {
    let base = "varexo e u; shocks; var e; stderr 0.1; var u; periods 2; values 1; end;\n";
    let extended = format!("{base}mshocks(overwrite); var u; periods 3; values 1.1; end;\n");
    let diff = compare(base, &extended);
    assert!(rows(&diff).iter().all(|row| row["target"] != "e"), "{diff}");
    assert!(rows(&diff)
        .iter()
        .any(|row| row["target"] == "u" && row["after"]["status"] == "superseded"));
}

#[test]
fn paths_overwrite_uses_typed_learning_period() {
    let base = "varexo e; shock_paths(learnt_in=2); var e; periods 3; values 1; end;\n";
    let different = format!(
        "{base}shock_paths(learnt_in=2024Q1 overwrite); var e; periods 2024Q2; values 2; end;"
    );
    let diff = compare(base, &different);
    assert!(rows(&diff)
        .iter()
        .all(|row| row["after"]["status"] != "superseded"));
    let same =
        format!("{base}shock_paths(learnt_in=2 overwrite); var e; periods 4; values 2; end;");
    let diff = compare(base, &same);
    assert!(rows(&diff)
        .iter()
        .any(|row| row["after"]["status"] == "superseded"));
}

#[test]
fn path_overwrite_supersedes_exogenous_and_controlled_rows() {
    let before = "var y; varexo e; shock_paths(learnt_in=2); var e; periods 3; values 1; exogenize y; periods 3; values 2; endogenize e; end;";
    let after = format!(
        "{before} shock_paths(learnt_in=2 overwrite); var e; periods 4; values 3; exogenize y; periods 4; values 4; endogenize e; end;"
    );
    let diff = compare(before, &after);
    assert!(rows(&diff).iter().any(|item| {
        item["form"] == "shock_path"
            && item["role"] == "exogenous"
            && item["after"]["periods"][0] == "3"
            && item["after"]["status"] == "superseded"
    }));
    assert!(rows(&diff).iter().any(|item| {
        item["form"] == "shock_path"
            && item["role"] == "controlled"
            && item["after"]["periods"][0] == "3"
            && item["after"]["status"] == "superseded"
    }));
}

#[test]
fn ambiguous_rows_stay_added_and_removed_and_declarations_do_not_hide_shocks() {
    let before = "varexo e; shocks; var e; periods 2; values 1; end; shocks; var e; periods 3; values 2; end;";
    let after = "varexo e u; shocks; var e; periods 4; values 3; end; shocks; var e; periods 5; values 4; end; shocks; var u; periods 1; values 1; end;";
    let diff = compare(before, after);
    assert_eq!(
        rows(&diff)
            .iter()
            .filter(|row| row["target"] == "e" && row["change"] == "removed")
            .count(),
        2
    );
    assert_eq!(
        rows(&diff)
            .iter()
            .filter(|row| row["target"] == "e" && row["change"] == "added")
            .count(),
        2
    );
    assert_eq!(row(&diff, "scheduled_shock", "u")["change"], "added");

    let declaration_only = compare("varexo e;", "varexo e u;");
    assert!(rows(&declaration_only).is_empty());
}

#[test]
fn declaration_type_alone_does_not_change_a_written_stochastic_row() {
    // Both are accepted by Dynare 7.2: the same row calibrates an exogenous
    // variance on the left and an observed-variable measurement error on the right.
    let before = "varexo e; shocks; var e = 0.1; end;";
    let after = "var e; varobs e; shocks; var e = 0.1; end;";
    let diff = compare(before, after);
    assert!(rows(&diff).is_empty(), "{diff}");

    let changed_row = after.replace("var e = 0.1", "var e = 0.2");
    let diff = compare(before, &changed_row);
    let change = row(&diff, "stochastic_shock", "e");
    assert_eq!(change["change"], "changed");
    assert_eq!(change["before"]["domain"], "exogenous");
    assert_eq!(change["after"]["domain"], "measurement_error");
    assert_eq!(change["after"]["values"][0], "0.2");
}

#[test]
fn learnt_shock_date_overwrite_uses_72_period_count() {
    let neighbours = [
        ("2024Q4+1", "2025Q1", true),
        ("2024Q3+1+1", "2025Q1", true),
        ("-2Q4+1", "-1Q1", true),
        ("2024A+1", "2025Y", true),
        ("2024S2+1", "2025H1", true),
        ("2024M12+1", "2025M1", true),
        ("2024Q4+1", "2025Q2", false),
        ("2024Q4+1", "2025M1", false),
    ];
    for (first, second, replaced) in neighbours {
        let text = format!(
            "varexo e; shocks(learnt_in={first}); var e; periods 2026Q1; values 1; end; \
             shocks(learnt_in={second},overwrite); var e; periods 2026Q2; values 2; end;"
        );
        let diff = compare("varexo e;", &text);
        let old = rows(&diff)
            .iter()
            .find(|item| item["after"]["learnt_in"]["text"] == first)
            .unwrap_or_else(|| panic!("missing first {first}: {diff}"));
        assert_eq!(
            old["after"]["status"],
            if replaced { "superseded" } else { "active" },
            "{first} versus {second}: {diff}"
        );
        assert!(rows(&diff)
            .iter()
            .any(|item| item["after"]["learnt_in"]["text"] == second));
    }
}

#[test]
fn shock_paths_date_overwrite_uses_the_same_typed_key() {
    for (second, replaced) in [("2025Q1", true), ("2025Q2", false), ("2025M1", false)] {
        let text = format!(
            "varexo e; shock_paths(learnt_in=2024Q4+1); var e; periods 2026Q1; values 1; end; \
             shock_paths(learnt_in={second} overwrite); var e; periods 2026Q2; values 2; end;"
        );
        let diff = compare("varexo e;", &text);
        let old = rows(&diff)
            .iter()
            .find(|item| item["after"]["learnt_in"]["text"] == "2024Q4+1")
            .unwrap_or_else(|| panic!("missing first path: {diff}"));
        assert_eq!(
            old["after"]["status"],
            if replaced { "superseded" } else { "active" },
            "2024Q4+1 versus {second}: {diff}"
        );
    }
}

#[test]
fn exact_cancellation_leaves_one_clear_pair() {
    let before = "varexo e; shocks; var e; periods 2; values 1; end; shocks; var e; periods 3; values 2; end;";
    let after = "varexo e; shocks; var e; periods 2; values 1; end; shocks; var e; periods 4; values 3; end;";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    let change = row(&diff, "scheduled_shock", "e");
    assert_eq!(change["change"], "changed");
    assert_eq!(change["before"]["periods"][0], "3");
    assert_eq!(change["after"]["periods"][0], "4");
}

#[test]
fn explicit_initial_learning_is_written_but_uses_regular_reset() {
    let before = "varexo e; shocks; var e; periods 2; values 1; end;";
    let after = "varexo e; shocks(learnt_in=1); var e; periods 2; values 1; end;";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    assert_eq!(
        row(&diff, "scheduled_shock", "e")["after"]["learnt_in"]["text"],
        "1"
    );
}

#[test]
fn overwrite_option_and_date_offset_are_written_even_without_status_change() {
    let before = "varexo e; shocks; var e; periods 2024Q1+1; values 0.1; end;";
    let after = "varexo e; shocks(overwrite); var e; periods 2024Q1+1; values 0.1; end;";
    let diff = compare(before, after);
    let item = row(&diff, "scheduled_shock", "e");
    assert_eq!(item["change"], "changed");
    assert_eq!(item["before"]["periods"][0], "2024Q1+1");
    assert_eq!(item["after"]["periods"][0], "2024Q1+1");
    assert_eq!(item["before"]["overwrite"], false);
    assert_eq!(item["after"]["overwrite"], true);
}

#[test]
fn locations_require_the_original_source_text() {
    let before = "varexo e;\nshocks; var e; periods 2; values 1; end;\n";
    let after = "varexo e;\nshocks; var e; periods 3; values 2; end;\n";
    let old_model = parse(before);
    let new_model = parse(after);
    let diff = compare_models_with_sources(
        &old_model,
        &new_model,
        Some(CompareSource {
            text: before,
            origin_uri: Some("before.mod"),
        }),
        Some(CompareSource {
            text: after,
            origin_uri: Some("after.mod"),
        }),
    )
    .to_json();
    let item = row(&diff, "scheduled_shock", "e");
    assert_eq!(item["before"]["location"]["line"], 2);
    assert_eq!(item["before"]["origin_uri"], "before.mod");
    assert_eq!(item["after"]["origin_uri"], "after.mod");

    let mismatched = compare_models_with_sources(
        &old_model,
        &new_model,
        Some(CompareSource {
            text: "@#include \"before.mod\"",
            origin_uri: Some("root.mod"),
        }),
        None,
    )
    .to_json();
    let item = row(&mismatched, "scheduled_shock", "e");
    assert!(item["before"].get("location").is_none(), "{mismatched}");
    assert!(item["before"].get("origin_uri").is_none(), "{mismatched}");
}

#[test]
fn identical_setup_produces_no_changes_or_solver_claims() {
    let text = "varexo e; shocks; var e; stderr 0.1; end;";
    let diff = compare(text, text);
    assert!(rows(&diff).is_empty());
    let blob = diff.to_string();
    assert!(!blob.contains("steady_state"));
    assert!(!blob.contains("computed"));
}

#[test]
fn installed_dynare_accepts_compare_option_neighbours() {
    use std::path::Path;
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    let binary = Path::new("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        return;
    }
    let temp_root = std::env::temp_dir().canonicalize().expect("temp root");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let directory = temp_root.join(format!(
        "dygnosis_compare_options_{}_{}",
        std::process::id(),
        nonce
    ));
    std::fs::create_dir(&directory).expect("create isolated probe directory");
    let prefix = "var y; varexo e; model; y=e; end; ";
    let probes = [
        (
            "mshocks_ok.mod",
            "mshocks(learnt_in=2 relative_to_initval); var e; periods 3; values 1.2; end;",
            true,
        ),
        (
            "mshocks_comma.mod",
            "mshocks(learnt_in=2,relative_to_initval); var e; periods 3; values 1.2; end;",
            false,
        ),
        (
            "paths_ok.mod",
            "shock_paths(learnt_in=2 overwrite); var e; periods 3; values 2; end;",
            true,
        ),
        (
            "paths_comma.mod",
            "shock_paths(learnt_in=2,overwrite); var e; periods 3; values 2; end;",
            false,
        ),
    ];
    let mut results = Vec::new();
    for (name, body, accepted) in probes {
        std::fs::write(directory.join(name), format!("{prefix}{body}"))
            .expect("write compare probe");
        let output = Command::new(binary)
            .current_dir(&directory)
            .arg(name)
            .arg("json=check")
            .arg("onlyjson")
            .output()
            .expect("run Dynare 7.2");
        results.push((name, accepted, output));
    }
    let resolved = directory.canonicalize().expect("probe directory");
    assert!(resolved.starts_with(&temp_root));
    std::fs::remove_dir_all(&resolved).expect("remove isolated probe directory");
    for (name, accepted, output) in results {
        let message = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.status.success(), accepted, "{name}: {message}");
        if !accepted {
            assert!(
                message.contains("syntax error, unexpected COMMA"),
                "{name}: {message}"
            );
        }
    }
}
