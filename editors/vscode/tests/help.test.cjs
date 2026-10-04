const assert = require('node:assert/strict');
const test = require('node:test');
const { helpMarkdown, helpPages } = require('../out/help_content');
const bundle = require('../help/bundle.json');

test('Help uses every current command, check, option and setting feature owner without mutating its source', () => {
  const original = JSON.stringify(bundle);
  const pages = helpPages(bundle), ids = new Set(pages.map(page => page.id));
  assert.equal(ids.size, pages.length);
  for (const code of bundle.reference.codes) assert.ok(ids.has(`check:${code.code}`));
  for (const tool of bundle.reference.tools) assert.ok(ids.has(`tool:${tool.name}`));
  for (const command of bundle.commands) assert.ok(ids.has(bundle.commandsTopics[command.command]));
  for (const [key, id] of Object.entries(bundle.settingsTopics)) assert.ok(pages.find(page => page.id === id).keywords.includes(key));
  assert.ok(!ids.has('settings'));
  assert.equal(JSON.stringify(bundle), original);
  assert.ok(pages.find(page => page.id === 'reference').markdown.includes('[dynare_diagnose](tool:dynare_diagnose)'));
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
