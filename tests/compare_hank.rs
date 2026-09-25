//! Heterogeneous `shocks(heterogeneity=…)` rows in the existing `shock_setup_changes` field.

use dygnosis::model_diff::{compare_models_with_sources, CompareSource};
use dygnosis::{compare_models, parse};

fn compare(before: &str, after: &str) -> serde_json::Value {
    compare_models(&parse(before), &parse(after)).to_json()
}

fn rows(diff: &serde_json::Value) -> &[serde_json::Value] {
    diff["shock_setup_changes"].as_array().expect("shock rows")
}

fn setting_dimension<'a>(row: &'a serde_json::Value, side: &str) -> Option<&'a str> {
    row.get(side)
        .and_then(|value| value.get("heterogeneity"))
        .and_then(|value| value.as_str())
}

fn het_rows<'a>(
    diff: &'a serde_json::Value,
    target: &str,
    dimension: Option<&str>,
) -> Vec<&'a serde_json::Value> {
    rows(diff)
        .iter()
        .filter(|row| row["target"] == target)
        .filter(|row| {
            setting_dimension(row, "before") == dimension
                || setting_dimension(row, "after") == dimension
        })
        .collect()
}

fn side_dimension(row: &serde_json::Value, side: &str) -> Option<String> {
    setting_dimension(row, side).map(str::to_string)
}

#[test]
fn same_dimension_variance_change_keeps_written_expression_and_location() {
    let before = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nshocks(heterogeneity=d);\nvar e = (0.1);\nend;\n";
    let after = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nshocks(heterogeneity=d);\nvar e = 0.2;\nend;\n";
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
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    let item = &het_rows(&diff, "e", Some("d"))[0];
    assert_eq!(item["form"], "stochastic_shock");
    assert_eq!(item["change"], "changed");
    assert_eq!(item["before"]["block"], "shocks");
    assert_eq!(item["before"]["measure"], "variance");
    assert_eq!(item["after"]["measure"], "variance");
    assert_eq!(item["before"]["heterogeneity"], "d");
    assert_eq!(item["after"]["heterogeneity"], "d");
    assert_eq!(item["before"]["values"][0], "(0.1)");
    assert_eq!(item["after"]["values"][0], "0.2");
    assert_eq!(item["before"]["overwrite"], false);
    assert_eq!(item["before"]["location"]["line"], 4);
    assert_eq!(item["before"]["origin_uri"], "before.mod");
    assert_eq!(item["after"]["origin_uri"], "after.mod");
    assert!(item.get("heterogeneity").is_none());
    let md = diff["markdown"].as_str().unwrap();
    let changed = md
        .lines()
        .find(|line| line.contains("stochastic shock"))
        .expect("changed row");
    assert_eq!(changed.matches("heterogeneity d").count(), 1, "{md}");
    assert!(changed.contains("(0.1)"), "{md}");

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
    let item = &het_rows(&mismatched, "e", Some("d"))[0];
    assert!(item["before"].get("location").is_none(), "{mismatched}");
    assert!(item["before"].get("origin_uri").is_none(), "{mismatched}");
}

#[test]
fn stderr_and_variance_stay_distinct_measures() {
    let before = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nshocks(heterogeneity=d);\nvar e; stderr 0.1;\nend;\n";
    let after = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nshocks(heterogeneity=d);\nvar e = 0.01;\nend;\n";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    let item = &het_rows(&diff, "e", Some("d"))[0];
    assert_eq!(item["change"], "changed");
    assert_eq!(item["before"]["measure"], "stderr");
    assert_eq!(item["after"]["measure"], "variance");
    assert_eq!(item["before"]["values"][0], "0.1");
    assert_eq!(item["after"]["values"][0], "0.01");
}

#[test]
fn covariance_and_correlation_stay_distinct_measures() {
    let before = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e u;\nshocks(heterogeneity=d);\nvar e, u = 0.2;\nend;\n";
    let after = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e u;\nshocks(heterogeneity=d);\ncorr u, e = 0.5;\nend;\n";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 1, "{diff}");
    let item = &het_rows(&diff, "e,u", Some("d"))[0];
    assert_eq!(item["change"], "changed");
    assert_eq!(item["role"], "pair");
    assert_eq!(item["before"]["measure"], "covariance");
    assert_eq!(item["after"]["measure"], "correlation");
    assert_eq!(item["before"]["written_target"], "e,u");
    assert_eq!(item["after"]["written_target"], "u,e");
    assert_eq!(item["before"]["values"][0], "0.2");
    assert_eq!(item["after"]["values"][0], "0.5");
}

#[test]
fn dimension_does_not_pair_with_another_dimension_or_ordinary_shocks() {
    let before = "heterogeneity_dimension d, e;\nvarexo(heterogeneity=d) e_d;\nvarexo(heterogeneity=e) e_e;\nvarexo e_o;\nshocks(heterogeneity=d);\nvar e_d = 0.1;\nend;\nshocks(heterogeneity=e);\nvar e_e = 0.2;\nend;\nshocks;\nvar e_o = 0.3;\nend;\n";
    let after = "heterogeneity_dimension d, e;\nvarexo(heterogeneity=d) e_d;\nvarexo(heterogeneity=e) e_e;\nvarexo e_o;\nshocks(heterogeneity=d);\nvar e_d = 0.4;\nend;\nshocks(heterogeneity=e);\nvar e_e = 0.5;\nend;\nshocks;\nvar e_o = 0.6;\nend;\n";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 3, "{diff}");
    for (target, dimension) in [("e_d", Some("d")), ("e_e", Some("e")), ("e_o", None)] {
        let matches = het_rows(&diff, target, dimension);
        assert_eq!(matches.len(), 1, "{target} {dimension:?} {diff}");
        assert_eq!(matches[0]["change"], "changed");
        assert_eq!(side_dimension(matches[0], "before").as_deref(), dimension);
        assert_eq!(side_dimension(matches[0], "after").as_deref(), dimension);
        if dimension.is_none() {
            assert!(matches[0]["before"].get("heterogeneity").is_none());
            assert!(matches[0]["after"].get("heterogeneity").is_none());
        }
    }

    let crossed = compare(
        "heterogeneity_dimension d, e;\nvarexo(heterogeneity=d) e;\nvarexo(heterogeneity=e) e;\nvarexo e;\nshocks(heterogeneity=d);\nvar e = 0.1;\nend;\n",
        "heterogeneity_dimension d, e;\nvarexo(heterogeneity=d) e;\nvarexo(heterogeneity=e) e;\nvarexo e;\nshocks(heterogeneity=e);\nvar e = 0.1;\nend;\nshocks;\nvar e = 0.1;\nend;\n",
    );
    assert!(
        rows(&crossed).iter().all(|row| row["change"] != "changed"),
        "{crossed}"
    );
    assert_eq!(rows(&crossed).len(), 3, "{crossed}");
    assert!(rows(&crossed).iter().any(|row| {
        row["change"] == "removed" && side_dimension(row, "before").as_deref() == Some("d")
    }));
    assert!(rows(&crossed).iter().any(|row| {
        row["change"] == "added" && side_dimension(row, "after").as_deref() == Some("e")
    }));
    assert!(rows(&crossed).iter().any(|row| {
        row["change"] == "added" && row.get("after").unwrap().get("heterogeneity").is_none()
    }));
}

#[test]
fn overwrite_supersedes_only_its_own_dimension() {
    let before = "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e w;
varexo(heterogeneity=e) u;
varexo z;
shocks(heterogeneity=d);
var e = 0.1;
end;
shocks(heterogeneity=e);
var u = 0.2;
end;
shocks;
var z = 0.3;
end;
";
    let after = "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e w;
varexo(heterogeneity=e) u;
varexo z;
shocks(heterogeneity=d);
var e = 0.1;
end;
shocks(heterogeneity=e);
var u = 0.2;
end;
shocks;
var z = 0.3;
end;
shocks(heterogeneity=d, overwrite);
var w = 0.4;
end;
";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 2, "{diff}");
    let superseded = &het_rows(&diff, "e", Some("d"))[0];
    assert_eq!(superseded["change"], "changed");
    assert_eq!(superseded["before"]["status"], "active");
    assert_eq!(superseded["after"]["status"], "superseded");
    assert_eq!(superseded["before"]["heterogeneity"], "d");
    assert_eq!(superseded["after"]["overwrite"], false);
    let added = &het_rows(&diff, "w", Some("d"))[0];
    assert_eq!(added["change"], "added");
    assert_eq!(added["after"]["values"][0], "0.4");
    assert_eq!(added["after"]["heterogeneity"], "d");
    assert_eq!(added["after"]["overwrite"], true);
    assert_eq!(added["after"]["status"], "active");
    assert!(
        rows(&diff)
            .iter()
            .all(|row| side_dimension(row, "before").as_deref() != Some("e")
                && side_dimension(row, "after").as_deref() != Some("e")),
        "{diff}"
    );
    assert!(rows(&diff).iter().all(|row| row["target"] != "z"), "{diff}");
    assert!(diff["markdown"]
        .as_str()
        .unwrap()
        .contains("superseded by overwrite"));

    let empty_after = "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e;
varexo(heterogeneity=e) u;
shocks(heterogeneity=d);
var e = 0.1;
end;
shocks(heterogeneity=e);
var u = 0.2;
end;
shocks(heterogeneity=d, overwrite);
end;
";
    let empty = compare(
        "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e;
varexo(heterogeneity=e) u;
shocks(heterogeneity=d);
var e = 0.1;
end;
shocks(heterogeneity=e);
var u = 0.2;
end;
",
        empty_after,
    );
    let resets: Vec<_> = rows(&empty)
        .iter()
        .filter(|row| row["form"] == "shock_reset")
        .collect();
    assert_eq!(resets.len(), 1, "{empty}");
    assert_eq!(resets[0]["change"], "added");
    assert_eq!(
        resets[0]["target"],
        "heterogeneous variance/covariance settings"
    );
    assert_eq!(resets[0]["after"]["heterogeneity"], "d");
    assert_eq!(resets[0]["after"]["block"], "shocks");
    assert_eq!(resets[0]["after"]["overwrite"], true);
    let d_rows = het_rows(&empty, "e", Some("d"));
    assert_eq!(d_rows.len(), 1, "{empty}");
    assert_eq!(d_rows[0]["after"]["status"], "superseded");
    assert!(het_rows(&empty, "u", Some("e")).is_empty(), "{empty}");

    let ordinary = compare(
        "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nvarexo z;\nshocks(heterogeneity=d);\nvar e = 0.1;\nend;\nshocks;\nvar z = 0.3;\nend;\n",
        "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nvarexo z;\nshocks(heterogeneity=d);\nvar e = 0.1;\nend;\nshocks;\nvar z = 0.3;\nend;\nshocks(overwrite);\nend;\n",
    );
    assert!(
        rows(&ordinary)
            .iter()
            .all(|row| side_dimension(row, "before").as_deref() != Some("d")
                && side_dimension(row, "after").as_deref() != Some("d")),
        "{ordinary}"
    );
    assert!(rows(&ordinary).iter().any(|row| row["target"] == "z"));

    let reset_only = "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e;
varexo(heterogeneity=e) u;
shocks(heterogeneity=d, overwrite);
end;
";
    let later_same = "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e;
varexo(heterogeneity=e) u;
shocks(heterogeneity=d, overwrite);
end;
shocks(heterogeneity=d, overwrite);
var e = 0.4;
end;
";
    let later = compare(reset_only, later_same);
    let reset = rows(&later)
        .iter()
        .find(|row| row["form"] == "shock_reset")
        .expect("reset");
    assert_eq!(reset["change"], "changed", "{later}");
    assert_eq!(reset["before"]["status"], "active");
    assert_eq!(reset["after"]["status"], "superseded");
    assert_eq!(reset["after"]["heterogeneity"], "d");
    let added_line = later["markdown"]
        .as_str()
        .unwrap()
        .lines()
        .find(|line| line.contains("added:"))
        .expect("added row");
    assert_eq!(added_line.matches("heterogeneity d").count(), 1, "{later}");

    let other_dimension = compare(
        reset_only,
        "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e;
varexo(heterogeneity=e) u;
shocks(heterogeneity=d, overwrite);
end;
shocks(heterogeneity=e, overwrite);
end;
",
    );
    assert!(
        rows(&other_dimension)
            .iter()
            .all(|row| side_dimension(row, "before").as_deref() != Some("d")
                && side_dimension(row, "after").as_deref() != Some("d")),
        "{other_dimension}"
    );
    assert_eq!(
        rows(&other_dimension)
            .iter()
            .filter(|row| row["form"] == "shock_reset")
            .count(),
        1,
        "{other_dimension}"
    );

    let ordinary_after_reset = compare(
        reset_only,
        "\
heterogeneity_dimension d, e;
varexo(heterogeneity=d) e;
varexo(heterogeneity=e) u;
shocks(heterogeneity=d, overwrite);
end;
shocks(overwrite);
end;
",
    );
    assert!(
        rows(&ordinary_after_reset)
            .iter()
            .all(|row| row["form"] != "shock_reset"
                || side_dimension(row, "after").as_deref() != Some("d")),
        "{ordinary_after_reset}"
    );
}

#[test]
fn unchanged_heterogeneous_row_is_absent() {
    let text = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e u;\nshocks(heterogeneity=d);\nvar e; stderr 0.1;\nvar e, u = 0.2;\nend;\n";
    let diff = compare(text, text);
    assert!(rows(&diff).is_empty(), "{diff}");
    assert!(!diff.to_string().contains("steady"));
}

#[test]
fn ambiguous_heterogeneous_rows_stay_added_and_removed() {
    let before = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nshocks(heterogeneity=d);\nvar e = 0.1;\nvar e = 0.2;\nend;\n";
    let after = "heterogeneity_dimension d;\nvarexo(heterogeneity=d) e;\nshocks(heterogeneity=d);\nvar e = 0.3;\nend;\n";
    let diff = compare(before, after);
    assert_eq!(rows(&diff).len(), 3, "{diff}");
    assert!(rows(&diff).iter().all(|row| row["change"] != "changed"));
    assert_eq!(
        rows(&diff)
            .iter()
            .filter(|row| row["change"] == "removed")
            .count(),
        2
    );
    assert_eq!(
        rows(&diff)
            .iter()
            .filter(|row| row["change"] == "added")
            .count(),
        1
    );
}
