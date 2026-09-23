use dygnosis::model_diff::{compare_models_with_sources, CompareSource};
use dygnosis::{compare_models, parse};

fn compare(before: &str, after: &str) -> serde_json::Value {
    compare_models(&parse(before), &parse(after)).to_json()
}

fn changes<'a>(diff: &'a serde_json::Value, form: &str) -> Vec<&'a serde_json::Value> {
    diff["shock_setup_changes"]
        .as_array()
        .expect("shock changes")
        .iter()
        .filter(|row| row["form"] == form)
        .collect()
}

fn context<'a>(diff: &'a serde_json::Value, role: &str, target: &str) -> &'a serde_json::Value {
    changes(diff, "shock_context")
        .into_iter()
        .find(|row| row["role"] == role && row["target"] == target)
        .unwrap_or_else(|| panic!("missing {role} {target}: {diff}"))
}

#[test]
fn parser_retains_group_identity_spans_and_estimation_data_options() {
    let text = "var y; varexo e u;\nshock_groups(name=group1); supply=e,u; end;\nshock_groups; other=u; end;\ninit2shocks(name=allocation); y,e; end;\nestimation(datafile='a.csv',first_obs=[10:12]);";
    let model = parse(text);
    assert_eq!(model.shock_group_blocks.len(), 2);
    let named = &model.shock_group_blocks[0];
    assert_eq!(named.group, "group1");
    assert_eq!(named.row_start, 0);
    assert_eq!(named.row_end, 1);
    assert_eq!(
        &text[named.group_span.unwrap().start as usize..named.group_span.unwrap().end as usize],
        "group1"
    );
    assert!(text[named.span.start as usize..named.span.end as usize].ends_with("end;"));
    assert_eq!(model.shock_group_blocks[1].group, "default");
    assert!(model.shock_group_blocks[1].group_span.is_none());
    let row = &model.shock_groups[0];
    assert_eq!(
        &text[row.span.start as usize..row.span.end as usize],
        "supply=e,u;"
    );

    let allocation = &model.init2shocks_blocks[0];
    assert_eq!(allocation.group, "allocation");
    assert_eq!(allocation.rows.len(), 1);
    assert!(text[allocation.span.start as usize..allocation.span.end as usize].ends_with("end;"));
    let options = &model.estimation_statements[0].data_options;
    assert_eq!(options.len(), 2);
    assert_eq!(options[0].value_text, "'a.csv'");
    assert_eq!(
        &text[options[1].value_span.start as usize..options[1].value_span.end as usize],
        "[10:12]"
    );
}

#[test]
fn multiplicative_baseline_changes_even_when_shock_row_does_not() {
    let before = "varexo e u; initval; e=1; u=4; end; mshocks; var e; periods 2; values 1.05; end;";
    let after = before.replace("e=1; u=4", "e=2; u=5");
    let diff = compare(before, &after);
    assert_eq!(changes(&diff, "scheduled_shock").len(), 0, "{diff}");
    let row = context(&diff, "baseline", "e");
    assert_eq!(row["change"], "changed");
    assert_eq!(row["before"]["block"], "initval");
    assert_eq!(row["before"]["values"][0], "1");
    assert_eq!(row["after"]["values"][0], "2");
    assert!(row["after"].get("status").is_none());
    assert_eq!(changes(&diff, "shock_context").len(), 1, "{diff}");
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("written baseline for mshocks"));
}

#[test]
fn endval_omission_keeps_initval_baseline_and_explicit_endval_is_linked() {
    let before = "varexo e u; initval; e=1; end; endval; u=4; end; mshocks; var e; periods 2; values 1.05; end;";
    let after = before.replace("e=1", "e=2");
    let fallback = compare(before, &after);
    assert_eq!(
        context(&fallback, "baseline", "e")["after"]["block"],
        "initval"
    );

    let before = "varexo e; initval; e=1; end; endval; e=2; end; mshocks; var e; periods 2; values 1.05; end;";
    let after = before.replace("endval; e=2", "endval; e=3");
    let explicit = compare(before, &after);
    assert_eq!(
        context(&explicit, "baseline", "e")["after"]["block"],
        "endval"
    );
    assert_eq!(changes(&explicit, "endval").len(), 1, "{explicit}");
    assert_eq!(changes(&explicit, "shock_context").len(), 1, "{explicit}");
}

#[test]
fn path_initval_reference_links_only_the_named_assignment() {
    let before = "var y z; varexo e; initval; y=1; z=4; end; shock_paths; var e; periods 1; values initval.y; end;";
    let after = before.replace("y=1; z=4", "y=2; z=5");
    let diff = compare(before, &after);
    assert_eq!(context(&diff, "baseline", "y")["after"]["values"][0], "2");
    assert_eq!(
        context(&diff, "baseline", "y")["after"]["related_target"],
        "e"
    );
    assert_eq!(changes(&diff, "shock_context").len(), 1, "{diff}");
    assert!(changes(&diff, "shock_path").is_empty(), "{diff}");
}

#[test]
fn changed_time_context_keeps_written_dates_and_command_names() {
    let before = "varexo e; set_time(2024Q1); perfect_foresight_setup(first_simulation_period=2024Q1,last_simulation_period=2024Q4); shocks; var e; periods 2024Q2; values 1; end;";
    let after = before
        .replace("set_time(2024Q1)", "set_time(2023Q4+1)")
        .replace(
            "last_simulation_period=2024Q4",
            "last_simulation_period=2025Q1",
        );
    let diff = compare(before, &after);
    assert_eq!(changes(&diff, "scheduled_shock").len(), 0, "{diff}");
    assert_eq!(
        context(&diff, "time", "set_time")["after"]["values"][0],
        "2023Q4+1"
    );
    assert_eq!(
        context(&diff, "time", "set_time")["after"]["related_target"],
        "e"
    );
    assert_eq!(
        context(
            &diff,
            "time",
            "perfect_foresight_setup.last_simulation_period"
        )["after"]["values"][0],
        "2025Q1"
    );
    assert_eq!(changes(&diff, "shock_context").len(), 2, "{diff}");
}

#[test]
fn database_declarations_and_written_path_references_are_visible() {
    let before = "varexo e; database db; shock_paths; var e; periods 1; values db.x; end;";
    let after = "varexo e; database ds; shock_paths; var e; periods 1; values ds.x; end;";
    let diff = compare(before, after);
    let db = changes(&diff, "shock_context");
    assert_eq!(db.len(), 2, "{diff}");
    assert!(db.iter().any(|row| row["role"] == "database"
        && row["target"] == "db"
        && row["change"] == "removed"));
    assert!(db.iter().any(|row| row["role"] == "database"
        && row["target"] == "ds"
        && row["change"] == "added"
        && row["after"]["references"][0] == "ds.x"));
    assert_eq!(
        context(&diff, "database", "ds")["after"]["related_target"],
        "e"
    );
    assert_eq!(changes(&diff, "shock_path").len(), 1, "{diff}");
}

#[test]
fn heteroskedastic_data_context_changes_without_a_shock_row_change() {
    let before = "varexo e; heteroskedastic_shocks; var e; periods 10; values 0.1; end; estimation(datafile='a.csv',first_obs=10);";
    let after = before
        .replace("'a.csv'", "'b.csv'")
        .replace("first_obs=10", "first_obs=[11:12]");
    let diff = compare(before, &after);
    assert!(changes(&diff, "heteroskedastic_shock").is_empty(), "{diff}");
    assert_eq!(
        context(&diff, "data_source", "estimation.source")["after"]["values"][0],
        "'b.csv'"
    );
    assert_eq!(
        context(&diff, "data_source", "estimation.source")["after"]["related_target"],
        "e"
    );
    assert_eq!(
        context(&diff, "data_source", "estimation.first_obs")["after"]["values"][0],
        "[11:12]"
    );

    let before = "varexo e; heteroskedastic_shocks; var e; periods 2024Q1; scales 0.5; end; data(file='a.csv',first_obs=2024Q1);";
    let after = before
        .replace("file='a.csv'", "series=myseries")
        .replace("first_obs=2024Q1", "first_obs=2024Q2");
    let diff = compare(before, &after);
    let source = context(&diff, "data_source", "data.source");
    assert_eq!(source["before"]["measure"], "file");
    assert_eq!(source["after"]["measure"], "series");
    assert_eq!(
        context(&diff, "data_source", "data.first_obs")["after"]["values"][0],
        "2024Q2"
    );
}

#[test]
fn analysis_changes_use_named_blocks_and_separate_markdown_section() {
    let before = "var y; varexo e u; shock_groups(name=g1); supply=e; end; init2shocks; y,e; end;";
    let after = "var y; varexo e u; shock_groups(name=g1); supply=e,u; end; init2shocks; y,u; end;";
    let diff = compare(before, after);
    let group = &changes(&diff, "shock_group")[0];
    assert_eq!(group["target"], "supply");
    assert_eq!(group["after"]["group"], "g1");
    assert_eq!(group["after"]["values"], serde_json::json!(["e", "u"]));
    assert!(group["after"].get("status").is_none());
    let attribution = &changes(&diff, "init2shocks")[0];
    assert_eq!(attribution["after"]["group"], "default");
    assert_eq!(attribution["after"]["values"][0], "u");
    let markdown = diff["markdown"].as_str().unwrap();
    assert!(markdown.contains("## Shock analysis setup"), "{markdown}");
    assert!(!markdown.contains("## Shock setup changes"), "{markdown}");
    assert!(markdown.contains("members e → e, u"), "{markdown}");
}

#[test]
fn ambiguous_group_rows_are_add_remove_and_group_rename_is_not_guessed() {
    let before =
        "varexo e u; shock_groups(name=g1); supply=e; end; shock_groups(name=g1); supply=u; end;";
    let after = "varexo e u; shock_groups(name=g1); supply=e,u; end; shock_groups(name=g1); supply=u,e; end;";
    let diff = compare(before, after);
    let groups = changes(&diff, "shock_group");
    assert_eq!(groups.len(), 4, "{diff}");
    assert_eq!(
        groups
            .iter()
            .filter(|row| row["change"] == "changed")
            .count(),
        0
    );

    let renamed = compare(
        "varexo e; shock_groups(name=g1); supply=e; end;",
        "varexo e; shock_groups(name=g2); supply=e; end;",
    );
    let groups = changes(&renamed, "shock_group");
    assert_eq!(groups.len(), 2, "{renamed}");
    assert!(groups.iter().all(|row| row["change"] != "changed"));
}

#[test]
fn direct_locations_are_safe_and_unrelated_context_stays_quiet() {
    let before =
        "varexo e u;\ninitval; e=1; u=4; end;\nmshocks; var e; periods 2; values 1.05; end;";
    let after = before.replace("e=1", "e=2");
    let parsed_before = parse(before);
    let parsed_after = parse(&after);
    let direct = compare_models_with_sources(
        &parsed_before,
        &parsed_after,
        Some(CompareSource {
            text: before,
            origin_uri: Some("before.mod"),
        }),
        Some(CompareSource {
            text: &after,
            origin_uri: Some("after.mod"),
        }),
    )
    .to_json();
    let row = context(&direct, "baseline", "e");
    assert_eq!(row["before"]["location"]["line"], 2);
    assert_eq!(row["after"]["origin_uri"], "after.mod");

    let unsafe_location = compare_models_with_sources(
        &parsed_before,
        &parsed_after,
        Some(CompareSource {
            text: "@#include \"before.inc\"",
            origin_uri: Some("before.mod"),
        }),
        Some(CompareSource {
            text: "@#include \"after.inc\"",
            origin_uri: Some("after.mod"),
        }),
    )
    .to_json();
    let row = context(&unsafe_location, "baseline", "e");
    assert!(row["before"].get("location").is_none(), "{unsafe_location}");
    assert!(
        row["after"].get("origin_uri").is_none(),
        "{unsafe_location}"
    );

    let unrelated = compare(before, &before.replace("u=4", "u=5"));
    assert!(
        changes(&unrelated, "shock_context").is_empty(),
        "{unrelated}"
    );
}

#[test]
fn analysis_rows_and_group_openers_have_separate_locations() {
    let before = "var y; varexo e u;\nshock_groups(name=g1);\n  supply=e;\nend;\ninit2shocks(name=allocation);\n  y,e;\nend;";
    let after = before
        .replace("supply=e;", "supply=e,u;")
        .replace("y,e;", "y,u;");
    let diff = compare_models_with_sources(
        &parse(before),
        &parse(&after),
        Some(CompareSource {
            text: before,
            origin_uri: Some("before.mod"),
        }),
        Some(CompareSource {
            text: &after,
            origin_uri: Some("after.mod"),
        }),
    )
    .to_json();
    let group = &changes(&diff, "shock_group")[0];
    assert_eq!(group["after"]["location"]["line"], 3);
    assert_eq!(group["after"]["group_location"]["line"], 2);
    assert_eq!(group["after"]["origin_uri"], "after.mod");
    let attribution = &changes(&diff, "init2shocks")[0];
    assert_eq!(attribution["after"]["location"]["line"], 6);
    assert_eq!(attribution["after"]["group_location"]["line"], 5);
}

#[test]
fn mcp_compare_exposes_context_change_with_direct_source_location() {
    let before = "varexo e;\nheteroskedastic_shocks; var e; periods 10; values 0.1; end;\nestimation(datafile='a.csv',first_obs=10);";
    let after = before.replace("first_obs=10", "first_obs=11");
    let diff = dygnosis::mcp::dynare_compare_models(
        before,
        &after,
        Some("before.mod"),
        Some("after.mod"),
        None,
        None,
        None,
    );
    let row = context(&diff, "data_source", "estimation.first_obs");
    assert_eq!(row["after"]["location"]["line"], 3);
    assert_eq!(row["after"]["origin_uri"], "after.mod");
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("Heteroskedastic data"));
}

#[test]
fn explicit_default_group_name_is_a_written_change() {
    let before = "var y; varexo e; shock_groups; supply=e; end; init2shocks; y,e; end;";
    let after = "var y; varexo e; shock_groups(name=default); supply=e; end; init2shocks(name=default); y,e; end;";
    let diff = compare(before, after);
    for form in ["shock_group", "init2shocks"] {
        let rows = changes(&diff, form);
        assert_eq!(rows.len(), 1, "{diff}");
        assert_eq!(rows[0]["change"], "changed");
        assert_eq!(rows[0]["before"]["group"], "default");
        assert_eq!(rows[0]["after"]["group"], "default");
        assert_eq!(rows[0]["before"]["group_explicit"], false);
        assert_eq!(rows[0]["after"]["group_explicit"], true);
    }
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("block name bare default → name=default"));
}

#[test]
fn one_written_baseline_has_one_context_change_for_multiple_references() {
    // Two path stanzas can read the same initval assignment in a valid 7.2 file.
    let before = "var y; varexo e u; model; y=e+u; end; initval; e=1; end; shock_paths; var e; periods 1; values initval.e; var u; periods 2; values initval.e; end;";
    let after = before.replace("e=1", "e=2");
    let diff = compare(before, &after);
    assert_eq!(changes(&diff, "shock_context").len(), 1, "{diff}");
    assert!(context(&diff, "baseline", "e")["after"]
        .get("related_target")
        .is_none());

    // This mix passes json=check but has an E113 transform clash. Comparison
    // still reports the shared written assignment only once.
    let before = "varexo e; initval; e=1; end; mshocks; var e; periods 2; values 1.05; end; shock_paths; var e; periods 1; values initval.e; end;";
    let after = before.replace("e=1", "e=2");
    let diff = compare(before, &after);
    assert_eq!(changes(&diff, "shock_context").len(), 1, "{diff}");
    assert_eq!(
        context(&diff, "baseline", "e")["after"]["related_setup"],
        "mshocks and shock_paths"
    );
}

#[test]
fn unchanged_context_does_not_look_removed_with_its_setup() {
    let cases = [
        (
            "varexo e; set_time(2024Q1); shocks; var e; periods 2024Q2; values 1; end;",
            "varexo e; set_time(2024Q1);",
        ),
        (
            "varexo e; database db; shock_paths; var e; periods 1; values db.x; end;",
            "varexo e; database db;",
        ),
        (
            "varexo e; heteroskedastic_shocks; var e; periods 10; values 0.1; end; estimation(datafile='a.csv',first_obs=10);",
            "varexo e; estimation(datafile='a.csv',first_obs=10);",
        ),
        (
            "varexo e; initval; e=1; end; mshocks; var e; periods 2; values 1.05; end;",
            "varexo e; initval; e=1; end;",
        ),
    ];
    for (before, after) in cases {
        let diff = compare(before, after);
        assert!(changes(&diff, "shock_context").is_empty(), "{diff}");
        assert!(!diff["shock_setup_changes"].as_array().unwrap().is_empty());
    }

    let before = "varexo e; initval; e=1; end; mshocks; var e; periods 2; values 1.05; end;";
    let after = "varexo e; initval; e=2; end;";
    let diff = compare(before, after);
    assert_eq!(context(&diff, "baseline", "e")["change"], "changed");
}

#[test]
fn installed_dynare_accepts_compare_context_neighbours() {
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
        "dygnosis_compare_context_{}_{}",
        std::process::id(),
        nonce
    ));
    std::fs::create_dir(&directory).expect("create isolated probe directory");
    let probes = [
        (
            "named.mod",
            "var y; varexo e u; model; y=e+u; end; shock_groups(name=default); supply=e,u; end; init2shocks(name=default); y,e; end;",
            true,
        ),
        (
            "bare.mod",
            "var y; varexo e; model; y=e; end; shock_groups; supply=e; end; init2shocks; y e; end;",
            true,
        ),
        (
            "estimation.mod",
            "var y; varexo e; model; y=e; end; varobs y; estimation(datafile='data.csv',first_obs=10);",
            true,
        ),
        (
            "data.mod",
            "var y; varexo e; model; y=e; end; varobs y; data(series=myseries,first_obs=2000Q1); estimation;",
            true,
        ),
        (
            "shared_initval.mod",
            "var y; varexo e u; model; y=e+u; end; initval; e=1; end; shock_paths; var e; periods 1; values initval.e; var u; periods 2; values initval.e; end;",
            true,
        ),
        (
            "bad_group.mod",
            "var y; varexo e; model; y=e; end; shock_groups(foo=group1); supply=e; end;",
            false,
        ),
    ];
    let mut results = Vec::new();
    for (name, text, accepted) in probes {
        std::fs::write(directory.join(name), text).expect("write probe");
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
        if name == "bad_group.mod" {
            assert!(
                message.contains("syntax error, unexpected IDENTIFIER, expecting NAME"),
                "{message}"
            );
        }
    }
}
