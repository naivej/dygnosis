// Build current protocol snapshot receipts for diff UI tests.
/* global structuredClone */
const clone = value => structuredClone(value);
const emptySemantic = () => ({
  comparison_versions: { semantic: 2, source_changes: 2, coverage: 1 },
  semantic: { schema_version: 2, availability: "complete", budgets: { token_alignment_cells: 1, source_alignment_cells: 1, references_per_side: 1, source_hunks: 1, serialized_output_bytes: 1 }, rows: [], references: [], limits: [] },
  source_changes: { schema_version: 2, availability: "complete", files: [], limits: [], root_text: "matching", executed_includes: { before: [], after: [] } },
  coverage: { schema_version: 1, availability: "complete", source_boundary: "captured_roots_and_executed_includes", families: [], limits: [] },
});
const summaryFor = ({ semantic, source_changes: source }) => {
  const rows = semantic.rows.length, files = source.files.length;
  const unpaired = source.files.filter(file => file.correspondence === "unpaired").length;
  return { semantic_rows: rows, available_semantic_rows: rows, source_files: unpaired ? null : files, available_source_files: files,
    reference_membership_complete: true, semantic_availability: semantic.availability, source_availability: source.availability,
    root_text: source.root_text, executed_includes: source.executed_includes, paired_file_changes: files - unpaired, unpaired_files: unpaired, issues: [] };
};
exports.emptySemantic = emptySemantic;
exports.summaryFor = summaryFor;

exports.currentSnapshot = ({ beforeRoot, afterRoot, diff, sources = { before: {}, after: {} }, navigationRows = [], revisions = { before: "before-revision", after: "after-revision" } }) => {
  const ids = { before: "before", after: "after" }, roots = { before: beforeRoot, after: afterRoot };
  const currentDiff = clone(diff);
  const semantic = emptySemantic();
  currentDiff.comparison_versions ??= semantic.comparison_versions;
  currentDiff.semantic ??= semantic.semantic;
  currentDiff.source_changes ??= semantic.source_changes;
  currentDiff.coverage ??= semantic.coverage;
  currentDiff.summary ??= summaryFor(currentDiff);
  for (const file of currentDiff.source_changes.files) for (const side of ["before", "after"]) {
    if (file[side]) file[side].input_id = ids[side];
  }
  const mappedTarget = (side, target) => target == null ? null : ({
    ...clone(target),
    written_locations: target.written_locations.map(location => ({ input_id: ids[side], file_key: location.file_key ?? location.uri,
      ...(location.commit !== undefined ? { commit: location.commit } : {}), range: clone(location.range) })),
  });
  const rows = navigationRows.map(row => ({ ...clone(row), before: mappedTarget("before", row.before), after: mappedTarget("after", row.after),
    ...(row.block_openers ? { block_openers: { before: mappedTarget("before", row.block_openers.before), after: mappedTarget("after", row.block_openers.after) } } : {}) }));
  const input = side => ({ input_id: ids[side], root_file: roots[side], revision: revisions[side], complete: true });
  const inputs = { schema_version: 1, before: input("before"), after: input("after") };
  const navigation = { schema_version: 2, before: input("before"), after: input("after"), rows };
  return { state: "result", inputs, sources: clone(sources), navigation, diff: currentDiff };
};

exports.parseCurrentSnapshot = (parse, snapshot) => parse(snapshot, { before: "before", after: "after" }, (side, key) => {
  if (!Object.hasOwn(snapshot.sources[side], key)) throw new Error("unsupported comparison source key");
  return key;
}, {
  before: { root_file: snapshot.inputs.before.root_file, revision: snapshot.inputs.before.revision },
  after: { root_file: snapshot.inputs.after.root_file, revision: snapshot.inputs.after.revision },
});
