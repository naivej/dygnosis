//! Revalidate a captured comparison's source union without changing the live
//! configuration or repeating model analysis.

use super::*;
use crate::engine::comparison::compare_snapshots::{MAX_SNAPSHOT_FILES, MAX_SNAPSHOT_SOURCE_BYTES};
use crate::engine::workspace::InputGuard;
use crate::engine::workspace::include_resolver::normalize_uri;
use std::collections::BTreeMap;

pub(super) fn validate(inner: &Inner, arguments: &[Value]) -> Value {
    let refusal = |code: &str, message: &str| json!({"code":code,"error":message});
    let Some(inputs) = arguments
        .first()
        .and_then(|arg| arg.get("inputs"))
        .and_then(Value::as_array)
        .filter(|inputs| inputs.len() == 2)
    else {
        return refusal(
            "INVALID_ARGUMENTS",
            "Provide both captured comparison inputs before using their source links.",
        );
    };
    let mut guards = BTreeMap::<String, InputGuard>::new();
    let mut bytes = 0usize;
    for input in inputs {
        if input["kind"] == "git" {
            continue;
        }
        if input["kind"] != "working" {
            return refusal(
                "INVALID_ARGUMENTS",
                "The comparison input kind is unsupported. Refresh Model Diff.",
            );
        }
        let Some(root) = input
            .get("root_uri")
            .and_then(Value::as_str)
            .and_then(|root| Url::parse(root).ok())
            .filter(is_model_root)
        else {
            return refusal(
                "INVALID_ARGUMENTS",
                "The captured Working input lacks its model root. Refresh Model Diff.",
            );
        };
        let Some(rows) = input
            .get("guards")
            .and_then(Value::as_array)
            .filter(|rows| !rows.is_empty() && rows.len() <= MAX_SNAPSHOT_FILES)
        else {
            return refusal(
                "CAPTURE_LIMIT",
                "The captured source guards are missing or exceed the comparison file allowance. Refresh Model Diff.",
            );
        };
        let root_key = normalize_uri(root.as_str());
        let mut root_guarded = false;
        for row in rows {
            let Ok(guard) = serde_json::from_value::<InputGuard>(row.clone()) else {
                return refusal(
                    "INVALID_ARGUMENTS",
                    "A captured source guard is invalid. Refresh Model Diff.",
                );
            };
            root_guarded |= guard.file == root_key;
            if let Some(previous) = guards.get(&guard.file) {
                if *previous != guard {
                    return json!({"schema_version":1,"current":false,"cause":{"code":"INPUT_CHANGED","message":"The comparison sides do not share the same captured source. Refresh Model Diff."}});
                }
                continue;
            }
            bytes = bytes
                .saturating_add(guard.file.len())
                .saturating_add(guard.length.unwrap_or(0).min(usize::MAX as u64) as usize);
            if guards.len() >= MAX_SNAPSHOT_FILES || bytes > MAX_SNAPSHOT_SOURCE_BYTES {
                return refusal(
                    "CAPTURE_LIMIT",
                    "The captured source union exceeds the comparison allowance. Refresh Model Diff with fewer or smaller sources.",
                );
            }
            guards.insert(guard.file.clone(), guard);
        }
        if !root_guarded {
            return refusal(
                "INVALID_ARGUMENTS",
                "The captured source guards do not include their model root. Refresh Model Diff.",
            );
        }
    }
    let current = inner
        .workspace
        .source_guards_are_current(&guards.into_values().collect::<Vec<_>>());
    if current {
        json!({"schema_version":1,"current":true})
    } else {
        json!({"schema_version":1,"current":false,"cause":{"code":"INPUT_CHANGED","message":"A captured source or include lookup changed. Refresh Model Diff before using its source links."}})
    }
}
