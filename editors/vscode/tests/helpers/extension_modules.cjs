// Resolve compiled extension modules for Node tests and native-host helpers.
const path = require("node:path");

const outputRoot = path.resolve(__dirname, "../../out");
const moduleFiles = Object.freeze({
  binary: "client/binary",
  captured_text: "comparison/captured_text",
  changes_editor: "comparison/changes_editor",
  changes_resource: "comparison/changes_resource",
  client: "client/client",
  color: "editor/color",
  diff: "comparison/diff",
  diff_view: "comparison/diff_view",
  extension: "extension",
  git_provenance: "comparison/git_provenance",
  git_source: "comparison/git_source",
  git_source_host: "comparison/git_source_host",
  help: "help/help",
  help_content: "help/help_content",
  history_sources: "comparison/history_sources",
  lenses: "editor/lenses",
  lifecycle: "client/lifecycle",
  mcp: "mcp/mcp",
  model_navigation: "editor/model_navigation",
  origin_jumps: "editor/origin_jumps",
  preview: "editor/preview",
  project_mcp: "mcp/project_mcp",
  project_status: "editor/project_status",
  protocol: "client/protocol",
  quickfix: "editor/quickfix",
  refusal: "client/refusal",
  rename_guard: "client/rename",
  semantic_view: "comparison/semantic_view",
  settings: "client/settings",
  snapshot_compare: "comparison/snapshot_compare",
  status: "editor/status",
});
const key = file => process.platform === "win32" ? file.toLowerCase() : file;
const moduleNames = new Map(Object.entries(moduleFiles).map(([name, relative]) => [key(path.join(outputRoot, relative + ".js")), name]));

function compiledModule(name) {
  if (!Object.hasOwn(moduleFiles, name)) throw new Error("Unknown extension test module: " + name);
  return path.join(outputRoot, moduleFiles[name] + ".js");
}

function importedModule(request, parent) {
  if (!parent?.filename || !request.startsWith(".") && !path.isAbsolute(request)) return undefined;
  const target = path.resolve(path.dirname(parent.filename), request);
  return moduleNames.get(key(target.endsWith(".js") ? target : target + ".js"));
}

module.exports = { compiledModule, importedModule };
