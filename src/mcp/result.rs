//! Versioned agent results, deterministic collection pages and atomic output limits.

use rmcp::model::{CallToolResult, ContentBlock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Serialized result ceiling. A complete edit proposal must fit in one response.
pub(crate) const OUTPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResultOptions {
    /// Compact omits optional prose, not collection rows. For large collections also set page_size and select sections or filters. Causes and edit guards remain.
    #[serde(default)]
    pub detail: Detail,
    /// Selected result sections. Omitted sections use the tool's default projection.
    #[serde(default)]
    pub sections: Vec<String>,
    /// Maximum rows per collection, from 1 to 1000. Set explicitly for large queries, for example 10 with compact detail. Omitted values return the full collection within limits, even with compact detail.
    pub page_size: Option<usize>,
    /// Opaque selection.continuation from the preceding page. Repeat the same inputs, filters, sections, detail and page_size. For model queries also supply the tool's original expected revision arguments. Read selection.collections for returned and available match counts; fetch another page only when needed.
    pub continuation: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Detail {
    #[default]
    Full,
    Compact,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct Reason {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct Outcome {
    pub status: String,
    pub reasons: Vec<Reason>,
}

/// The root object is identical in JSON text and supported structuredContent.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub(crate) struct ToolResult {
    pub schema_version: u32,
    pub outcome: Outcome,
    pub inputs: Vec<Value>,
    pub coverage: Option<Value>,
    /// Collection paging: collections.<name> has returned_count, available_matches, total_matches (null when unknown) and offset. continuation is the opaque next-page token, or null when no available matches remain.
    pub selection: Option<Value>,
    pub data: Value,
    /// Complete edit proposal, or null. inputs[] holds file, exists, directory, identity, text_sha256, byte_sha256 and length guards; missing candidates have exists=false and null hashes. revision binds these guards, source_policy, checked_owners and include_dirs. edits[] holds file, range (line, column, end_line, end_column), expected_text and new_text. Range positions are one-based Unicode scalars with exclusive ends. Verify all guards and expected text before applying the complete proposal with external file tools.
    pub proposal: Option<Value>,
    pub limits: Vec<Value>,
}

impl ToolResult {
    pub fn new(data: Value) -> Self {
        Self {
            schema_version: 3,
            outcome: Outcome {
                status: "ok".into(),
                reasons: Vec::new(),
            },
            inputs: Vec::new(),
            coverage: None,
            selection: None,
            data,
            proposal: None,
            limits: Vec::new(),
        }
    }

    pub fn cause(&mut self, status: &str, code: &str, message: impl Into<String>) {
        self.outcome.status = status.into();
        self.outcome.reasons.push(Reason {
            code: code.into(),
            message: message.into(),
        });
    }

    pub fn refused(code: &str, message: impl Into<String>) -> Self {
        let mut result = Self::new(Value::Null);
        result.cause("refused", code, message);
        result
    }

    pub fn wire(mut self) -> CallToolResult {
        if let Some(coverage) = &self.coverage {
            collect_work_limits(coverage, None, &mut self.limits);
        }
        let mut value = serde_json::to_value(&self).expect("result object");
        let build = |value: Value, status: &str| {
            let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
            result.structured_content = Some(value);
            result.is_error = Some(status == "refused");
            result
        };
        let full = build(value.clone(), &self.outcome.status);
        let size = serde_json::to_vec(&full)
            .expect("complete wire result")
            .len()
            + 512;
        if size > OUTPUT_BYTES {
            let had_proposal = self.proposal.is_some()
                || self
                    .data
                    .get("actions")
                    .and_then(Value::as_array)
                    .is_some_and(|a| a.iter().any(|r| r.get("proposal").is_some()));
            self.data = Value::Null;
            self.proposal = None;
            self.selection = None;
            self.cause(if had_proposal || self.outcome.status=="refused" { "refused" } else { "partial" }, "OUTPUT_LIMIT",
                if had_proposal { "The complete edit proposal exceeds the output limit. No edits were returned. Select a smaller action or model owner set." }
                else { "The requested detail exceeds the output limit. No result rows were returned. Request compact detail, selected sections or a bounded page." });
            self.limits.push(json!({"code":"OUTPUT_LIMIT","ceiling_bytes":OUTPUT_BYTES,"requested_bytes":size,"omitted":"result detail"}));
            value = serde_json::to_value(&self).expect("limited result object");
        }
        let limited = build(value, &self.outcome.status);
        if serde_json::to_vec(&limited)
            .expect("limited wire result")
            .len()
            + 512
            > OUTPUT_BYTES
        {
            self.inputs.clear();
            self.coverage = None;
            let original_codes: Vec<_> = self
                .outcome
                .reasons
                .iter()
                .take(32)
                .map(|reason| reason.code.chars().take(64).collect::<String>())
                .collect();
            self.outcome.reasons=vec![Reason{code:"OUTPUT_LIMIT".into(),message:
                "The result and its metadata exceed the output limit. No facts or proposal are available. Extended outcome text, input guards and coverage were omitted; original outcome codes are retained in limits. Use shorter arguments, compact detail or a smaller owner set.".into()}];
            self.limits = vec![json!({"code":"OUTPUT_LIMIT","ceiling_bytes":OUTPUT_BYTES,
                "omitted":"result rows, extended outcome text, input guards and coverage", "original_outcome_codes":original_codes})];
            return build(
                serde_json::to_value(&self).expect("minimal result"),
                &self.outcome.status,
            );
        }
        limited
    }
}

/// Coverage carries typed engine work stops; prose or other unavailable facts
/// cannot become a reached budget in the canonical result.
fn collect_work_limits(coverage: &Value, side: Option<&str>, records: &mut Vec<Value>) {
    match coverage {
        Value::Object(object) => {
            if let Some(stops) = object.get("work_limits").and_then(Value::as_array) {
                for stop in stops {
                    let mut stop = stop.clone();
                    if let Some(side) = side {
                        stop["side"] = json!(side);
                    }
                    if !records.contains(&stop) {
                        records.push(stop);
                    }
                }
            }
            for (name, value) in object
                .iter()
                .filter(|(name, _)| name.as_str() != "work_limits")
            {
                collect_work_limits(
                    value,
                    if matches!(name.as_str(), "before" | "after") {
                        Some(name)
                    } else {
                        side
                    },
                    records,
                );
            }
        }
        Value::Array(roots) => {
            for root in roots {
                collect_work_limits(root, side, records);
            }
        }
        _ => {}
    }
}

pub(crate) fn output_schema() -> std::sync::Arc<serde_json::Map<String, Value>> {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<ToolResult>();
    let mut object = schema.as_object().expect("result schema object").clone();
    object.remove("title");
    object.insert(
        "required".into(),
        json!([
            "schema_version",
            "outcome",
            "inputs",
            "coverage",
            "selection",
            "data",
            "proposal",
            "limits"
        ]),
    );
    if let Some(status) = object
        .get_mut("definitions")
        .and_then(|definitions| definitions.get_mut("Outcome"))
        .and_then(|outcome| outcome.get_mut("properties"))
        .and_then(|properties| properties.get_mut("status"))
    {
        *status = json!({"type":"string","enum":["ok","changed","unchanged","partial","refused"]});
    }
    object.insert("properties".into(), {
        let mut properties = object["properties"]
            .as_object()
            .expect("result properties")
            .clone();
        properties.insert("schema_version".into(), json!({"const":3}));
        Value::Object(properties)
    });
    std::sync::Arc::new(object)
}

#[derive(Debug, Deserialize, Serialize)]
struct Cursor {
    tool: String,
    revision: String,
    query: String,
    offsets: std::collections::BTreeMap<String, usize>,
}

pub(crate) struct Pages {
    cursor: Cursor,
    size: Option<usize>,
    collections: serde_json::Map<String, Value>,
    more: bool,
    filters: Value,
    remaining: Option<usize>,
}

impl Pages {
    pub fn new(
        tool: &str,
        revision: &str,
        query: &Value,
        options: &ResultOptions,
    ) -> Result<Self, String> {
        if options
            .page_size
            .is_some_and(|size| !(1..=1000).contains(&size))
        {
            return Err("page_size must be a whole number from 1 to 1000".into());
        }
        let mut query = query.clone();
        if let Some(object) = query.as_object_mut() {
            for key in [
                "continuation",
                "expected_revision",
                "expected_revisions",
                "expected_revision_a",
                "expected_revision_b",
                "file_content",
                "files",
                "source",
                "sources",
                "active_file",
                "file_content_a",
                "file_content_b",
                "files_a",
                "files_b",
            ] {
                object.remove(key);
            }
        }
        let mut filters = query.clone();
        if let Some(object) = filters.as_object_mut() {
            for key in ["detail", "sections", "page_size"] {
                object.remove(key);
            }
        }
        let query = format!("{:x}", Sha256::digest(serde_json::to_vec(&json!({"filters":filters,"sections":options.sections,"detail":options.detail,"page_size":options.page_size})).expect("query JSON")));
        let cursor = if let Some(token) = &options.continuation {
            let cursor: Cursor =
                serde_json::from_str(token).map_err(|_| "Invalid continuation token")?;
            if cursor.tool != tool || cursor.revision != revision || cursor.query != query {
                return Err("INPUT_CHANGED: Continuation does not match this tool, input revision or query. Start a new first page.".into());
            }
            cursor
        } else {
            Cursor {
                tool: tool.into(),
                revision: revision.into(),
                query,
                offsets: Default::default(),
            }
        };
        if options.continuation.is_some() && options.page_size.is_none() {
            return Err("A continuation requires the original page_size".into());
        }
        Ok(Self {
            cursor,
            size: options.page_size,
            collections: Default::default(),
            more: false,
            filters,
            remaining: None,
        })
    }

    /// Canonical collections share one row allowance across the response.
    pub fn share_budget(&mut self) {
        self.remaining = self.size;
    }

    #[cfg(test)]
    pub fn collection(&mut self, name: &str, rows: &mut Vec<Value>) {
        self.collection_with_coverage(name, rows, true);
    }

    pub fn collection_with_coverage(&mut self, name: &str, rows: &mut Vec<Value>, complete: bool) {
        let total = rows.len();
        let offset = self
            .cursor
            .offsets
            .get(name)
            .copied()
            .unwrap_or(0)
            .min(total);
        let allowance = self.remaining.or(self.size).unwrap_or(total);
        let end = offset.saturating_add(allowance).min(total);
        let selected = rows.drain(offset..end).collect();
        *rows = selected;
        if let Some(remaining) = &mut self.remaining {
            *remaining = remaining.saturating_sub(rows.len());
        }
        self.cursor.offsets.insert(name.into(), end);
        self.more |= end < total;
        self.collections.insert(name.into(), json!({"total_matches":complete.then_some(total),"available_matches":total,"returned_count":rows.len(),"offset":offset}));
    }

    pub fn finish(self, options: &ResultOptions) -> Option<Value> {
        if self.collections.is_empty() {
            return None;
        }
        Some(
            json!({"filters":self.filters,"sections":options.sections,"detail":options.detail,"collections":self.collections,
            "continuation": self.more.then(|| serde_json::to_string(&self.cursor).expect("cursor JSON"))}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_cause_and_canonical_json_are_identical_for_text_and_structured_clients() {
        let mut result = ToolResult::new(json!([]));
        result.cause(
            "partial",
            "MISSING_INCLUDE",
            "The include was not supplied.",
        );
        let wire = result.wire();
        let ContentBlock::Text(last) = wire.content.last().unwrap() else {
            panic!("JSON text");
        };
        assert_eq!(
            serde_json::from_str::<Value>(&last.text).unwrap(),
            wire.structured_content.unwrap()
        );
        assert_eq!(wire.is_error, Some(false));
    }

    #[test]
    fn continuation_rejects_changed_filter_and_source_revision() {
        let options = ResultOptions {
            page_size: Some(1),
            ..Default::default()
        };
        let query = json!({"code":"W013","page_size":1});
        let mut pages = Pages::new("diagnose", "one", &query, &options).unwrap();
        let mut rows = vec![json!(1), json!(2)];
        pages.collection("diagnostics", &mut rows);
        let token = pages.finish(&options).unwrap()["continuation"]
            .as_str()
            .unwrap()
            .to_owned();
        let next = ResultOptions {
            continuation: Some(token),
            ..options
        };
        assert!(Pages::new("diagnose", "two", &query, &next).is_err());
        assert!(
            Pages::new(
                "diagnose",
                "one",
                &json!({"code":"E001","page_size":1}),
                &next
            )
            .is_err()
        );
        let mut pages = Pages::new("diagnose", "one", &query, &next).unwrap();
        let mut rows = vec![json!(1), json!(2)];
        pages.collection("diagnostics", &mut rows);
        assert_eq!(rows, vec![json!(2)]);
        assert!(pages.finish(&next).unwrap()["continuation"].is_null());
    }

    #[test]
    fn output_limit_never_returns_an_executable_subset_of_a_proposal() {
        let mut result = ToolResult::new(json!({"changed_text":"x".repeat(OUTPUT_BYTES)}));
        result.proposal = Some(json!({"edits":[{"new_text":"replacement"}]}));
        let wire = result.wire();
        let value = wire.structured_content.unwrap();
        assert_eq!(value["outcome"]["status"], "refused");
        assert!(value["proposal"].is_null());
        assert!(value["data"].is_null());
        assert_eq!(wire.is_error, Some(true));
    }

    #[test]
    fn output_limit_also_bounds_oversized_outcome_and_guard_metadata() {
        // Guards a large catalog refusal or supplied key surviving the final
        // metadata fallback and exceeding the advertised response ceiling.
        let mut result = ToolResult::refused("UNKNOWN_COMMAND", "x".repeat(OUTPUT_BYTES));
        result.inputs.push(json!({"root":"x".repeat(OUTPUT_BYTES)}));
        let wire = result.wire();
        assert!(serde_json::to_vec(&wire).unwrap().len() + 512 <= OUTPUT_BYTES);
        assert_eq!(wire.is_error, Some(true));
        let canonical = wire.structured_content.unwrap();
        assert!(canonical["proposal"].is_null());
        assert_eq!(canonical["outcome"]["status"], "refused");
        assert!(
            canonical["limits"][0]["original_outcome_codes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|code| code == "UNKNOWN_COMMAND")
        );
    }

    #[test]
    fn incomplete_collection_does_not_claim_an_exact_total() {
        let options = ResultOptions::default();
        let mut pages = Pages::new(
            "equations",
            "one",
            &json!({"tags":{"name":"test"}}),
            &options,
        )
        .unwrap();
        pages.collection_with_coverage("equations", &mut Vec::new(), false);
        let result = pages.finish(&options).unwrap();
        assert!(result["collections"]["equations"]["total_matches"].is_null());
        assert_eq!(result["collections"]["equations"]["returned_count"], 0);
        assert_eq!(result["filters"]["tags"]["name"], "test");
    }
    #[test]
    fn side_work_stops_reach_the_canonical_limits_without_classifying_prose() {
        let mut result = ToolResult::new(Value::Null);
        result.coverage = Some(
            json!({"before":{"work_limits":[{"code":"MACRO_LIMIT","limit":"iteration work","ceiling":1000000}]},
            "after":{"reasons":[{"code":"I211","message":"Valid macro expression needs unavailable evaluation"}]}}),
        );
        let result = result.wire().structured_content.unwrap();
        assert_eq!(result["limits"].as_array().unwrap().len(), 1);
        assert_eq!(result["limits"][0]["side"], "before");
    }
}
