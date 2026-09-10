use dygnosis::{command_options, is_known_command, list_options, option_doc};

fn to_value(payload: impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(payload).unwrap()
}

#[test]
fn list_options_omitted_matches_expected() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.omitted.json")).unwrap();
    assert_eq!(to_value(list_options(None)), expected);
    assert_eq!(to_value(list_options(Some(""))), expected);
}

#[test]
fn list_options_known_matches_expected() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    for (key, payload) in expected.as_object().unwrap() {
        assert_eq!(
            to_value(list_options(Some(key))),
            *payload,
            "known mismatch for {key}"
        );
    }
    assert_eq!(
        to_value(list_options(Some("Stoch_Simul"))),
        expected["stoch_simul"]
    );
}

#[test]
fn list_options_unknown_matches_expected() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.unknown.json")).unwrap();
    for (input, payload) in expected.as_object().unwrap() {
        assert_eq!(
            to_value(list_options(Some(input))),
            *payload,
            "unknown mismatch for {input:?}"
        );
    }
}

#[test]
fn command_options_accessors() {
    assert!(command_options("nope").is_empty());
    assert!(command_options("").is_empty());
    let known: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    let names: Vec<&str> = command_options("STOCH_SIMUL")
        .iter()
        .map(|(name, _)| *name)
        .collect();
    let expected: Vec<&str> = known["stoch_simul"]["options"]
        .as_array()
        .unwrap()
        .iter()
        .map(|opt| opt["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, expected);
    assert!(is_known_command("STOCH_SIMUL"));
    assert!(!is_known_command("nope"));
}

#[test]
fn option_doc_matches_expected() {
    let docs: serde_json::Value =
        serde_json::from_str(include_str!("expected/option_docs.json")).unwrap();
    for (key, val) in docs.as_object().unwrap() {
        assert_eq!(
            option_doc(key),
            val.as_str().unwrap(),
            "option_doc mismatch for {key}"
        );
    }
    assert_eq!(option_doc("nsam"), "");
    assert_eq!(option_doc("Nsam"), docs["Nsam"].as_str().unwrap());

    let known: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    let mut saw_absent = false;
    for payload in known.as_object().unwrap().values() {
        for opt in payload["options"].as_array().unwrap() {
            let name = opt["name"].as_str().unwrap();
            if !docs.as_object().unwrap().contains_key(name) {
                assert_eq!(option_doc(name), "");
                saw_absent = true;
            }
        }
    }
    assert!(
        saw_absent,
        "expected a COMMAND_OPTIONS name absent from OPTION_DOCS"
    );
}
