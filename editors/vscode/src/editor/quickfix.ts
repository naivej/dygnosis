import { refuse, runCommand } from "../client/refusal";
import * as vscode from "vscode";
import { randomUUID } from "node:crypto";
import { DocumentDiagnosticRequest, Middleware, WorkspaceDiagnosticRequest, WorkspaceDocumentDiagnosticReport, vsdiag } from "vscode-languageclient/node";
import { DygnosisClient, isAnalysisDocument } from "../client/client";
import { record } from "../client/protocol";
import { booleanSetting } from "../client/settings";
import { fileLabel } from "../client/failure";

type PushNext = NonNullable<Middleware["handleDiagnostics"]> extends
  (uri: vscode.Uri, diagnostics: vscode.Diagnostic[], next: infer Next) => void ? Next : never;
interface RawSet {
  uri: vscode.Uri; items: vscode.Diagnostic[]; version: number | null;
  resultId?: string; next?: PushNext;
}
interface ActionContext {
  uri: vscode.Uri; version: number; instance: number; diagnostic: vscode.Diagnostic;
}
interface PendingEdit {
  edit: vscode.WorkspaceEdit; versions: Map<string, number | null>; instance: number; folder: number;
  documentUri: string; documentVersion: number; diagnostics: vscode.Diagnostic[];
  roots: { root: string; revision: string }[];
}

/** Keep the diagnostic object intact: the language client's subclass carries LSP data. */
export function diagnosticCode(diagnostic: vscode.Diagnostic): string | undefined {
  const code = diagnostic.code;
  const value = typeof code === "object" ? code.value : code;
  return typeof value === "number" || (typeof value === "string" && value.length > 0) ? String(value) : undefined;
}
function ownershipAgrees(left: vscode.Diagnostic, right: vscode.Diagnostic): boolean {
  const leftData = "data" in left && record(left.data) ? left.data : undefined;
  const rightData = "data" in right && record(right.data) ? right.data : undefined;
  return ["root", "input_revision", "writing_context"].every(key => {
    const leftValue = leftData?.[key];
    const rightValue = rightData?.[key];
    if (leftValue === undefined && rightValue === undefined) return true;
    if (leftValue === undefined || rightValue === undefined) return false;
    return JSON.stringify(leftValue) === JSON.stringify(rightValue);
  });
}
function sameDiagnostic(left: vscode.Diagnostic, right: vscode.Diagnostic): boolean {
  if (!ownershipAgrees(left, right)) return false;
  return left === right || (left.source === right.source && left.message === right.message &&
    left.severity === right.severity && diagnosticCode(left) === diagnosticCode(right) &&
    left.range.isEqual(right.range));
}
function overlapping(left: vscode.Range, right: vscode.Range): boolean {
  return left.start.isBeforeOrEqual(right.end) && right.start.isBeforeOrEqual(left.end);
}

/** Register before startup so push paint and Ignore share one window filter; pull stays empty here. */
export function registerDiagnosticActions(service: DygnosisClient): vscode.Disposable {
  const hiddenCodes = new Set<string>();
  const pushed = new Map<string, RawSet>();
  const pulled = new Map<string, RawSet>();
  const actionContexts = new Map<string, ActionContext>();
  const pendingEdits = new Map<string, PendingEdit>();
  // Keep only bounded display receipts after discarding obsolete edit payloads.
  const retiredEdits = new Map<string, string>();
  const retireEdit = (key: string, cause: string): void => {
    const saved = pendingEdits.get(key); if (!saved) return;
    const subject = `Fix ${saved.diagnostics.map(diagnosticCode).filter(Boolean).join(", ")} in ${fileLabel(saved.documentUri)}`;
    if (retiredEdits.size >= 128) retiredEdits.delete(retiredEdits.keys().next().value!);
    retiredEdits.set(key, `${subject}: ${cause}`.slice(0, 512)); pendingEdits.delete(key);
  };
  const retireAll = (cause: string): void => { for (const key of pendingEdits.keys()) retireEdit(key, cause); };
  const progressSubscriptions = new Set<vscode.Disposable>();
  const middleware = service.middleware;
  const previousPush = middleware.handleDiagnostics;
  const previousPull = middleware.provideDiagnostics;
  const previousWorkspace = middleware.provideWorkspaceDiagnostics;
  const previousActions = middleware.provideCodeActions;
  let disposed = false, actionSequence = 0, folderGeneration = 0, inputGeneration = 0;
  let instance = service.currentInstance, revisionConfirmedChange = false;
  const status = vscode.window.createStatusBarItem("dygnosis.hiddenDiagnostics", vscode.StatusBarAlignment.Left, 9);
  status.name = "Dygnosis hidden checks";
  status.command = "dygnosis.showDiagnostic";
  const documentFor = (uri: vscode.Uri): vscode.TextDocument | undefined =>
    vscode.workspace.textDocuments.find(document => document.uri.toString() === uri.toString());
  const versionFor = (uri: vscode.Uri): number | null => documentFor(uri)?.version ?? null;
  const visible = (items: vscode.Diagnostic[]): vscode.Diagnostic[] => items.filter(diagnostic => !hiddenCodes.has(diagnosticCode(diagnostic) ?? ""));
  const updateStatus = (): void => {
    if (hiddenCodes.size === 0) { status.hide(); return; }
    status.text = `$(eye-closed) ${hiddenCodes.size} hidden ${hiddenCodes.size === 1 ? "check" : "checks"}`;
    status.tooltip = "Dygnosis: Show a hidden check. Hiding affects this window only.";
    status.show();
  };
  const syncInstance = (): void => {
    if (instance === service.currentInstance) return;
    instance = service.currentInstance;
    pushed.clear(); pulled.clear(); actionContexts.clear(); retireAll("The engine stopped or restarted.");
  };
  const refresh = (): void => {
    syncInstance();
    for (const raw of pushed.values()) {
      // Never replay locations recorded before an edit. The live server will replace them.
      if (raw.version === versionFor(raw.uri)) raw.next?.(raw.uri, visible(raw.items));
    }
    const feature = service.client?.getFeature(DocumentDiagnosticRequest.method);
    const providers = new Set(vscode.workspace.textDocuments.filter(isAnalysisDocument)
      .map(document => feature?.getProvider(document)).filter(provider => provider !== undefined));
    for (const provider of providers) provider.onDidChangeDiagnosticsEmitter.fire();
    updateStatus();
  };
  // Actions and Show replay the current push only. Pull facts stay in a separate cache.
  const currentRaw = (uri: vscode.Uri): RawSet | undefined => pushed.get(uri.toString());
  const acceptFull = (uri: vscode.Uri, items: vscode.Diagnostic[], resultId: string | undefined, version: number | null): void => {
    if (items.length === 0) pulled.delete(uri.toString());
    else pulled.set(uri.toString(), { uri, items, resultId, version });
  };
  const emptyDisplay = (report: vsdiag.DocumentDiagnosticReport, relatedDocuments?: vsdiag.DocumentDiagnosticReport["relatedDocuments"]): vsdiag.DocumentDiagnosticReport => ({
    kind: vsdiag.DocumentDiagnosticReportKind.full, resultId: report.resultId, items: [],
    ...(relatedDocuments ? { relatedDocuments } : {}),
  });
  const fullReport = (uri: vscode.Uri, report: vsdiag.DocumentDiagnosticReport, version: number | null): vsdiag.DocumentDiagnosticReport => {
    if (report.kind === vsdiag.DocumentDiagnosticReportKind.full) acceptFull(uri, report.items, report.resultId, version);
    const relatedDocuments: NonNullable<vsdiag.DocumentDiagnosticReport["relatedDocuments"]> = {};
    for (const [key, related] of Object.entries(report.relatedDocuments ?? {})) {
      relatedDocuments[key] = fullReport(vscode.Uri.parse(key), related, versionFor(vscode.Uri.parse(key)));
    }
    // This editor paints push only. Keep the raw pull cache; return an empty display report.
    // Unchanged keeps that cache and still returns a full empty row so result IDs update.
    return emptyDisplay(report, report.relatedDocuments ? relatedDocuments : undefined);
  };
  const workspaceReport = (report: vsdiag.WorkspaceDiagnosticReport): vsdiag.WorkspaceDiagnosticReport => ({
    items: report.items.map(row => ({ ...row, ...fullReport(row.uri, row, row.version ?? versionFor(row.uri)) })),
  });

  const push: NonNullable<Middleware["handleDiagnostics"]> = (uri, diagnostics, next) => {
    if (disposed) { next(uri, diagnostics); return; }
    syncInstance();
    const retain: PushNext = (target, items) => {
      const key = target.toString();
      for (const [id, saved] of pendingEdits) {
        if (saved.documentUri !== key || saved.diagnostics.length === 0) continue;
        if (!saved.diagnostics.every(diagnostic => items.some(item => sameDiagnostic(item, diagnostic)))) retireEdit(id, "The selected diagnostics changed.");
      }
      pushed.set(key, { uri: target, items, version: versionFor(target), next });
      next(target, visible(items));
    };
    if (previousPush) previousPush(uri, diagnostics, retain); else retain(uri, diagnostics);
  };
  const pull: NonNullable<Middleware["provideDiagnostics"]> = async (document, previousResultId, token, next) => {
    syncInstance();
    const uri = document instanceof vscode.Uri ? document : document.uri;
    const version = versionFor(uri), requestInstance = instance, generation = folderGeneration, input = inputGeneration;
    const report = await (previousPull ? previousPull(document, previousResultId, token, next) : next(document, previousResultId, token));
    if (!report || disposed || token.isCancellationRequested || requestInstance !== service.currentInstance || generation !== folderGeneration || input !== inputGeneration || version !== versionFor(uri)) return undefined;
    return fullReport(uri, report, version);
  };
  const workspacePull: NonNullable<Middleware["provideWorkspaceDiagnostics"]> = async (resultIds, token, reporter, next) => {
    syncInstance();
    const requestInstance = instance, generation = folderGeneration, input = inputGeneration;
    const versions = new Map(vscode.workspace.textDocuments.map(document => [document.uri.toString(), document.version]));
    const current = (): boolean => !disposed && !token.isCancellationRequested && requestInstance === service.currentInstance && generation === folderGeneration && input === inputGeneration;
    const currentRow = (row: vsdiag.WorkspaceDocumentDiagnosticReport): boolean =>
      (row.version ?? versions.get(row.uri.toString()) ?? null) === versionFor(row.uri);
    const filterChunk: vsdiag.ResultReporter = chunk => {
      if (!current()) return;
      reporter(chunk ? workspaceReport({ items: chunk.items.filter(currentRow) }) : null);
    };
    // v9's next closes over the original reporter. Use public request/converter APIs
    // to filter every partial chunk, rather than allowing that closure to bypass us.
    const client = service.client;
    const request: typeof next = client ? async (ids, cancellation, deliver) => {
      const convert = async (row: WorkspaceDocumentDiagnosticReport): Promise<vsdiag.WorkspaceDocumentDiagnosticReport> => {
        const uri = client.protocol2CodeConverter.asUri(row.uri);
        return row.kind === "full" ? { ...row, uri, kind: vsdiag.DocumentDiagnosticReportKind.full,
          items: await client.protocol2CodeConverter.asDiagnostics(row.items, cancellation) } :
          { ...row, uri, kind: vsdiag.DocumentDiagnosticReportKind.unChanged };
      };
      const partialResultToken = randomUUID();
      let chunks = Promise.resolve();
      let conversionError: unknown;
      const progress = client.onProgress(WorkspaceDiagnosticRequest.partialResult, partialResultToken, chunk => {
        chunks = chunks.then(async () => { deliver({ items: await Promise.all(chunk.items.map(convert)) }); })
          .catch((error: unknown) => { conversionError = error; });
      });
      progressSubscriptions.add(progress);
      try {
        const capability = client.initializeResult?.capabilities.diagnosticProvider;
        const report = await client.sendRequest(WorkspaceDiagnosticRequest.type, {
          identifier: capability && "identifier" in capability ? capability.identifier : undefined,
          previousResultIds: ids.map(row => ({ uri: client.code2ProtocolConverter.asUri(row.uri), value: row.value })),
          partialResultToken,
        }, cancellation);
        await chunks;
        if (conversionError) throw conversionError instanceof Error ? conversionError : new Error("Converting requested diagnostics failed.");
        deliver({ items: await Promise.all(report.items.map(convert)) });
        return { items: [] };
      } finally { progress.dispose(); progressSubscriptions.delete(progress); }
    } : next;
    const report = await (previousWorkspace ? previousWorkspace(resultIds, token, filterChunk, request) : request(resultIds, token, filterChunk));
    if (!current() || !report) return undefined;
    // Public v9 workspace consumers update diagnostics/result IDs only via reporter.
    if (report.items.length > 0) filterChunk(report);
    return { items: [] };
  };
  const actions: NonNullable<Middleware["provideCodeActions"]> = async (document, range, context, token, next) => {
    if (!isAnalysisDocument(document)) return [];
    syncInstance();
    const version = document.version, requestInstance = instance, generation = folderGeneration, input = inputGeneration;
    const raw = currentRaw(document.uri);
    const requested = context.diagnostics;
    const engineDiagnostics = requested.length === 0 ? [] : requested.flatMap(supplied => {
      if (!raw || raw.version !== version) return [supplied];
      const exact = raw.items.find(diagnostic => diagnostic === supplied);
      if (exact) return [exact];
      const matches = raw.items.filter(diagnostic => sameDiagnostic(diagnostic, supplied));
      return matches.length > 0 ? matches : [supplied];
    });
    const fullContext = { ...context, diagnostics: [...new Set(engineDiagnostics)] };
    const result = await (previousActions ? previousActions(document, range, fullContext, token, next) : next(document, range, fullContext, token));
    if (disposed || token.isCancellationRequested || version !== document.version || requestInstance !== service.currentInstance || generation !== folderGeneration || input !== inputGeneration) return [];
    const retained = (result ?? []).filter(action => !("diagnostics" in action) || !action.diagnostics?.length || action.diagnostics.some(diagnostic => !hiddenCodes.has(diagnosticCode(diagnostic) ?? "")));
    // The language client builds a WorkspaceEdit without TextDocumentEdit.version.
    // Keep the edit here and apply it only through the guarded command below.
    for (const action of retained) {
      if (!("edit" in action) || !action.edit) continue;
      const edit = action.edit;
      const key = String(++actionSequence);
      const versions = new Map<string, number | null>();
      if (typeof edit.entries === "function") for (const [uri] of edit.entries()) versions.set(uri.toString(), versionFor(uri));
      const diagnostics = "diagnostics" in action && action.diagnostics ? [...action.diagnostics] : [];
      const roots = diagnostics.flatMap(diagnostic => {
        const data = "data" in diagnostic && record(diagnostic.data) ? diagnostic.data : undefined;
        return typeof data?.root === "string" && typeof data.input_revision === "string" ? [{ root: data.root, revision: data.input_revision }] : [];
      });
      pendingEdits.set(key, { edit, versions, instance, folder: folderGeneration, documentUri: document.uri.toString(), documentVersion: version, diagnostics, roots });
      action.edit = undefined;
      action.command = { title: action.title, command: "dygnosis.applyDiagnosticEdit", arguments: [{ diagnosticEdit: key }] };
    }
    if (context.only && !context.only.contains(vscode.CodeActionKind.QuickFix)) return retained;
    const ignore = booleanSetting("diagnosticActions.ignore", document.uri, true, service.log);
    for (const [key, saved] of actionContexts) if (saved.uri.toString() === document.uri.toString()) actionContexts.delete(key);
    const current = raw?.version === version ? raw.items : [];
    const visibleOwner = (diagnostic: vscode.Diagnostic): boolean =>
      diagnostic.source === "dygnosis" && overlapping(diagnostic.range, range) && !hiddenCodes.has(diagnosticCode(diagnostic) ?? "");
    const candidates = requested.length === 0
      ? current.filter(visibleOwner)
      : requested.flatMap(supplied => supplied.source === "dygnosis"
        ? current.filter(diagnostic => visibleOwner(diagnostic) && sameDiagnostic(diagnostic, supplied))
        : []);
    for (const diagnostic of candidates) {
      const code = diagnosticCode(diagnostic);
      if (!code) continue;
      // Match the work-stop sentence used by current and older engines. Other
      // I211 limits cannot be changed with this setting.
      if (code === "I211" && diagnostic.message.startsWith("Macro expansion stopped at the iteration work limit;")) {
        const title = "Configure macro work limit";
        const action = new vscode.CodeAction(title, vscode.CodeActionKind.QuickFix);
        action.diagnostics = [diagnostic];
        action.command = {
          title, command: "workbench.action.openSettings", arguments: ["@id:dynare.macroWorkLimit"],
        };
        retained.push(action);
      }
      const key = String(++actionSequence);
      actionContexts.set(key, { uri: document.uri, version, instance, diagnostic });
      if (ignore) {
        const title = "Ignore this check";
        const action = new vscode.CodeAction(`${title} (${code})`, vscode.CodeActionKind.QuickFix);
        action.diagnostics = [diagnostic];
        action.command = { title, command: "dygnosis.ignoreDiagnostic", arguments: [{ diagnosticAction: key }] };
        retained.push(action);
      }
    }
    return retained;
  };
  middleware.handleDiagnostics = push;
  middleware.provideDiagnostics = pull;
  middleware.provideWorkspaceDiagnostics = workspacePull;
  middleware.provideCodeActions = actions;

  const editCause = (saved: PendingEdit): string | undefined => {
    if (disposed) return "The editor action closed.";
    if (saved.instance !== service.currentInstance) return "The engine stopped or restarted.";
    if (saved.folder !== folderGeneration) return "Workspace folders changed.";
    const document = documentFor(vscode.Uri.parse(saved.documentUri));
    if (!document || document.version !== saved.documentVersion) return `${fileLabel(saved.documentUri)} changed or closed.`;
    for (const [key, offered] of saved.versions) {
      const open = documentFor(vscode.Uri.parse(key));
      if (offered === null) { if (open) return `${fileLabel(key)} opened after the fix was prepared.`; }
      else if (!open || open.version !== offered) return `${fileLabel(key)} changed or closed.`;
    }
    if (saved.diagnostics.length === 0) return undefined;
    const raw = currentRaw(document.uri);
    if (!raw || raw.version !== document.version) return "Current diagnostics are unavailable.";
    if (!saved.diagnostics.every(diagnostic => !hiddenCodes.has(diagnosticCode(diagnostic) ?? "") && raw.items.some(item => sameDiagnostic(item, diagnostic)))) return "The selected diagnostics changed or were hidden.";
    return undefined;
  };
  const savedAction = (argument: unknown): ActionContext | undefined => {
    if (typeof argument !== "object" || argument === null || !("diagnosticAction" in argument) || typeof argument.diagnosticAction !== "string") return undefined;
    const saved = actionContexts.get(argument.diagnosticAction);
    return saved && saved.instance === service.currentInstance && saved.version === versionFor(saved.uri) ? saved : undefined;
  };
  const registrations = [status,
    vscode.commands.registerCommand("dygnosis.applyDiagnosticEdit", async (argument: unknown): Promise<boolean> => (await runCommand(service, async () => {
      syncInstance();
      const key = typeof argument === "object" && argument !== null && "diagnosticEdit" in argument && typeof argument.diagnosticEdit === "string" ? argument.diagnosticEdit : undefined;
      const saved = key ? pendingEdits.get(key) : undefined;
      if (key) pendingEdits.delete(key);
      if (!saved) {
        const cause = key && retiredEdits.get(key); if (key) retiredEdits.delete(key);
        refuse(`${cause || "The diagnostic fix is no longer current."} No edits were applied. Request it from the current Problems list.`);
      }
      const cause = editCause(saved);
      if (cause) refuse(`Fix ${saved.diagnostics.map(diagnosticCode).filter(Boolean).join(", ")} in ${fileLabel(saved.documentUri)} stopped: ${cause} No edits were applied. Request the fix again.`);
      const applied = await vscode.workspace.applyEdit(saved.edit) === true;
      if (!applied) refuse("VS Code did not apply the diagnostic fix. Check that its source is writable and request the fix again.");
      return true;
    }, () => !disposed)) ?? false),
    vscode.commands.registerCommand("dygnosis.ignoreDiagnostic", (argument: unknown) => runCommand(service, () => {
      const saved = savedAction(argument), code = saved ? diagnosticCode(saved.diagnostic) : undefined;
      if (!code || !saved) refuse("The selected diagnostic expired. Request the action from the current Problems list.");
      if (!booleanSetting("diagnosticActions.ignore", saved.uri, true, service.log)) refuse("Ignore check is disabled by dynare.diagnosticActions.ignore.");
      hiddenCodes.add(code);
      for (const [id, pending] of pendingEdits) {
        if (pending.diagnostics.some(diagnostic => diagnosticCode(diagnostic) === code)) retireEdit(id, `Check ${code} was hidden.`);
      }
      refresh();
    }, () => !disposed)),
    vscode.commands.registerCommand("dygnosis.showDiagnostic", async () => {
      const code = await vscode.window.showQuickPick([...hiddenCodes].sort().map(code => ({ label: code, description: "Show this check in this window", code })), { title: "Dygnosis: Show a hidden check", placeHolder: "Choose a check to restore" });
      if (!code || disposed) return;
      hiddenCodes.delete(code.code); refresh();
    }),
    vscode.commands.registerCommand("dygnosis.showAllDiagnostics", () => {
      if (disposed) return;
      hiddenCodes.clear(); refresh();
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      ++folderGeneration;
      pushed.clear(); pulled.clear();
      hiddenCodes.clear(); actionContexts.clear(); retireAll("Workspace folders changed."); refresh();
    }),
    vscode.workspace.onDidCloseTextDocument(document => {
      const key = document.uri.toString();
      for (const [id, saved] of actionContexts) if (saved.uri.toString() === key) actionContexts.delete(id);
      for (const [id, saved] of pendingEdits) if (saved.documentUri === key || saved.versions.has(key)) retireEdit(id, `${fileLabel(key)} closed.`);
      pushed.delete(document.uri.toString()); pulled.delete(document.uri.toString());
    }),
    service.onDidInvalidate(event => {
      // invalidate() fires changed immediately afterwards. An owner change fires
      // changed alone, so only that following changed may keep proved edits.
      revisionConfirmedChange = true;
      queueMicrotask(() => { revisionConfirmedChange = false; });
      if (event.reason === "file" || event.inputRevision === undefined) retireAll(event.reason === "lifecycle" ? "The engine stopped or restarted." : event.uri ? `${fileLabel(event.uri)} changed.` : "Model inputs changed.");
      else for (const [key, saved] of pendingEdits) {
        const contradicted = saved.roots.some(row => row.root === event.root && row.revision !== event.inputRevision);
        if (saved.roots.length === 0 || contradicted) retireEdit(key, "The model input revision changed.");
      }
      // Input changes can leave the document version unchanged. A server
      // notification follows its push; preserve facts from that same revision.
      if (!event.root) pushed.clear();
      else for (const [key, raw] of pushed) {
        const items = raw.items.filter(diagnostic => {
          const data = "data" in diagnostic ? diagnostic.data : undefined;
          // Older engines may omit provenance. Their current push still
          // survives its following notification; local input changes clear it.
          if (!event.inputRevision || !record(data) || typeof data.root !== "string" || typeof data.input_revision !== "string") return true;
          return data.root !== event.root || data.input_revision === event.inputRevision;
        });
        if (items.length !== raw.items.length) pushed.set(key, { ...raw, items });
      }
      pulled.clear();
    }),
    service.onDidChange(() => {
      ++inputGeneration; actionContexts.clear(); syncInstance();
      const followedInvalidation = revisionConfirmedChange;
      revisionConfirmedChange = false;
      if (!followedInvalidation) retireAll("The selected model owner or context changed.");
    }),
  ];
  return vscode.Disposable.from(...registrations, new vscode.Disposable(() => {
    disposed = true; hiddenCodes.clear(); pushed.clear(); pulled.clear(); actionContexts.clear(); pendingEdits.clear(); retiredEdits.clear();
    for (const subscription of progressSubscriptions) subscription.dispose();
    progressSubscriptions.clear();
    if (middleware.handleDiagnostics === push) middleware.handleDiagnostics = previousPush;
    if (middleware.provideDiagnostics === pull) middleware.provideDiagnostics = previousPull;
    if (middleware.provideWorkspaceDiagnostics === workspacePull) middleware.provideWorkspaceDiagnostics = previousWorkspace;
    if (middleware.provideCodeActions === actions) middleware.provideCodeActions = previousActions;
  }));
}
