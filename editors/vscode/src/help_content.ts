import { createRequire } from "node:module";
import type { MarkedToken, Token } from "marked" with { "resolution-mode": "import" };

const { Lexer } = createRequire(__filename)("marked") as { Lexer: { lex(markdown: string): Token[] } };
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
export const escapeHtml = (value: string): string => value.replace(/[&<>"']/g, char => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[char]!));
export const headingId = (value: string): string => value.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

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
      case "br": return "<br>";
      case "hr": return "<hr>";
      case "code": return `<div class="example"><button type="button" data-copy>Copy example</button><pre><code>${escapeHtml(token.text)}</code></pre></div>`;
      case "link": {
        const target = token.href.replace(/^([a-z][a-z-]+)\.md(?=#|$)/, "help:$1");
        const allowed = /^(help:[a-z-]+(?:#[\w-]+)?|check:[A-Z]\d{3}|tool:dynare_\w+|option:\w+|settings:dynare\.[\w.]+|action:(output|restart|shortcuts)|https?:\/\/[^\s<>]+|#[\w-]+)$/.test(target);
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
  const reference = pages.find(topic => topic.id === "reference");
  if (reference) reference.markdown += `\n## Commands\n\n| Command | Action |\n|---|---|\n${bundle.commands.map(command => `| \`${command.command}\` | [${command.category}: ${command.title}](help:${bundle.commandsTopics[command.command]}) |`).join("\n")}\n\n## Preview shortcuts\n\n${bundle.keybindings.map(key => `- ${key.command}: ${key.key}; macOS ${key.mac ?? key.key}`).join("\n")}\n\n## Diagnostic codes\n\n${bundle.reference.codes.map(code => `- [${code.code} — ${code.title}](check:${code.code}) (${code.kind})`).join("\n")}\n\n## Command options\n\n${bundle.reference.options.map(entry => `- [${entry.command}](option:${entry.command})`).join("\n")}\n\n## MCP tools\n\n${bundle.reference.tools.map(tool => `- [${tool.name}](tool:${tool.name})`).join("\n")}\n`;
  pages.push(...bundle.reference.options.map(entry => ({ id: `option:${entry.command}`, title: `${entry.command} options`, keywords: [entry.command, "options"], markdown: `# ${entry.command} options\n\n[Edit a model](help:edit-models) · [Command index](help:reference)\n\n| Option | Meaning |\n|---|---|\n${entry.options.map(option => `| \`${option.name}\` | ${option.description.replaceAll("|", "\\|").replaceAll("\n", " ")} |`).join("\n")}` })));
  pages.push(...bundle.reference.codes.map(code => ({ id: `check:${code.code}`, title: `${code.code}: ${code.title}`, keywords: [code.code, code.kind], markdown: `${code.markdown}\n\n[Use diagnostic actions](help:diagnostics) · [Diagnostic code index](help:reference#diagnostic-codes)` })));
  pages.push(...bundle.reference.tools.map(tool => ({ id: `tool:${tool.name}`, title: tool.name, keywords: [tool.name, "MCP"], markdown: `# ${tool.name}\n\n${tool.description}\n\n[Connect an agent](help:agents)\n\n## Input schema\n\n\`\`\`json\n${JSON.stringify(tool.inputSchema, null, 2)}\n\`\`\`` })));
  for (const [key, id] of Object.entries(bundle.settingsTopics)) pages.find(topic => topic.id === id)?.keywords.push(key);
  return pages;
}
