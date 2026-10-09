const assert = require("node:assert/strict");
const test = require("node:test");
const { createHost, load, captured, snapshotResponse, deferred, flush, working, historical, comparison, commits, Uri, rootUri, anchorUri, baselineUri, Disposable, CancellationTokenSource, historyScheme, changesViewType } = require("./helpers/changes_host.cjs");
const { resourceData, resourceQuery, resourceName, selector } = require("../out/changes_resource");

function setup(t) { const env = createHost(); env.host.install(); t.after(() => env.host.registration.dispose()); return env; }
const fourChoices = ["With previous revision", "With revision…", "With branch or tag…", "With .mod file…"];
function activateHost(env) {
  const { host, service, vscode } = env; host.registeredChanges = 0;
  const registrations = { "./changes_editor": { registerChanges: value => { assert.equal(value, service); host.registeredChanges++; return new Disposable(); } },
    "./client": { DygnosisClient: class { constructor() { return service; } }, isAnalysisDocument: document => document.uri.scheme === "file" },
  };
  for (const [module, name] of [["project_status", "registerProjectStatus"], ["project_mcp", "registerProjectMcp"], ["origin_jumps", "registerOriginJumps"], ["quickfix", "registerDiagnosticActions"], ["lenses", "registerLenses"], ["color", "registerColors"], ["model_view", "registerModelView"], ["status", "registerStatus"], ["mcp", "registerMcp"], ["preview", "registerEffectivePreview"], ["help", "registerHelp"]]) registrations[`./${module}`] = { [name]: () => new Disposable() };
  const { activate } = load("extension", vscode, registrations), context = { subscriptions: [] };
  assert.equal(activate(context), service); return context;
}

test("Open changes offers exactly four choices in order and retains the Working anchor", async t => {
  const { host, vscode } = setup(t); await host.open();
  assert.deepEqual(host.calls.find(call => call.id === "vscode.openWith").args[2], { viewColumn: vscode.ViewColumn.Active, preview: false });
  assert.deepEqual(host.picks[0].items.map(item => item.label), fourChoices);
  assert.equal(host.picks[0].items[0].description, "Last committed model · ccccccc");
  assert.deepEqual(host.captures[0].resource.before, historical(commits.head, "main.mod", "HEAD"));
  assert.deepEqual(host.captures[0].resource.after, working()); assert.deepEqual(host.captures[0].resource.anchor, working());
  assert.equal(host.last().status, "ready"); assert.equal(host.viewType, changesViewType);
  assert.equal(host.providerOptions.supportsMultipleEditorsPerDocument, true);
  assert.equal(host.panels[0].webview.options.enableCommandUris, false); assert.match(host.panels[0].webview.html, /default-src 'none'/);
});

test("unavailable Git keeps all choices visible and file comparison uses selected Before and anchored After", async t => {
  const { host } = setup(t); host.gitUnavailable = true; host.pick = items => items.find(item => item.mode === "file");
  await host.open(); const choices = host.picks[0].items;
  assert.deepEqual(choices.map(item => item.label), fourChoices); assert.deepEqual(choices.map(item => item.available), [false, false, false, true]);
  assert.match(choices[0].description, /Git is unavailable/); assert.deepEqual(host.captures[0].resource.before, working(baselineUri));
  assert.deepEqual(host.captures[0].resource.after, working()); assert.match(host.dialogs[0].title, /Before.*After/);
});

test("the retained old historical anchor survives previous, revision, ref and file choices while HEAD points elsewhere", async t => {
  for (const mode of ["previous", "revision", "ref", "file"]) {
    const { host } = createHost(); host.install(); t.after(() => host.registration.dispose());
    host.editor.document = host.document(Uri.from({ scheme: "git", path: `${anchorUri.path}.git`, query: JSON.stringify({ path: anchorUri.fsPath, ref: commits.opened }) }));
    host.pick = items => items.find(item => item.mode === mode) ?? items.find(item => item.revision) ?? items[0];
    await host.open(); const resource = host.captures[0].resource;
    assert.deepEqual(resource.after, historical()); assert.deepEqual(resource.anchor, historical()); assert.equal(host.previous[0], commits.opened);
    assert.notEqual(resource.after.commit, commits.head);
    if (mode === "file") assert.deepEqual(resource.before, working(baselineUri));
    else assert.equal(resource.before.kind, "git");
  }
});

test("explicit Source Control resource selects the Working model even when a different text editor is active", async t => {
  const { host } = setup(t), selected = Uri.joinPath(rootUri, "selected.mod");
  host.editor.document = host.document(Uri.joinPath(rootUri, "unrelated.mod"));
  await host.open({ resourceUri: selected });
  assert.deepEqual(host.captures[0].resource.after, working(selected)); assert.deepEqual(host.captures[0].resource.anchor, working(selected));
  assert.equal(host.captures[0].resource.before.root_file, "selected.mod");
});

test("explicit known SCM includes with arbitrary or no extension use their proven model owner", async t => {
  const { host, service, vscode } = setup(t); host.editor.document = host.document(Uri.joinPath(rootUri, "unrelated.txt")); host.editor.document.languageId = "plaintext";
  const setLanguage = vscode.languages.setTextDocumentLanguage;
  vscode.languages.setTextDocumentLanguage = (document, language) => { assert.ok(host.owners.some(value => value.source.toString() === document.uri.toString() && value.root.toString() === anchorUri.toString()), "proven owner is retained before native language reassignment clears cached model facts"); host.modelInfo = () => undefined; return setLanguage(document, language); };
  for (const name of ["definitions.data", "definitions"]) {
    const included = Uri.joinPath(rootUri, name), unsaved = `// unsaved ${name}\r\nmodel; y=99; end;\r\n`, document = host.document(included, unsaved); document.languageId = "plaintext"; document.version = 2;
    host.rootFor = () => undefined; host.knownOwners = uri => uri.toString() === included.toString() ? [anchorUri] : [];
    const proofs = [];
    // Plaintext is outside the LSP selector: include-context document_version is null and its native version cannot match.
    host.modelInfo = (root, source, fresh) => { proofs.push({ root, source, fresh }); return source.toString() === root.toString() ? { revision: `revision:${root.toString()}`, complete: true, related_files: [{ kind: "include", path: included.fsPath }] } : undefined; };
    host.capture = (resource, cancel, history) => {
      assert.equal(document.languageId, "dynare"); assert.equal(document.getText(), unsaved, "native adoption preserves the unsaved include text");
      const result = captured(resource, history, host, service.currentInstance); result.texts.after[included.toString()] = document.getText(); result.sourceUris.after.set(included.toString(), included); result.inputs.after.file_keys.push(included.toString()); result.snapshot.rows[0].navigation.after.written_locations[0].uri = included.toString(); return result;
    };
    await host.open({ resourceUri: included }); assert.equal(host.failures.length, 0, host.failures.join("\n")); assert.equal(host.last().status, "ready"); assert.deepEqual(host.panels.at(-1).document.resource.after, working());
    assert.equal(host.panels.at(-1).document.resource.before.root_file, "main.mod"); assert.equal(document.languageId, "dynare");
    assert.equal(proofs.length, 1); assert.equal(proofs[0].source.toString(), proofs[0].root.toString(), "fresh membership proof uses the model root document context"); assert.equal(proofs[0].fresh, true);
    assert.ok(host.owners.some(value => value.source.toString() === included.toString() && value.root.toString() === anchorUri.toString()));
    if (name === "definitions.data") { host.source("after"); await flush(); assert.equal(host.shown.at(-1).document.getText(), unsaved); }
  }
  assert.equal(host.captures.length, 1, "both proven include entrances reuse the same root comparison");
});

test("plaintext SCM multi-owner selection chooses one freshly proved root and cancellation preserves the document", async t => {
  for (const operation of ["selected", "owner cancelled", "proof cancelled"]) {
    const { host } = setup(t), included = Uri.joinPath(rootUri, "definitions.data"), document = host.document(included), pending = deferred(); document.languageId = "plaintext"; document.version = 2;
    host.rootFor = value => value.uri.toString() === anchorUri.toString() ? anchorUri : undefined;
    host.knownOwners = uri => uri.toString() === included.toString() ? [anchorUri, baselineUri] : [];
    let cancelled;
    host.pick = (items, _options, cancel) => {
      if (items[0].root) { const owner = items.find(item => item.root.toString() === baselineUri.toString()); if (operation === "owner cancelled") { cancelled = { token: cancel, reply: owner }; return pending.promise; } return owner; }
      return items.find(item => item.mode === "file") ?? items[0];
    };
    const proof = { revision: "proved owner", complete: true, related_files: [{ kind: "include", path: included.fsPath }] };
    host.modelInfo = (root, source, fresh, cancel) => { assert.equal(source.toString(), root.toString(), "plaintext membership uses root/root fresh facts"); assert.equal(fresh, true); return operation === "proof cancelled" ? (cancelled = { token: cancel, reply: proof }, pending.promise) : proof; };
    const selected = host.open({ resourceUri: included }); await flush();
    if (operation === "selected") {
      await selected; assert.equal(host.failures.length, 0, host.failures.join("\n")); assert.deepEqual(host.panels[0].document.resource.after, working(baselineUri)); assert.equal(document.languageId, "dynare");
      assert.deepEqual(host.owners.map(value => value.root.toString()), [baselineUri.toString()]); assert.ok(host.picks[0].cancel);
    } else {
      assert.ok(cancelled); host.pick = items => items.find(item => item.mode === "file") ?? items[0];
      await host.open({ resourceUri: anchorUri }); assert.equal(cancelled.token.isCancellationRequested, true);
      pending.resolve(cancelled.reply); await selected; await flush(); assert.equal(document.languageId, "plaintext"); assert.equal(host.owners.length, 0); assert.equal(host.captures.length, 1);
      assert.deepEqual(host.panels[0].document.resource.after, working());
    }
  }
});

test("an unowned or disproved plaintext SCM include cannot become a model through a selected filename", async t => {
  for (const known of [false, true]) {
    const { host } = setup(t), included = Uri.joinPath(rootUri, "unowned.data"), document = host.document(included); document.languageId = "plaintext";
    host.rootFor = () => undefined; host.knownOwners = () => known ? [anchorUri] : []; host.select = () => [anchorUri];
    host.modelInfo = () => ({ revision: "current root", complete: true, related_files: [{ kind: "include", path: Uri.joinPath(rootUri, "different.data").fsPath }] });
    await host.open({ resourceUri: included }); assert.equal(host.panels.length, 0); assert.equal(host.owners.length, 0); assert.equal(document.languageId, "plaintext"); assert.match(host.failures[0], /does not include this source/);
  }
});

test("the file command keeps the original anchor across picker focus changes and cancellation preserves an existing tab", async t => {
  const { host } = setup(t); host.select = () => { host.editor.document = host.document(Uri.joinPath(rootUri, "other.mod")); return [baselineUri]; };
  await host.file(); assert.deepEqual(host.captures[0].resource.after, working());
  const document = host.panels[0].document, count = host.panels.length;
  host.pick = () => undefined; await host.open(); assert.equal(host.panels.length, count); assert.equal(document.disposed, false); assert.equal(host.last().status, "ready");
});

test("superseded Git preparation cannot display an obsolete picker after a newer comparison is ready", async t => {
  for (const operation of ["previous", "history", "refs", "capture", "rename"]) {
    const { host, git } = createHost(); host.install(); t.after(() => host.registration.dispose());
    const pending = deferred(), original = git[operation]; let delayed = true;
    git[operation] = async (...args) => { const value = await original(...args); return delayed ? (delayed = false, pending.promise.then(() => value)) : value; };
    if (operation === "rename") { host.editor.document = host.document(Uri.joinPath(rootUri, "renamed.mod")); host.rename = { before: "main.mod", after: "renamed.mod" }; }
    host.pick = items => items.find(item => item.mode === (operation === "history" ? "revision" : operation === "refs" ? "ref" : "previous")) ?? items[0];
    const obsolete = host.open(); await flush(); const olderPicker = host.picks.at(-1);
    host.editor.document = host.document(anchorUri); host.pick = items => items.find(item => item.mode === "file") ?? items[0];
    await host.open(); assert.equal(host.last().status, "ready", operation); const pickCount = host.picks.length, panelCount = host.panels.length, captureCount = host.captures.length;
    if (olderPicker) assert.equal(olderPicker.cancel?.isCancellationRequested, true, `${operation} picker receives invocation cancellation`);
    pending.resolve(); await obsolete; await flush();
    assert.equal(host.picks.length, pickCount, `${operation} must not display a late picker`); assert.equal(host.panels.length, panelCount, operation); assert.equal(host.captures.length, captureCount, operation);
    assert.deepEqual(host.panels.at(-1).document.resource.after, working());
  }
});

test("superseded baseline and revision-expression pickers receive cancellation and cannot replace the newer anchor", async t => {
  for (const operation of ["baseline", "expression"]) {
    const { host } = createHost(); host.install(); t.after(() => host.registration.dispose()); const pending = deferred(); let older;
    if (operation === "baseline") host.pick = (items, _options, cancel) => { older = { cancel, value: items.find(item => item.mode === "previous") }; return pending.promise; };
    else {
      host.pick = items => items.find(item => item.mode === "revision") ?? items.find(item => item.action === "enter");
      host.input = (_options, cancel) => { older = { cancel, value: "HEAD~1" }; return pending.promise; };
    }
    const obsolete = host.open(); await flush(); assert.ok(older, operation);
    const selected = Uri.joinPath(rootUri, "new-anchor.mod"); host.editor.document = host.document(selected); host.input = undefined; host.pick = items => items.find(item => item.mode === "file") ?? items[0];
    await host.open(); assert.equal(older.cancel.isCancellationRequested, true); const pickCount = host.picks.length;
    pending.resolve(older.value); await obsolete; await flush();
    assert.equal(host.picks.length, pickCount); assert.equal(host.captures.length, 1); assert.deepEqual(host.panels[0].document.resource.after, working(selected));
  }
});

test("superseded unknown-source recovery and historical owner choices cannot continue after newer explicit selection", async t => {
  for (const operation of ["source", "owner", "provenance"]) {
    const { host, git } = createHost(); host.install(); t.after(() => host.registration.dispose()); const pending = deferred(); let ownerChoice, provenanceChoice;
    if (operation === "owner") {
      const include = Uri.joinPath(rootUri, "written.data"); host.editor.document = host.document(Uri.from({ scheme: "git", path: `${include.path}.git`, query: JSON.stringify({ path: include.fsPath, ref: commits.opened }) })); host.knownOwners = () => [anchorUri];
      host.pick = (items, _options, cancel) => { ownerChoice = { value: items[0], cancel }; return pending.promise; };
    } else {
      host.editor.document = host.document(Uri.from({ scheme: "thirdparty", path: anchorUri.path }));
      if (operation === "source") { host.provenance = () => { throw new Error("Choose source revision"); }; host.select = () => pending.promise; }
      else git.provenance = (_document, signal) => { provenanceChoice = { signal }; return pending.promise; };
    }
    const obsolete = host.open(); await flush();
    host.knownOwners = undefined; host.select = undefined; host.pick = items => items.find(item => item.mode === "file") ?? items[0];
    await host.open({ resourceUri: anchorUri }); const picks = host.picks.length, inputs = host.inputs.length;
    if (ownerChoice) assert.equal(ownerChoice.cancel.isCancellationRequested, true);
    if (provenanceChoice) assert.equal(provenanceChoice.signal.aborted, true);
    pending.resolve(operation === "source" ? [anchorUri] : operation === "owner" ? ownerChoice.value : { repository: { rootUri }, commit: { hash: commits.opened, requested_ref: commits.opened }, file_key: "main.mod" });
    await obsolete; await flush(); assert.equal(host.picks.length, picks); assert.equal(host.inputs.length, inputs); assert.equal(host.captures.length, 1); assert.deepEqual(host.panels[0].document.resource.after, working());
  }
});

test("symbolic Git plaintext and third-party sources require an explicit revision whose source text matches", async t => {
  for (const source of ["symbolic Git", "third-party", "mismatched source"]) {
    const { host } = setup(t), uri = source === "symbolic Git" ? Uri.from({ scheme: "git", path: `${anchorUri.path}.git`, query: JSON.stringify({ path: anchorUri.fsPath, ref: "main" }) }) : Uri.from({ scheme: "thirdparty", path: anchorUri.path });
    host.editor.document = host.document(uri, source === "mismatched source" ? "unrelated displayed bytes" : '@#include computed_name\r\nvar y;\r\n');
    if (source === "symbolic Git") host.editor.document.languageId = "plaintext";
    host.provenance = () => { throw new Error("Source has no fixed provenance"); }; host.select = () => [anchorUri]; host.input = () => commits.opened;
    await host.open(); assert.equal(host.inputs.length, 1); assert.ok(host.inputs[0].cancel, "explicit source revision receives invocation cancellation");
    if (source === "mismatched source") { assert.equal(host.panels.length, 0); assert.match(host.failures[0], /does not match.*source revision/); }
    else { assert.equal(host.last().status, "ready"); assert.deepEqual(host.captures[0].resource.after, historical()); assert.deepEqual(host.captures[0].resource.anchor, historical()); }
  }
});

test("same ordered comparison reuses its custom document and split views share capture with independent filters", async t => {
  const { host } = setup(t); await host.openSaved(comparison()); await host.openSaved(comparison());
  assert.equal(host.custom.size, 1); assert.equal(host.panels.length, 1); assert.equal(host.captures.length, 1);
  const first = host.panels[0], second = await host.split(first.document), choices = { ...host.last(first).choices, search: "goods", layout: "stacked", expanded: { group: false } };
  host.message({ type: "choices", key: host.last(first).key, choices }, first);
  host.message({ type: "ready", key: host.last(second).key, choices: host.last(second).choices }, second);
  assert.equal(host.last(first).choices.search, "goods"); assert.equal(host.last(second).choices.search, ""); assert.equal(host.captures.length, 1);
  assert.equal(host.last(first).token, host.last(second).token);
  host.close(first); assert.equal(second.document.disposed, false); host.source("before", second); await flush(); assert.equal(host.shown.length, 1);
  host.message({ type: "refresh" }, second); await flush(); assert.equal(host.captures.length, 2); assert.equal(host.last(second).status, "ready");
});

test("Swap opens a reversed resource while Change comparison restores baseline-to-anchor direction and retains filters", async t => {
  const { host } = setup(t), original = comparison(); await host.openSaved(original);
  const panel = host.panels[0], choices = { ...host.last().choices, search: "calibration", layout: "stacked", expanded: { "occurrence:1": true } };
  host.message({ type: "choices", key: host.last().key, choices }); host.message({ type: "swap" }); await flush();
  const swapped = host.panels.at(-1), resource = swapped.document.resource;
  assert.deepEqual(resource.before, original.after); assert.deepEqual(resource.after, original.before); assert.deepEqual(resource.anchor, original.anchor);
  assert.equal(host.last(swapped).status, "ready");
  assert.equal(host.last(swapped).choices.search, "calibration"); assert.equal(Object.hasOwn(host.last(swapped).choices, "expanded"), false);
  host.pick = items => items.find(item => item.mode === "file"); host.message({ type: "changeComparison" }, swapped); await flush();
  const changed = host.panels.at(-1).document.resource;
  assert.deepEqual(changed.after, original.anchor); assert.deepEqual(changed.before, working(baselineUri)); assert.equal(panel.disposed, false);
});

test("revision picker loads bounded pages, accepts an explicit expression, and branch groups preserve the anchor", async t => {
  const { host, git } = setup(t); let pages = 0;
  git.history = async (_repository, cursor) => { pages++; return cursor ? { commits: [{ hash: commits.old, requested_ref: commits.old, message: "old include-only commit", commitDate: new Date("2026-10-01") }] } : { commits: [{ hash: commits.opened, requested_ref: commits.opened, message: "recent include-only commit", commitDate: new Date("2026-10-08") }], next: { commits: [commits.head], offset: 1 } }; };
  let more = true; host.pick = items => items.find(item => item.mode === "revision") ?? (more ? (more = false, items.find(item => item.action === "more")) : items.find(item => item.revision?.hash === commits.old));
  await host.open(); assert.equal(pages, 2); assert.equal(host.captures[0].resource.before.commit, commits.old); assert.deepEqual(host.captures[0].resource.after, working());
  host.pick = items => items.find(item => item.mode === "revision") ?? items.find(item => item.action === "enter"); host.input = () => "HEAD~1"; await host.open();
  assert.equal(host.panels.at(-1).document.resource.before.requested_ref, "HEAD~1"); assert.equal(host.panels.at(-1).document.resource.before.commit, commits.opened);
  host.pick = items => items.find(item => item.mode === "ref") ?? items.find(item => item.label === "release"); await host.open();
  const groups = host.picks.at(-1).items.filter(item => item.kind === -1).map(item => item.label); assert.deepEqual(groups, ["Local branches", "Remote-tracking branches", "Tags"]);
  assert.deepEqual(host.panels.at(-1).document.resource.after, working());
});

test("a Git rename requires a choice and missing-root path recovery retains the fixed revision", async t => {
  const { host } = setup(t); host.editor.document = host.document(Uri.joinPath(rootUri, "renamed.mod")); host.rename = { before: "main.mod", after: "renamed.mod" };
  host.pick = items => items.find(item => item.mode === "previous") ?? items[0]; await host.open();
  assert.match(host.picks[1].items[0].label, /main.mod.*renamed.mod/); assert.equal(host.captures[0].resource.before.root_file, "main.mod"); assert.equal(host.captures[0].resource.after.root_uri.endsWith("renamed.mod"), true);
  const retained = host.panels[0]; host.pick = items => items.find(item => item.mode === "previous"); await host.open(); assert.equal(host.panels.length, 1); assert.equal(retained.disposed, false);
  host.capture = () => { throw new host.ComparisonFailure("MISSING_ROOT", "Before root is absent", "before", "renamed.mod"); };
  await host.openSaved(comparison(historical(commits.old, "renamed.mod"))); assert.equal(host.last().status, "failure");
  host.capture = undefined; host.pick = items => typeof items[0] === "string" ? "main.mod" : items[0]; host.message({ type: "choosePath" }); await flush();
  const restored = host.panels.at(-1).document.resource; assert.equal(restored.before.commit, commits.old); assert.equal(restored.before.root_file, "main.mod");
});

test("historical include ownership uses a candidate root only after a proof at the opened revision", async t => {
  for (const included of [true, false]) {
    const { host, service } = createHost(); host.install(); t.after(() => host.registration.dispose());
    const invoking = Uri.joinPath(rootUri, "written.data"); host.editor.document = host.document(Uri.from({ scheme: "git", path: `${invoking.path}.git`, query: JSON.stringify({ path: invoking.fsPath, ref: commits.opened }) }));
    host.knownOwners = () => [anchorUri]; host.pick = items => items.find(item => item.root_file === "main.mod") ?? items.find(item => item.mode === "previous") ?? items[0];
    host.capture = (resource, cancel, history) => { const result = captured(resource, history, host, service.currentInstance); if (!included) result.inputs.after.file_keys = ["main.mod"]; return result; };
    await host.open(); assert.equal(host.captures[0].resource.before.root_file, "main.mod"); assert.equal(host.captures[0].resource.after.commit, commits.opened);
    if (included) { assert.equal(host.panels.length, 1); assert.equal(host.panels[0].document.resource.after.root_file, "main.mod"); }
    else { assert.equal(host.panels.length, 0); assert.match(host.failures[0], /does not include.*written.data.*bbbbbbb/); }
  }
});

test("Refresh retains selected commits after a branch moves", async t => {
  const { host, git } = setup(t); await host.openSaved(comparison(historical(commits.old, "main.mod", "release")));
  const nativeResolve = git.resolve; git.resolve = (repository, ref) => ref === "release" ? Promise.resolve({ hash: commits.moved, requested_ref: "release", parents: [], message: "new release" }) : nativeResolve(repository, ref);
  host.message({ type: "refresh" }); await flush(); assert.equal(host.captures.at(-1).resource.before.commit, commits.old);
});

test("an added row opens the verified Before file even though the row is absent", async t => {
  const { host, service } = setup(t); let expected;
  host.capture = (resource, cancel, history) => {
    const result = captured(resource, history, host, service.currentInstance), row = result.snapshot.rows[0];
    row.kind = "added"; row.before = null; row.navigation.before = null;
    const beforeKey = [...result.sourceUris.before.keys()][1], afterKey = [...result.sourceUris.after.keys()][1];
    expected = result.sourceUris.before.get(beforeKey).toString();
    result.snapshot.sourceChanges = { files: [{ correspondence: "selected_roots", before: { file_key: beforeKey, exact_text_available: true }, after: { file_key: afterKey, exact_text_available: true } }] };
    return result;
  };
  await host.openSaved(comparison()); host.source("before"); await flush();
  assert.equal(host.shown.length, 1); assert.equal(host.shown[0].document.uri.toString(), expected);
  assert.equal(host.shown[0].document.languageId, "dynare");
  assert.equal(host.shown[0].options.selection.start.line, 0);
  host.panels[0].document.result.snapshot.sourceChanges.files[0].correspondence = "unpaired";
  host.source("before"); await flush(); assert.equal(host.shown.length, 1, "an unpaired source file cannot supply a missing-side link");
});

test("an old source load or validation cannot stale a newer ready comparison", async t => {
  for (const kind of ["load", "validation"]) {
    const { host } = createHost(); host.install(); t.after(() => host.registration.dispose()); await host.openSaved(comparison()); const pending = deferred();
    if (kind === "load") host.loadText = () => pending.promise; else host.validate = () => pending.promise;
    host.source("after"); await flush(); host.loadText = undefined; host.validate = undefined;
    host.message({ type: "refresh" }); await flush(); const token = host.last().token; pending.resolve(undefined); await flush();
    assert.equal(host.last().status, "ready"); assert.equal(host.last().token, token); assert.equal(host.shown.length, 0);
  }
});

test("several contributing sources use an exact native picker and unavailable row sides stay disabled", async t => {
  const { host, service } = setup(t); host.capture = (resource, cancel, history) => {
    const result = captured(resource, history, host, service.currentInstance), row = result.snapshot.rows[0];
    row.navigation.before.written_locations.push({ uri: result.sourceUris.before.get(result.inputs.before.root_file).toString(), range: { start: { line: 1, character: 0 }, end: { line: 1, character: 3 } } }); row.navigation.after = null; return result;
  };
  await host.openSaved(comparison(historical(commits.old), historical())); host.pick = items => items[1]; host.source(); await flush();
  assert.equal(host.shown.length, 1); assert.match(host.picks[0].items[1].description, /Before.*line 2/); assert.equal(host.shown[0].options.selection.start.line, 1);
  host.source("after"); await flush(); assert.equal(host.shown.length, 1);
});

test("reopening a saved custom document captures again, retains selectors and restores per-view display state", async t => {
  const { host } = setup(t), resource = comparison(historical(commits.old), historical()); await host.openSaved(resource);
  const choices = { ...host.last().choices, search: "saved", layout: "stacked", sections: [] }; host.message({ type: "choices", key: host.last().key, choices }); host.close(host.panels[0]);
  await host.openSaved(JSON.parse(resourceQuery(resource))); assert.equal(host.captures.length, 2); assert.equal(host.last().status, "ready"); assert.equal(host.last().choices.search, "saved"); assert.equal(Object.hasOwn(host.last().choices, "sections"), false);
  assert.deepEqual(host.panels.at(-1).document.resource, resource);
});

test("a fresh host restores search and category selection while ignoring retired section and scope filters", async t => {
  const { host } = setup(t), resource = comparison(historical(commits.old), historical());
  await host.openSaved(resource);
  const saved = { ...host.last().choices, search: "saved across reload", group: "symbols:Symbols", sections: [], scope: "firms", groupVisibility: { "symbols:Symbols": false } };
  host.message({ type: "ready", key: host.last().key, choices: saved });
  assert.equal(host.last().choices.search, saved.search); assert.equal(host.last().choices.group, saved.group);
  for (const key of ["sections", "scope", "groupVisibility"]) assert.equal(Object.hasOwn(host.last().choices, key), false);
  const token = host.last().token;
  host.settings.set(anchorUri.toString(), { "diff.sections": [] });
  host.configChanged.fire({ affectsConfiguration: key => key === "dynare.diff.sections" });
  assert.equal(host.last().token, token); assert.equal(host.last().choices.group, saved.group);
});

test("Open changes from a custom tab uses its recorded anchor while activeTextEditor points elsewhere", async t => {
  const { host } = setup(t), anchor = historical(); await host.openSaved(comparison(working(baselineUri), anchor, anchor));
  host.editor.document = host.document(Uri.joinPath(rootUri, "wrong-active.mod")); host.pick = items => items.find(item => item.mode === "file");
  await host.open(); const resource = host.panels.at(-1).document.resource;
  assert.deepEqual(resource.after, anchor); assert.deepEqual(resource.anchor, anchor);
});

test("superseded capture is cancelled and its late result cannot replace the current generation", async t => {
  const { host, service } = setup(t); await host.openSaved(comparison()); const pending = deferred(); let calls = 0;
  host.capture = (resource, cancel, history) => ++calls === 1 ? pending.promise : captured(resource, history, host, service.currentInstance, false);
  host.message({ type: "refresh" }); await flush(); const older = host.captures.at(-1);
  host.message({ type: "refresh" }); await flush(); assert.equal(older.cancel.isCancellationRequested, true); assert.equal(host.last().status, "ready"); assert.deepEqual(host.last().rows, []);
  const current = host.last().token; pending.resolve(captured(older.resource, older.history, host)); await flush(); assert.equal(host.last().token, current); assert.deepEqual(host.last().rows, []);
});

test("user progress cancellation has an explicit recovery state and disposal cancels capture and all subscriptions", async t => {
  const { host } = setup(t); await host.openSaved(comparison()); const pending = deferred();
  host.capture = () => pending.promise; host.message({ type: "refresh" }); await flush(); const capture = host.captures.at(-1);
  host.progress.at(-1).source.cancel(); pending.reject(new Error("cancelled capture")); await flush();
  assert.equal(capture.cancel.isCancellationRequested, true); assert.equal(host.last().status, "failure"); assert.match(host.last().message, /cancel|Refresh/i);
  const later = deferred(); host.capture = () => later.promise; host.message({ type: "refresh" }); await flush(); const latest = host.captures.at(-1);
  host.registration.dispose(); assert.equal(latest.cancel.isCancellationRequested, true);
  assert.equal(host.changed.listeners.size, 0); assert.equal(host.configChanged.listeners.size, 0); assert.equal(host.panels[0].received.listeners.size, 0);
  assert.equal(host.panels[0].disposed, true); later.reject(new Error("disposed capture")); await flush();
});

test("Working dependency edits and missing-candidate creation stale only affected inputs; fixed histories ignore unrelated edits", async t => {
  const { host } = setup(t); await host.openSaved(comparison()); const ready = host.last();
  host.changed.fire({ reason: "file", uri: Uri.joinPath(rootUri, "unrelated.data").toString() }); assert.equal(host.last().status, "ready");
  const missing = host.captures[0].resource.after.root_uri.replace("main.mod", "missing.data");
  host.changed.fire({ reason: "file", uri: missing }); assert.equal(host.last().status, "stale"); assert.equal(host.last().rows.length, 1);
  host.source("before"); await flush(); assert.equal(host.shown.length, 0);
  await host.openSaved(comparison(historical(commits.old), historical())); const historyPanel = host.panels.at(-1), token = host.last().token;
  host.changed.fire({ reason: "file", uri: anchorUri.toString() }); assert.equal(host.last(historyPanel).status, "ready"); assert.equal(host.last(historyPanel).token, token);
  host.message({ type: "openSource", token: ready.token, rowId: "/changed_equations/0", side: "after" }, host.panels[0]); await flush(); assert.equal(host.shown.length, 0);
});

test("exact candidate file watchers and include settings invalidate, while presentation settings keep source actions current", async t => {
  const { host } = setup(t); await host.openSaved(comparison());
  const candidates = host.watchers.filter(watcher => watcher.pattern.baseUri.toString() === rootUri.toString()); assert.ok(candidates.length); assert.ok(candidates.every(watcher => watcher.pattern.pattern === "*"));
  for (const watcher of candidates) watcher.create.fire(Uri.joinPath(rootUri, "missing.data")); assert.equal(host.last().status, "stale");
  host.message({ type: "refresh" }); await flush(); const token = host.last().token;
  host.settings.set(anchorUri.toString(), { "diff.sections": [] }); host.configChanged.fire({ affectsConfiguration: key => key === "dynare.diff.sections" });
  assert.equal(host.last().status, "ready"); assert.equal(host.last().token, token); assert.equal(Object.hasOwn(host.last().choices, "sections"), false);
  await host.openSaved(comparison(historical(commits.old), historical())); const fixed = host.panels.at(-1);
  host.configChanged.fire({ affectsConfiguration: key => key === "dynare.searchPaths" }); assert.equal(host.last(fixed).status, "stale");
});

test("literal candidate watchers accept bracket and brace names, ignore unrelated events and retain native path case rules", async t => {
  const { host, service } = setup(t), candidate = Uri.joinPath(rootUri, "missing[1]{final}.data");
  host.capture = (resource, cancel, history) => { const result = captured(resource, history, host, service.currentInstance); result.inputs.after.dependency_candidates.push(candidate.toString()); return result; };
  await host.openSaved(comparison()); const watchers = host.watchers.filter(watcher => watcher.pattern.baseUri.toString() === rootUri.toString()); assert.ok(watchers.length);
  assert.ok(watchers.every(watcher => watcher.pattern.pattern === "*"));
  for (const watcher of watchers) watcher.create.fire(Uri.joinPath(rootUri, "missing1final.data")); assert.equal(host.last().status, "ready", "glob-looking names must not match a different path");
  for (const watcher of watchers) watcher.change.fire(Uri.joinPath(rootUri, "unrelated.data")); assert.equal(host.last().status, "ready");
  for (const watcher of watchers) watcher.create.fire(candidate); assert.equal(host.last().status, "stale");
  host.message({ type: "refresh" }); await flush(); const current = host.watchers.filter(watcher => !watcher.disposed), varied = Uri.from({ scheme: candidate.scheme, path: candidate.path.toUpperCase() });
  for (const watcher of current) watcher.remove.fire(varied); assert.equal(host.last().status, process.platform === "win32" ? "stale" : "ready");
});

test("root notifications compare the model revision while diagnostic input tokens remain independent", async t => {
  const { host } = setup(t); await host.openSaved(comparison());
  const expected = host.panels[0].document.result.working[0].expected, token = host.last().token;
  host.changed.fire({ reason: "input", root: anchorUri.toString(), inputRevision: "different-diagnostic-token", modelRevision: expected });
  assert.equal(host.last().status, "ready"); assert.equal(host.last().token, token);
  host.changed.fire({ reason: "input", root: anchorUri.toString(), inputRevision: "different-diagnostic-token", modelRevision: "changed-model-view-token" });
  assert.equal(host.last().status, "stale"); assert.equal(host.last().rows.length, 1);
});

test("engine restart rejects fixed-history source actions and cancelled restart results cannot publish", async t => {
  const { host, service } = setup(t); await host.openSaved(comparison(historical(commits.old), historical()));
  service.currentInstance++; host.changed.fire({ reason: "restart" }); assert.equal(host.last().status, "stale"); host.source(); await flush(); assert.equal(host.shown.length, 0);
  host.message({ type: "refresh" }); await flush(); assert.equal(host.last().status, "ready"); assert.equal(host.panels[0].document.instance, service.currentInstance);
});

test("engine lifecycle invalidation stales fixed histories before a replacement can finish and cancels a loading result", async t => {
  const { host, service } = setup(t); await host.openSaved(comparison(historical(commits.old), historical())); const oldInstance = service.currentInstance;
  host.changed.fire({ reason: "lifecycle" }); assert.equal(service.currentInstance, oldInstance); assert.equal(host.last().status, "stale");
  host.source(); host.message({ type: "rootTextDiff" }); await flush(); assert.equal(host.shown.length, 0); assert.equal(host.calls.some(call => call.id === "vscode.diff"), false);
  const pending = deferred(); host.capture = () => pending.promise; host.message({ type: "refresh" }); await flush(); const obsolete = host.captures.at(-1);
  host.changed.fire({ reason: "lifecycle" }); assert.equal(obsolete.cancel.isCancellationRequested, true); assert.equal(host.last().status, "stale");
  pending.resolve(captured(obsolete.resource, obsolete.history, host, oldInstance)); await flush(); assert.equal(host.last().status, "stale");
});

test("source actions use retained side identity, exact bytes and UTF-16 ranges; root text diff uses the same resources", async t => {
  const { host } = setup(t); await host.openSaved(comparison(historical(commits.old), historical()));
  const initial = host.last(); host.source("before"); await flush(); host.source("after"); await flush();
  assert.equal(host.shown.length, 2); assert.notEqual(host.shown[0].document.uri.toString(), host.shown[1].document.uri.toString());
  assert.equal(host.shown[0].document.uri.scheme, historyScheme); assert.equal(host.shown[0].document.getText().slice(3, 5), "😀");
  assert.deepEqual(host.shown[0].options.selection.start, { line: 0, character: 3 }); assert.deepEqual(host.shown[0].options.selection.end, { line: 0, character: 5 });
  assert.equal(host.revalidations.length, 0);
  for (const message of [{ rowId: "missing" }, { side: "outside" }, { token: initial.token - 1 }]) host.source("before", host.panels[0], message);
  await flush(); assert.equal(host.shown.length, 2);
  host.message({ type: "rootTextDiff", token: initial.token - 1 }); await flush();
  assert.equal(host.calls.some(call => call.id === "vscode.diff"), false);
  host.message({ type: "rootTextDiff", token: host.last().token }); await flush(); const call = host.calls.find(call => call.id === "vscode.diff");
  assert.equal(call.args[0].scheme, historyScheme); assert.equal(call.args[1].scheme, historyScheme); assert.equal(call.args[2], "main.mod (aaaaaaa) ↔ main.mod (bbbbbbb)");
});

test("Working source navigation revalidates before and after document load and refuses modified exact text", async t => {
  const { host } = setup(t); await host.openSaved(comparison()); const pending = deferred();
  host.loadText = () => pending.promise; host.source("after"); await flush(); host.validate = () => undefined; pending.resolve(); await flush();
  assert.equal(host.shown.length, 0); assert.equal(host.last().status, "stale");
  host.validate = undefined; host.loadText = undefined; host.message({ type: "refresh" }); await flush();
  const target = host.last().rows[0].navigation.after.written_locations[0], document = host.texts.get(target.uri); document.text += "modified";
  host.source("after"); await flush(); assert.equal(host.shown.length, 0); assert.equal(host.last().status, "stale");
});

test("historical source tabs retain captured bytes after the comparison closes and restore through a fresh provider", async t => {
  const { host } = setup(t); await host.openSaved(comparison(historical(commits.old), historical())); host.source(); await flush();
  const written = host.shown[0].document, provider = host.providers.get(historyScheme), text = written.getText(); host.close(host.panels[0]);
  assert.equal(await provider.provideTextDocumentContent(written.uri, new CancellationTokenSource().token), text);
  host.closeText(written); assert.equal(provider.owners(written.uri).length, 0);
  const saved = Uri.from({ scheme: historyScheme, path: written.uri.path, query: written.uri.query });
  assert.match(await provider.provideTextDocumentContent(saved, new CancellationTokenSource().token), /computed_name/);
});

test("failed or incomplete refresh clears authoritative rows and shows a distinct outcome", async t => {
  const { host } = setup(t); await host.openSaved(comparison());
  host.capture = () => { throw new host.ComparisonFailure("INCOMPLETE_INPUT", "Before model input is incomplete", "before"); };
  host.message({ type: "refresh" }); await flush(); assert.equal(host.last().status, "incomplete"); assert.deepEqual(host.last().rows, []);
  host.capture = () => { throw new Error("local object absent"); }; host.message({ type: "refresh" }); await flush(); assert.equal(host.last().status, "failure"); assert.deepEqual(host.last().rows, []);
});

test("comparison selectors survive reload without source bodies and invalid selectors fail explicitly", () => {
  const resource = comparison(historical(commits.old, "path/model.mod", "release"), historical());
  assert.deepEqual(resourceData(JSON.parse(resourceQuery(resource))), resource); assert.match(resourceName(resource), /aaaaaaa.*bbbbbbb.*dygnosis-changes$/);
  assert.equal(resourceQuery(resource).includes("source text"), false);
  for (const mutate of [value => value.schema_version = 2, value => value.before.commit = "HEAD", value => value.before.root_file = "../model.mod", value => value.after.repository_uri = "command:bad"]) {
    const value = JSON.parse(JSON.stringify(resource)); mutate(value); assert.throws(() => resourceData(value));
  }
  assert.equal(selector({ kind: "working", root_uri: "untitled:/draft.mod" }), true);
});

test("capture handshake pins IDs and Working revision while only engine-requested historical paths are loaded", async t => {
  const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
  const { captureComparison } = load("snapshot_compare", vscode); host.settings.set(rootUri.toString(), { searchPaths: ["common", "common"] }); let calls = 0;
  host.engine = (_command, [request]) => ++calls === 1 ? { state: "needs_sources", requests: [{ side: "before", input_id: request.before.input_id, file_keys: ["computed.data"] }] } : snapshotResponse(request);
  const result = await captureComparison(service, async () => git, history, comparison(), token());
  assert.deepEqual(host.loaded, ["computed.data"]); assert.equal(host.engineCalls.length, 2);
  assert.equal(host.engineCalls[0].args[0].before.input_id, host.engineCalls[1].args[0].before.input_id);
  assert.equal(host.engineCalls[0].args[0].after.expected_revision, host.engineCalls[1].args[0].after.expected_revision);
  assert.deepEqual(host.engineCalls[1].args[0].before.search_paths, ["common"]); assert.equal(Object.hasOwn(host.engineCalls[1].args[0].before.sources, "unused.data"), false);
  assert.equal(result.snapshot.rows[0].navigation.before.written_locations[0].uri.startsWith(`${historyScheme}:`), true); assert.equal(result.instance, 1);
});

test("capture refuses no-progress, wrong-side IDs, absent manifest keys and Working-source requests", async t => {
  for (const kind of ["no-progress", "wrong-id", "outside", "working"]) {
    const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
    const { captureComparison } = load("snapshot_compare", vscode);
    host.engine = (_command, [request]) => ({ state: "needs_sources", requests: [{ side: kind === "working" ? "after" : "before", input_id: kind === "wrong-id" ? request.after.input_id : request[kind === "working" ? "after" : "before"].input_id, file_keys: [kind === "outside" ? "not-in-tree.data" : kind === "no-progress" ? "main.mod" : "computed.data"] }] });
    await assert.rejects(captureComparison(service, async () => git, history, comparison(), token()), /progress|invalid historical|outside its captured tree/);
    assert.equal(host.engineCalls.length, 1);
  }
});

test("snapshot navigation rejects side-ID mismatches and unregistered file keys before source tabs can open", async t => {
  for (const wrong of ["input_id", "file_key"]) {
    const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
    const { captureComparison } = load("snapshot_compare", vscode);
    host.engine = (_command, [request]) => { const result = snapshotResponse(request); result.navigation.rows[0].before.written_locations[0][wrong] = wrong === "input_id" ? request.after.input_id : "unregistered.data"; return result; };
    await assert.rejects(captureComparison(service, async () => git, history, comparison(), token()), /unsupported comparison|outside its captured input/); assert.equal(host.shown.length, 0);
  }
});

test("snapshot results must retain the requested side, source identity and exact registered source map", async t => {
  const mutations = {
    "Git kind": result => result.inputs.before.kind = "working",
    "Git root": result => result.inputs.before.root_file = "another.mod",
    "Git repository": result => result.inputs.before.repository_uri = Uri.joinPath(rootUri, "other-repository").toString(),
    "Git commit": result => result.inputs.before.commit = commits.moved,
    "Git requested ref": result => result.inputs.before.requested_ref = "other-tag",
    "Git policy": result => result.inputs.before.source_policy = "editor_buffers_and_disk",
    "Working root": result => result.inputs.after.root_uri = baselineUri.toString(),
    "Working revision": result => result.inputs.after.expected_revision = "other-model-revision",
    "Working policy": result => result.inputs.after.source_policy = "git_tree",
    "duplicate file keys": result => result.inputs.before.file_keys.push(result.inputs.before.root_file),
    "missing root key": result => { result.inputs.before.file_keys = []; result.sources.before = {}; },
    "extra source body": result => result.sources.before["unregistered.data"] = "model; y=999; end;",
    "missing source body": result => delete result.sources.before[result.inputs.before.root_file],
    "substituted root body": result => result.sources.before[result.inputs.before.root_file] += "model; y=999; end;",
  };
  for (const [name, mutate] of Object.entries(mutations)) {
    const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
    const held = new Set(), retain = history.retain.bind(history), release = history.release.bind(history);
    history.retain = (holder, ...args) => { held.add(holder); return retain(holder, ...args); }; history.release = holder => { held.delete(holder); return release(holder); };
    const { captureComparison } = load("snapshot_compare", vscode);
    host.engine = (_command, [request]) => { const result = snapshotResponse(request); mutate(result); return result; };
    await assert.rejects(captureComparison(service, async () => git, history, comparison(), token()), /snapshot inputs|snapshot identity|source list|historical text/, name);
    assert.equal(held.size, 0, `${name} cannot retain navigable historical bytes`); assert.equal(host.shown.length, 0);
  }
});

test("historical requested includes must match acquired bytes and preserve the parser newline rule", async t => {
  for (const mutation of ["substituted include", "lone CR normalization", "CRLF replacement"]) {
    const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
    const originalCapture = git.capture; git.capture = async (...args) => { const capture = await originalCapture(...args); capture.sources["main.mod"].text += "// lone terminator\r"; return capture; };
    const { captureComparison } = load("snapshot_compare", vscode); let calls = 0;
    host.engine = (_command, [request]) => {
      if (++calls === 1) return { state: "needs_sources", requests: [{ side: "before", input_id: request.before.input_id, file_keys: ["computed.data"] }] };
      const result = snapshotResponse(request);
      if (mutation === "substituted include") result.sources.before["computed.data"] += "// substituted object\n";
      else result.sources.before["main.mod"] = mutation === "lone CR normalization" ? result.sources.before["main.mod"].replace(/\r(?!\n)/g, "\n") : result.sources.before["main.mod"].replace(/\r\n/g, "\n");
      return result;
    };
    const capture = captureComparison(service, async () => git, history, comparison(), token());
    if (mutation === "lone CR normalization") { const result = await capture; assert.match(result.texts.before["main.mod"], /computed_name\r\n/); assert.equal(result.texts.before["main.mod"].endsWith("\n"), true); }
    else await assert.rejects(capture, /historical text differs/, mutation);
    assert.deepEqual(host.loaded, ["computed.data"]);
  }
});

test("snapshot navigation must use the verified envelope and the exact historical commit", async t => {
  const mutations = {
    "navigation root": result => result.navigation.before.root_file = "other.mod",
    "navigation revision": result => result.navigation.before.revision = "other-revision",
    "navigation incomplete": result => result.navigation.before.complete = false,
    "historical target commit": result => result.navigation.rows[0].before.written_locations[0].commit = commits.moved,
    "missing historical target commit": result => delete result.navigation.rows[0].before.written_locations[0].commit,
    "Working target commit": result => result.navigation.rows[0].after.written_locations[0].commit = commits.old,
  };
  for (const [name, mutate] of Object.entries(mutations)) {
    const { host, service, vscode, git, token } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
    const held = new Set(), retain = history.retain.bind(history), release = history.release.bind(history);
    history.retain = (holder, ...args) => { held.add(holder); return retain(holder, ...args); }; history.release = holder => { held.delete(holder); return release(holder); };
    const { captureComparison } = load("snapshot_compare", vscode);
    host.engine = (_command, [request]) => { const result = snapshotResponse(request); mutate(result); return result; };
    await assert.rejects(captureComparison(service, async () => git, history, comparison(), token()), /unsupported comparison/, name); assert.equal(held.size, 0); assert.equal(host.shown.length, 0);
  }
});

test("snapshot capture refuses Working changes, cancellation, engine restart and unavailable historical capabilities", async t => {
  for (const kind of ["changed", "cancel", "restart", "capability"]) {
    const { host, service, vscode, git } = createHost(), { HistoricalSources } = load("history_sources", vscode), history = new HistoricalSources(async () => "unused"); t.after(() => history.dispose());
    const { captureComparison } = load("snapshot_compare", vscode), cancel = new CancellationTokenSource();
    if (kind === "changed") host.validate = () => undefined;
    if (kind === "capability") service.client.initializeResult.capabilities.experimental = {};
    host.engine = (_command, [request]) => { if (kind === "cancel") cancel.cancel(); if (kind === "restart") service.currentInstance++; return snapshotResponse(request); };
    await assert.rejects(captureComparison(service, async () => git, history, comparison(), cancel.token), /changed|cancelled|bundled binary/);
  }
});

test("extension activation registers Changes and updates context when its tab is focused", async () => {
  const env = createHost(), { host, vscode } = env, context = activateHost(env); assert.equal(host.registeredChanges, 1); await flush();
  host.activeTab = { input: new vscode.TabInputCustom(Uri.from({ scheme: "dygnosis-changes", path: "/saved.dygnosis-changes", query: resourceQuery(comparison()) }), changesViewType) };
  host.tabsChanged.fire(); await flush();
  const update = host.calls.filter(call => call.id === "setContext" && call.args[0] === "dygnosis.changesContext").at(-1); assert.equal(update.args[1], true);
  host.activeTab = undefined;
  host.editor.document = host.document(Uri.from({ scheme: "git", path: `${anchorUri.path}.git`, query: JSON.stringify({ path: anchorUri.fsPath, ref: commits.opened }) })); host.editor.document.languageId = "plaintext";
  host.editorChanged.fire(); await flush();
  const historicalContext = host.calls.filter(call => call.id === "setContext" && call.args[0] === "dygnosis.changesContext").at(-1); assert.equal(historicalContext.args[1], true, "a built-in fixed .mod.git source retains Open changes context even when inferred as plaintext");
  for (const item of context.subscriptions) item.dispose?.();
});

test("activation exposes unknown-source recovery and exact known include paths for arbitrary extensions", async t => {
  const env = createHost(), { host, service } = env, context = activateHost(env); t.after(() => context.subscriptions.forEach(item => item.dispose?.())); await flush();
  const readContext = name => host.calls.filter(call => call.id === "setContext" && call.args[0] === name).at(-1)?.args[1];
  for (const [uri, languageId] of [
    [Uri.from({ scheme: "git", path: `${anchorUri.path}.git`, query: JSON.stringify({ path: anchorUri.fsPath, ref: "main" }) }), "plaintext"],
    [Uri.from({ scheme: "thirdparty", path: anchorUri.path }), "dynare"],
  ]) {
    host.editor.document = host.document(uri); host.editor.document.languageId = languageId; host.editorChanged.fire(); await flush();
    assert.equal(readContext("dygnosis.changesContext"), true, `${uri.scheme} can choose fixed source provenance`); assert.equal(readContext("dygnosis.modelContext"), false);
  }
  const included = [Uri.joinPath(rootUri, "definitions.data").path, Uri.joinPath(rootUri, "definitions").path]; host.includedFiles = included;
  host.changed.fire({ reason: "input" }); await flush(); assert.deepEqual(readContext("dygnosis.knownIncludedPaths"), included);
  service.currentInstance++; host.includedFiles = []; host.changed.fire({ reason: "lifecycle" }); await flush(); assert.deepEqual(readContext("dygnosis.knownIncludedPaths"), []);
});

test("fresh owner facts enable plaintext historical include context without another editor or invalidation event", async t => {
  const env = createHost(), { host } = env, context = activateHost(env); t.after(() => context.subscriptions.forEach(item => item.dispose?.())); await flush();
  const readContext = name => host.calls.filter(call => call.id === "setContext" && call.args[0] === name).at(-1)?.args[1], known = new Set();
  host.knownOwners = uri => known.has(uri.toString()) ? [anchorUri] : [];
  for (const name of ["definitions.data", "definitions"]) {
    const written = Uri.joinPath(rootUri, name), historicalUri = Uri.from({ scheme: "git", path: `${written.path}.git`, query: JSON.stringify({ path: written.fsPath, ref: commits.opened }) });
    host.editor.document = host.document(historicalUri); host.editor.document.languageId = "plaintext"; host.editorChanged.fire(); await flush();
    assert.equal(readContext("dygnosis.changesContext"), false, `${name} has no current owner candidate`);
    known.add(written.toString()); host.includedFiles = [written.path]; host.modelUpdated.fire({ root: anchorUri }); await flush();
    assert.equal(readContext("dygnosis.changesContext"), true, `${name} gains the historical owner entrance after asynchronous facts arrive`);
    assert.equal(readContext("dygnosis.modelContext"), false); assert.deepEqual(readContext("dygnosis.knownIncludedPaths"), [written.path]); assert.equal(host.editor.document.uri.toString(), historicalUri.toString()); assert.equal(host.editor.document.languageId, "plaintext");
    known.clear(); host.includedFiles = []; host.modelUpdated.fire({ root: anchorUri }); await flush();
    assert.equal(readContext("dygnosis.changesContext"), false); assert.deepEqual(readContext("dygnosis.knownIncludedPaths"), []);
  }
  assert.equal(host.modelUpdated.listeners.size, 1); context.subscriptions.forEach(item => item.dispose?.()); assert.equal(host.modelUpdated.listeners.size, 0);
});

test("SCM contribution enables explicit resource selection independently of the active editor and includes proven paths", () => {
  const { contributes } = require("../package.json"), command = contributes.commands.find(item => item.command === "dygnosis.openChanges"), scm = contributes.menus["scm/resourceState/context"].find(item => item.command === command.command);
  assert.equal(command.enablement, undefined, "global command enablement cannot use the unrelated active editor context");
  assert.match(scm.when, /scmProvider == git/); assert.match(scm.when, /resourceExtname/); assert.match(scm.when, /resourcePath in dygnosis\.knownIncludedPaths/);
  assert.equal(scm.when.includes("dygnosis.changesContext"), false); assert.equal(scm.when.includes("inc"), false, "include eligibility comes from proven paths rather than an extension list");
  assert.ok(contributes.menus.commandPalette.some(item => item.command === command.command && item.when.includes("dygnosis.changesContext")));
});
