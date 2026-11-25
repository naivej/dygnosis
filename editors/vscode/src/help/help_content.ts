import type { MarkedToken, Token } from "marked" with { "resolution-mode": "import" };

// eslint-disable-next-line @typescript-eslint/no-require-imports -- Static require lets the package bundle include this ESM dependency.
const { Lexer } = require("marked") as { Lexer: { lex(markdown: string): Token[] } };
export interface HelpTopic { id: string; title: string; keywords: string[]; markdown: string }
export interface HelpCode { code: string; title: string; kind: string; markdown: string }
export interface HelpTool { name: string; description: string; inputSchema: unknown }
export interface HelpBundle {
  version: string; topics: HelpTopic[];
  reference: { version: string; codes: HelpCode[]; tools: HelpTool[]; options: { command: string; options: { name: string; description: string }[] }[] };
  commands: { command: string; title: string; category: string }[];
  keybindings: { command: string; key: string; mac?: string }[];
  settingsTopics: Record<string, string>;
  commandsTopics: Record<string, string>;
}
export interface PreparedHelpBundle {
  version: string;
  topics: (Omit<HelpTopic, "markdown"> & { html: string; body: string; text: string })[];
  settingsTopics: HelpBundle["settingsTopics"];
  assets: string[];
}

export function prepareHelpBundle(bundle: HelpBundle): PreparedHelpBundle {
  const assets = new Set<string>();
  const topics = helpPages(bundle).map(({ id, title, keywords, markdown }) => {
    const html = helpMarkdown(markdown, file => {
      assets.add(file);
      return `dygnosis-help-asset:${file}`;
    });
    return { id, title, keywords, html, body: markdown.toLowerCase(), text: helpSearchText(html) };
  });
  return { version: bundle.version, topics, settingsTopics: bundle.settingsTopics, assets: [...assets].sort() };
}

export function resolveHelpAssets(
  { topics, assets }: PreparedHelpBundle,
  imageUri: (file: string) => string,
): PreparedHelpBundle["topics"] {
  if (!assets.length) return topics;
  const uris = new Map(assets.map(file => [`src="dygnosis-help-asset:${file}"`, `src="${escapeHtml(imageUri(file))}"`]));
  return topics.map(topic => ({
    ...topic,
    html: topic.html.replace(/src="dygnosis-help-asset:assets\/[a-zA-Z0-9/_-]+\.(?:png|jpg|svg)"/g, attribute => uris.get(attribute) ?? attribute),
  }));
}
const htmlEntities = { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" };
const textEntities = new Map(Object.entries(htmlEntities).map(([char, entity]) => [entity, char]));
export const escapeHtml = (value: string): string => value.replace(/[&<>"']/g, char => htmlEntities[char as keyof typeof htmlEntities]);
export const headingId = (value: string): string => value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

/** Read the renderer's escaped text without exposing link targets or controls. */
export function helpSearchText(html: string): string {
  return html
    .replace(/<button\b[^>]*>[\s\S]*?<\/button>/g, "")
    .replace(/<\/(?:p|li|td|th|pre|h[1-6])>/g, " ")
    .replace(/<[^>]*>/g, "")
    .replace(/&(amp|lt|gt|quot|#39);/g, entity => textEntities.get(entity) ?? entity)
    .replace(/\s+/g, " ").trim();
}

/** Strip internal engine-warrant sections from user-facing diagnostic explanations. */
export const userDiagnosticMarkdown = (markdown: string): string =>
  markdown.replace(/\n\n\*\*(?:Range w|W)arrant\*\*\n\n[\s\S]*?(?=\n\n\*\*|$)/g, "");

/** Reuse the agent topic's task summaries; wire contracts stay in MCP metadata. */
function userToolPages(tools: HelpTool[], agents: HelpTopic | undefined): HelpTopic[] {
  const summaries = new Map<string, string>();
  for (const value of Lexer.lex(agents?.markdown ?? "")) {
    const token = value as MarkedToken;
    if (token.type !== "table") continue;
    for (const row of token.rows) {
      const link = row[0]?.tokens.find(value => {
        const token = value as MarkedToken;
        return token.type === "link" && /^tool:dynare_\w+$/.test(token.href);
      }) as MarkedToken | undefined;
      if (link?.type === "link" && row[1]) summaries.set(link.href.slice(5), row[1].text);
    }
  }
  return tools.flatMap(tool => {
    const summary = summaries.get(tool.name);
    return summary ? [{ id: `tool:${tool.name}`, title: tool.name, keywords: [tool.name, "MCP"], markdown: `# ${tool.name}\n\n${summary}\n\n[Work with an agent](help:agents)` }] : [];
  });
}

/** Render tokens, never raw HTML. Links become messages, never command URIs. */
export function helpMarkdown(markdown: string, imageUri: (file: string) => string | undefined): string {
  const render = (tokens: Token[]): string => tokens.map(value => {
    const token = value as MarkedToken;
    switch (token.type) {
      case "html": case "def": case "checkbox": return "";
      case "space": return "\n";
      case "text": return token.tokens ? render(token.tokens) : escapeHtml(token.text);
      case "escape": case "codespan": return token.type === "codespan" ? `<code>${escapeHtml(token.text)}</code>` : escapeHtml(token.text);
      case "paragraph": return `<p>${render(token.tokens)}</p>`;
      case "heading": return `<h${token.depth} id="${headingId(token.text)}">${render(token.tokens)}</h${token.depth}>`;
      case "strong": case "em": case "del": return `<${token.type}>${render(token.tokens)}</${token.type}>`;
      case "br": return " ";
      case "hr": return "<hr>";
      case "code": return `<div class="example"><button type="button" data-copy>Copy example</button><pre><code>${escapeHtml(token.text)}</code></pre></div>`;
      case "link": {
        const target = token.href.replace(/^([a-z][a-z-]+)\.md(?=#|$)/, "help:$1");
        const allowed = /^(help:[a-z-]+(?:#[\w-]+)?|check:[A-Z]\d{3}|tool:dynare_\w+|option:\w+|settings:dynare\.(?:[\w.]+|MATLAB\/OctaveTint)|action:(output|restart|settings|shortcuts)|https?:\/\/[^\s<>]+|#[\w-]+)$/.test(target);
        return allowed ? `<a href="#" data-link="${escapeHtml(target)}">${render(token.tokens)}</a>` : render(token.tokens);
      }
      case "image": {
        const uri = /^assets\/[a-zA-Z0-9/_-]+\.(png|jpg|svg)$/.test(token.href) ? imageUri(token.href) : undefined;
        return uri ? `<button class="image" type="button" aria-label="Enlarge: ${escapeHtml(token.text)}"><img src="${escapeHtml(uri)}" alt="${escapeHtml(token.text)}"></button>` : escapeHtml(token.text);
      }
      case "blockquote": return `<blockquote>${render(token.tokens)}</blockquote>`;
      case "list": return `<${token.ordered ? "ol" : "ul"}>${token.items.map(item => `<li>${render(item.tokens)}</li>`).join("")}</${token.ordered ? "ol" : "ul"}>`;
      case "list_item": return render(token.tokens);
      case "table": {
        const row = (cells: { tokens: Token[] }[], tag: string): string => `<tr>${cells.map(cell => `<${tag}>${render(cell.tokens)}</${tag}>`).join("")}</tr>`;
        return `<div class="table"><table><thead>${row(token.header, "th")}</thead><tbody>${token.rows.map(cells => row(cells, "td")).join("")}</tbody></table></div>`;
      }
    }
  }).join("");
  return render(Lexer.lex(markdown));
}

export function helpPages(bundle: HelpBundle): HelpTopic[] {
  const pages = bundle.topics.map(topic => ({ ...topic, keywords: [...topic.keywords] }));
  const about = pages.find(topic => topic.id === "about");
  if (about) about.markdown = `${about.markdown.trimEnd()}\n\nHelp edition ${bundle.version}\n`;
  const reference = pages.find(topic => topic.id === "reference");
  // A bracket in a check title must stay inside the link. Raw "]" ends it early.
  const codeIndexLink = (code: HelpCode): string => `- [${code.code} — ${code.title.replaceAll(/[[\]]/g, bracket => `\\${bracket}`)}](check:${code.code}) (${code.kind})`;
  const commandLink = (command: { command: string; title: string; category: string }): string => {
    const scope = command.command === "dygnosis.ignoreWorkspaceFolder" ? " (folder row action in Dynare Workspace)" : "";
    return `- [${command.category}: ${command.title}](help:${bundle.commandsTopics[command.command]})${scope}`;
  };
  if (reference) {
    const shortcuts = bundle.keybindings.map(key => {
      const title = bundle.commands.find(command => command.command === key.command)?.title ?? key.command;
      return `- ${title}: ${key.key}; macOS ${key.mac ?? key.key}`;
    });
    const diagnosticKinds = [
      "| Label | Meaning |", "|---|---|",
      "| Shared | Dynare reports this on the file you edit, and Dygnosis reports it too. |",
      "| Added | Extra guidance from Dygnosis. Dynare never reports it. |",
      "| Skipped | Documented only. Dynare's message depends on rewritten equations or on the MATLAB file it generates, so Dygnosis does not show the check. |",
    ];
    const sections = [
      { title: "Commands", content: bundle.commands.map(commandLink).join("\n") },
      { title: "Dygnosis shortcuts", content: `Open [Keyboard Shortcuts](action:shortcuts) to customize Dygnosis keybindings.\n\n${shortcuts.join("\n")}` },
      { title: "Diagnostic codes", content: `${diagnosticKinds.join("\n")}\n\n${bundle.reference.codes.map(codeIndexLink).join("\n")}` },
      { title: "Command options", content: bundle.reference.options.map(entry => `- [${entry.command}](option:${entry.command})`).join("\n") },
    ];
    const links = sections.map(({ title }) => `[${title}](#${headingId(title)})`).join(" · ");
    reference.markdown = `${reference.markdown.trimEnd()}\n\n${links}\n\n${sections.map(({ title, content }) => `## ${title}\n\n${content}`).join("\n\n")}\n`;
  }
  pages.push(...bundle.reference.options.map(entry => ({ id: `option:${entry.command}`, title: `${entry.command} options`, keywords: [entry.command, "options"], markdown: `# ${entry.command} options\n\n[Editing](help:edit-models) · [Command options](help:reference#command-options)\n\n| Option | Meaning |\n|---|---|\n${entry.options.map(option => `| \`${option.name}\` | ${option.description.replaceAll("|", "\\|").replaceAll("\n", " ")} |`).join("\n")}` })));
  pages.push(...bundle.reference.codes.map(code => ({ id: `check:${code.code}`, title: `${code.code}: ${code.title}`, keywords: [code.code, code.kind], markdown: `${userDiagnosticMarkdown(code.markdown)}\n\n[Use diagnostic actions](help:diagnostics) · [Diagnostic code index](help:reference#diagnostic-codes)` })));
  pages.push(...userToolPages(bundle.reference.tools, pages.find(topic => topic.id === "agents")));
  for (const command of bundle.commands) pages.find(topic => topic.id === bundle.commandsTopics[command.command])?.keywords.push(command.command, `${command.category}: ${command.title}`);
  for (const [key, id] of Object.entries(bundle.settingsTopics)) pages.find(topic => topic.id === id)?.keywords.push(key);
  return pages;
}
