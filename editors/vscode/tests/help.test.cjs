const { compiledModule } = require("./helpers/extension_modules.cjs");
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const { helpMarkdown, helpPages } = require(compiledModule("help_content"));
const bundle = require('../help/bundle.json');

test('Help uses every current command, check, option and setting feature owner without mutating its source', () => {
  const original = JSON.stringify(bundle);
  const pages = helpPages(bundle), ids = new Set(pages.map(page => page.id));
  assert.equal(ids.size, pages.length);
  for (const code of bundle.reference.codes) assert.ok(ids.has(`check:${code.code}`));
  for (const tool of bundle.reference.tools) assert.ok(ids.has(`tool:${tool.name}`));
  for (const command of bundle.commands) {
    const owner = pages.find(page => page.id === bundle.commandsTopics[command.command]);
    assert.ok(owner, command.command);
    assert.ok(owner.keywords.includes(command.command), command.command);
    assert.ok(owner.keywords.includes(`${command.category}: ${command.title}`), command.title);
  }
  for (const [key, id] of Object.entries(bundle.settingsTopics)) assert.ok(pages.find(page => page.id === id).keywords.includes(key));
  assert.ok(!ids.has('settings'));
  assert.equal(JSON.stringify(bundle), original);
  const referencePage = pages.find(page => page.id === 'reference');
  assert.equal(referencePage.markdown.includes('## MCP tools'), false);
  assert.equal(referencePage.markdown.includes('[dynare_diagnose](tool:dynare_diagnose)'), false);
  assert.ok(pages.find(page => page.id === 'agents').markdown.includes('[dynare_diagnose](tool:dynare_diagnose)'));
  const referenceHtml = helpMarkdown(referencePage.markdown, () => undefined);
  for (const code of bundle.reference.codes) assert.match(referenceHtml, new RegExp(`data-link="check:${code.code}"`), code.code);
  assert.match(referenceHtml, /planner_discount outside \(0, 1\]/);
});

test('untrusted engine explanations cannot execute HTML, links or remote images', () => {
  for (const target of ['command:evil', 'command%3Aevil', 'javascript:evil', 'data:text/html,x', 'vscode:evil']) {
    const html = helpMarkdown(`[Run](${target})\n\n<img src=x onerror=evil()>\n<script>evil()</script>`, () => 'local-image');
    assert.doesNotMatch(html, /<script|<img|onerror|data-link=/);
  }
  assert.equal(helpMarkdown('![External](https://example.org/x.png)', () => 'local-image'), '<p>External</p>');
  assert.doesNotMatch(helpMarkdown('![Escape](assets/../secret.png)', () => 'local-image'), /<img/);
  const html = helpMarkdown('```mod\nvar y; <script>\n```\n\n[Help](help:diagnostics)\n\n[Settings](settings:dynare.serverPath)', () => undefined);
  assert.match(html, /&lt;script&gt;/); assert.match(html, /data-copy/);
  assert.match(html, /data-link="help:diagnostics"/); assert.match(html, /data-link="settings:dynare.serverPath"/);
  assert.match(helpMarkdown('[Feature](appearance.md#expression-value-hints)', () => undefined), /data-link="help:appearance#expression-value-hints"/);
});

test('tool Help uses the authored task summary without exposing MCP contracts', () => {
  // Guards contract or schema changes leaking into human Help, and keeps the
  // authored summary as the single owner for the overview and tool page.
  const changed = JSON.parse(JSON.stringify(bundle));
  changed.reference.tools.find(tool => tool.name === 'dynare_model_info').description = 'Internal MCP contract marker.';
  changed.reference.tools.find(tool => tool.name === 'dynare_model_info').inputSchema = { properties: { internal_contract_field: {} } };
  const agents = changed.topics.find(topic => topic.id === 'agents');
  agents.markdown = agents.markdown.replace(/(\[dynare_model_info\]\(tool:dynare_model_info\) \| )[^\n]+/, '$1Inspect the model for this task. |');
  const page = helpPages(changed).find(page => page.id === 'tool:dynare_model_info');
  assert.match(page.markdown, /Inspect the model for this task\./);
  assert.doesNotMatch(page.markdown, /Internal MCP contract marker|internal_contract_field|## Parameters|## Input schema/);
  assert.equal(changed.reference.tools.find(tool => tool.name === 'dynare_model_info').description, 'Internal MCP contract marker.');
  assert.match(helpMarkdown(page.markdown, () => undefined), /data-link="help:agents"/);
});

test('Help About and the extension listing use the product README without its logo', () => {
  const product = fs.readFileSync(path.join(__dirname, '../../../README.md'), 'utf8').replace(/!\[[^\]]*\]\([^)]*logo[^)]*\)\s*/g, '');
  assert.equal(bundle.topics.find(topic => topic.id === 'about').markdown, product);
  assert.equal(fs.readFileSync(path.join(__dirname, '../README.md'), 'utf8'), product);
  assert.doesNotMatch(product, /!\[[^\]]*\]\(/);
});

test('every authored topic has balanced code examples and valid internal destinations', () => {
  const pages = helpPages(bundle), ids = new Set(pages.map(page => page.id));
  for (const topic of pages) {
    for (const [, kind, target] of topic.markdown.matchAll(/\]\((help:|check:|tool:|option:)([^)]+)\)/g)) {
      assert.ok(ids.has(kind === 'help:' ? target.split('#')[0] : kind + target), target);
    }
    for (const [, id] of topic.markdown.matchAll(/\]\(([a-z][a-z-]+)\.md(?:#[\w-]+)?\)/g)) assert.ok(ids.has(id), id);
    assert.doesNotThrow(() => helpMarkdown(topic.markdown, () => undefined));
  }
});
