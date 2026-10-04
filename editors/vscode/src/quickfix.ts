import * as vscode from "vscode";
import { randomUUID } from "node:crypto";
import { createRequire } from "node:module";
import type { MarkedToken, Token } from "marked" with { "resolution-mode": "import" };
import { DocumentDiagnosticRequest, Middleware, WorkspaceDiagnosticRequest, WorkspaceDocumentDiagnosticReport, vsdiag } from "vscode-languageclient/node";
import { DygnosisClient, isAnalysisDocument } from "./client";
import { record } from "./protocol";
import { booleanSetting } from "./settings";

// marked and entities publish ESM only. This file emits CommonJS, so load the values with require.
const nodeRequire = createRequire(__filename);
const { Lexer } = nodeRequire("marked") as { Lexer: { lex(markdown: string): Token[] } };
const entities = nodeRequire("entities") as { decodeHTML(html: string): string };

type PushNext = NonNullable<Middleware["handleDiagnostics"]> extends
  (uri: vscode.Uri, diagnostics: vscode.Diagnostic[], next: infer Next) => void ? Next : never;
interface RawSet {
  uri: vscode.Uri; items: vscode.Diagnostic[]; version: number | null;
  resultId?: string; next?: PushNext;
}
interface ActionContext {
  uri: vscode.Uri; version: number; instance: number; diagnostic: vscode.Diagnostic;
}

/** Keep the diagnostic object intact: the language client's subclass carries LSP data. */
export function diagnosticCode(diagnostic: vscode.Diagnostic): string | undefined {
  const code = diagnostic.code;
  const value = typeof code === "object" ? code.value : code;
  return typeof value === "number" || (typeof value === "string" && value.length > 0) ? String(value) : undefined;
}
function sameDiagnostic(left: vscode.Diagnostic, right: vscode.Diagnostic): boolean {
  const supplied = "data" in right ? right.data : undefined;
  const retained = "data" in left ? left.data : undefined;
  if (record(supplied) && ["root", "input_revision"].some(key =>
    supplied[key] !== undefined && (!record(retained) || supplied[key] !== retained[key]))) return false;
  return left === right || (left.source === right.source && left.message === right.message &&
    left.severity === right.severity && diagnosticCode(left) === diagnosticCode(right) &&
    left.range.isEqual(right.range));
}
function overlapping(left: vscode.Range, right: vscode.Range): boolean {
  return left.start.isBeforeOrEqual(right.end) && right.start.isBeforeOrEqual(left.end);
}

/** Native Markdown previews are untrusted; never pass executable links or HTML through. */
export function safeExplanation(markdown: string): string {
  const safeTarget = (target: string): boolean => {
    let decoded = entities.decodeHTML(target.trim());
    try { decoded = decodeURIComponent(decoded); } catch { /* Keep malformed escapes literal. */ }
    decoded = decoded.replace(/\s/g, "");
    return !decoded.includes(":") || /^(?:https?:\/\/|mailto:)/i.test(decoded);
  };
  const plain = (text: string): string => text.replace(/[\\`*_[\]<>!#+-]/g, "\\$&");
  // Prevent a second Markdown parse from decoding entities or escapes in a href.
  const destination = (href: string): string => `<${href.replace(/[\\<>\s]/g, character => encodeURIComponent(character))}>`;
  const serialize = (tokens: Token[]): string => tokens.map(value => {
    // A default lexer has only built-in tokens; no extension tokens are enabled.
    const token = value as MarkedToken;
    switch (token.type) {
      case "html": case "def": return "";
      case "space": case "hr": case "code": case "codespan": case "escape": case "br": return token.raw;
      case "text": return token.tokens ? serialize(token.tokens) : plain(token.text);
      case "paragraph": return `${serialize(token.tokens)}\n\n`;
      case "heading": return `${"#".repeat(token.depth)} ${serialize(token.tokens)}\n\n`;
      case "strong": return `**${serialize(token.tokens)}**`;
      case "em": return `*${serialize(token.tokens)}*`;
      case "del": return `~~${serialize(token.tokens)}~~`;
      case "link": case "image": {
        if (!safeTarget(token.href)) return plain(token.text);
        const label = token.type === "image" ? plain(token.text) : serialize(token.tokens);
        const title = token.title ? ` "${token.title.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"` : "";
        return `${token.type === "image" ? "!" : ""}[${label}](${destination(token.href)}${title})`;
      }
      case "blockquote": return `${serialize(token.tokens).trimEnd().split("\n").map(line => `> ${line}`).join("\n")}\n\n`;
      case "list": return `${token.items.map((item, index) => {
        const prefix = token.ordered ? `${Number(token.start) + index}. ` : "- ";
        const content = serialize(item.tokens).trimEnd();
        return `${prefix}${item.task ? `[${item.checked ? "x" : " "}] ` : ""}${content.split("\n").join(`\n${" ".repeat(prefix.length)}`)}`;
      }).join(token.loose ? "\n\n" : "\n")}\n\n`;
      case "list_item": return serialize(token.tokens);
      case "checkbox": return "";
      case "table": {
        const row = (cells: { tokens: Token[] }[]): string => `| ${cells.map(cell => serialize(cell.tokens).replace(/\|/g, "\\|")).join(" | ")} |\n`;
        return `${row(token.header)}| ${token.align.map(align => align === "center" ? ":---:" : align === "right" ? "---:" : align === "left" ? ":---" : "---").join(" | ")} |\n${token.rows.map(row).join("")}\n`;
      }
    }
  }).join("");
  return serialize(Lexer.lex(markdown));
}

/** Register before startup so both LSP diagnostic transports share the window's filter. */
export function registerDiagnosticActions(service: DygnosisClient): vscode.Disposable {
  const hiddenCodes = new Set<string>();
  const pushed = new Map<string, RawSet>();
  const pulled = new Map<string, RawSet>();
  const actionContexts = new Map<string, ActionContext>();
  const progressSubscriptions = new Set<vscode.Disposable>();
  const middleware = service.middleware;
  const previousPush = middleware.handleDiagnostics;
  const previousPull = middleware.provideDiagnostics;
  const previousWorkspace = middleware.provideWorkspaceDiagnostics;
  const previousActions = middleware.provideCodeActions;
  let disposed = false, actionSequence = 0, folderGeneration = 0, inputGeneration = 0;
  let instance = service.currentInstance;
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
    pushed.clear(); pulled.clear(); actionContexts.clear();
  };
  const refresh = (): void => {
    syncInstance();
    for (const raw of pushed.values()) {
      // Never replay locations recorded before an edit. The live server will replace them.
      const latest = currentRaw(raw.uri) ?? raw;
      if (latest.version === versionFor(raw.uri)) raw.next?.(raw.uri, visible(latest.items));
    }
    const feature = service.client?.getFeature(DocumentDiagnosticRequest.method);
    const providers = new Set(vscode.workspace.textDocuments.filter(isAnalysisDocument)
      .map(document => feature?.getProvider(document)).filter(provider => provider !== undefined));
    for (const provider of providers) provider.onDidChangeDiagnosticsEmitter.fire();
    updateStatus();
  };
  const currentRaw = (uri: vscode.Uri): RawSet | undefined => pulled.get(uri.toString()) ?? pushed.get(uri.toString());
  const acceptFull = (uri: vscode.Uri, items: vscode.Diagnostic[], resultId: string | undefined, version: number | null): void => {
    pulled.set(uri.toString(), { uri, items, resultId, version });
  };
  const fullReport = (uri: vscode.Uri, report: vsdiag.DocumentDiagnosticReport, version: number | null): vsdiag.DocumentDiagnosticReport => {
    if (report.kind === vsdiag.DocumentDiagnosticReportKind.full) acceptFull(uri, report.items, report.resultId, version);
    const raw = pulled.get(uri.toString());
    const items = report.kind === vsdiag.DocumentDiagnosticReportKind.full ? report.items : raw?.version === version ? raw.items : undefined;
    const relatedDocuments: NonNullable<vsdiag.DocumentDiagnosticReport["relatedDocuments"]> = {};
    for (const [key, related] of Object.entries(report.relatedDocuments ?? {})) {
      relatedDocuments[key] = fullReport(vscode.Uri.parse(key), related, versionFor(vscode.Uri.parse(key)));
    }
    // A fresh full result also clears VS Code's cached filtered set after Show.
    if (items) return { kind: vsdiag.DocumentDiagnosticReportKind.full, resultId: report.resultId, items: visible(items),
      ...(report.relatedDocuments ? { relatedDocuments } : {}) };
    return { ...report, ...(report.relatedDocuments ? { relatedDocuments } : {}) };
  };
  const workspaceReport = (report: vsdiag.WorkspaceDiagnosticReport): vsdiag.WorkspaceDiagnosticReport => ({
    items: report.items.map(row => ({ ...row, ...fullReport(row.uri, row, row.version ?? versionFor(row.uri)) })),
  });

  const push: NonNullable<Middleware["handleDiagnostics"]> = (uri, diagnostics, next) => {
    if (disposed) { next(uri, diagnostics); return; }
    syncInstance();
    const retain: PushNext = (target, items) => {
      pushed.set(target.toString(), { uri: target, items, version: versionFor(target), next });
      pulled.delete(target.toString());
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
    const rawContext = context.diagnostics.flatMap(supplied => {
      const exact = raw?.items.find(diagnostic => diagnostic === supplied);
      return exact ? [exact] : raw?.items.filter(diagnostic => sameDiagnostic(diagnostic, supplied)) ?? [supplied];
    });
    const fullContext = { ...context, diagnostics: [...new Set(rawContext)] };
    const result = await (previousActions ? previousActions(document, range, fullContext, token, next) : next(document, range, fullContext, token));
    if (disposed || token.isCancellationRequested || version !== document.version || requestInstance !== service.currentInstance || generation !== folderGeneration || input !== inputGeneration) return [];
    const retained = (result ?? []).filter(action => !("diagnostics" in action) || !action.diagnostics?.length || action.diagnostics.some(diagnostic => !hiddenCodes.has(diagnosticCode(diagnostic) ?? "")));
    if (context.only && !context.only.contains(vscode.CodeActionKind.QuickFix)) return retained;
    const ignore = booleanSetting("diagnosticActions.ignore", document.uri, true, service.log);
    const explain = booleanSetting("diagnosticActions.explain", document.uri, true, service.log);
    if (!ignore && !explain) return retained;
    for (const [key, saved] of actionContexts) if (saved.uri.toString() === document.uri.toString()) actionContexts.delete(key);
    const candidates = (raw?.version === version ? raw.items : fullContext.diagnostics)
      .filter(diagnostic => diagnostic.source === "dygnosis" && overlapping(diagnostic.range, range) && !hiddenCodes.has(diagnosticCode(diagnostic) ?? ""));
    for (const diagnostic of candidates) {
      const code = diagnosticCode(diagnostic);
      if (!code) continue;
      const key = String(++actionSequence);
      actionContexts.set(key, { uri: document.uri, version, instance, diagnostic });
      for (const [enabled, title, command] of [
        [ignore, "Ignore this check", "dygnosis.ignoreDiagnostic"],
        [explain, "Explain this check", "dygnosis.explainDiagnostic"],
      ] as const) {
        if (!enabled) continue;
        const action = new vscode.CodeAction(`${title} (${code})`, vscode.CodeActionKind.QuickFix);
        action.diagnostics = [diagnostic];
        action.command = { title, command, arguments: [{ diagnosticAction: key }] };
        retained.push(action);
      }
    }
    return retained;
  };
  middleware.handleDiagnostics = push;
  middleware.provideDiagnostics = pull;
  middleware.provideWorkspaceDiagnostics = workspacePull;
  middleware.provideCodeActions = actions;

  const savedAction = (argument: unknown): ActionContext | undefined => {
    if (typeof argument !== "object" || argument === null || !("diagnosticAction" in argument) || typeof argument.diagnosticAction !== "string") return undefined;
    const saved = actionContexts.get(argument.diagnosticAction);
    return saved && saved.instance === service.currentInstance && saved.version === versionFor(saved.uri) ? saved : undefined;
  };
  const registrations = [status,
    vscode.commands.registerCommand("dygnosis.ignoreDiagnostic", (argument: unknown) => {
      const saved = savedAction(argument), code = saved ? diagnosticCode(saved.diagnostic) : undefined;
      if (!code || !saved || disposed || !booleanSetting("diagnosticActions.ignore", saved.uri, true, service.log)) return;
      hiddenCodes.add(code); refresh();
    }),
    vscode.commands.registerCommand("dygnosis.showDiagnostic", async () => {
      const code = await vscode.window.showQuickPick([...hiddenCodes].sort().map(code => ({ label: code, description: "Show this check in this window", code })), { title: "Dygnosis: Show a hidden check", placeHolder: "Choose a check to restore" });
      if (!code || disposed) return;
      hiddenCodes.delete(code.code); refresh();
    }),
    vscode.commands.registerCommand("dygnosis.showAllDiagnostics", () => {
      if (disposed) return;
      hiddenCodes.clear(); refresh();
    }),
    vscode.commands.registerCommand("dygnosis.explainDiagnostic", async (argument: unknown) => {
      const startFolder = folderGeneration, startInstance = service.currentInstance, startInput = inputGeneration;
      const saved = savedAction(argument);
      if (argument !== undefined && !saved) return;
      if (saved && !booleanSetting("diagnosticActions.explain", saved.uri, true, service.log)) return;
      // Palette invocation can start the first client. Establish that instance before
      // capturing the preview guard, while rejecting a superseded existing client.
      await service.ensureStarted();
      if (disposed || startFolder !== folderGeneration || (saved && !savedAction(argument)) ||
        (startInstance > 0 && (startInstance !== service.currentInstance || startInput !== inputGeneration))) return;
      const generation = folderGeneration, requestInstance = service.currentInstance, input = inputGeneration;
      const current = (): boolean => !disposed && generation === folderGeneration && requestInstance === service.currentInstance &&
        input === inputGeneration && (!saved || !!savedAction(argument));
      const code = saved ? diagnosticCode(saved.diagnostic) : (await vscode.window.showInputBox({ title: "Dygnosis: Explain a check", prompt: "Enter a diagnostic code, for example W010" }))?.trim();
      if (!code || !current()) return;
      try {
        const markdown = await service.execute("dynare/explainDiagnostic", [code]);
        if (!current()) return;
        if (typeof markdown !== "string") throw new Error("The selected engine returned no diagnostic explanation. Use the bundled binary or update dynare.serverPath.");
        await vscode.commands.executeCommand("dygnosis.openHelp", { code, markdown: safeExplanation(markdown), engineVersion: service.client?.initializeResult?.serverInfo?.version });
      } catch (error) { if (current()) await service.failure(String(error)); }
    }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => {
      ++folderGeneration;
      hiddenCodes.clear(); actionContexts.clear(); refresh();
    }),
    vscode.workspace.onDidCloseTextDocument(document => {
      for (const [key, saved] of actionContexts) if (saved.uri.toString() === document.uri.toString()) actionContexts.delete(key);
      pushed.delete(document.uri.toString()); pulled.delete(document.uri.toString());
    }),
    service.onDidChange(() => { ++inputGeneration; actionContexts.clear(); syncInstance(); }),
  ];
  return vscode.Disposable.from(...registrations, new vscode.Disposable(() => {
    disposed = true; hiddenCodes.clear(); pushed.clear(); pulled.clear(); actionContexts.clear();
    for (const subscription of progressSubscriptions) subscription.dispose();
    progressSubscriptions.clear();
    if (middleware.handleDiagnostics === push) middleware.handleDiagnostics = previousPush;
    if (middleware.provideDiagnostics === pull) middleware.provideDiagnostics = previousPull;
    if (middleware.provideWorkspaceDiagnostics === workspacePull) middleware.provideWorkspaceDiagnostics = previousWorkspace;
    if (middleware.provideCodeActions === actions) middleware.provideCodeActions = previousActions;
  }));
}
