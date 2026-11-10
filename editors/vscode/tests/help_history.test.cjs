const assert = require('node:assert/strict');
const test = require('node:test');
const vm = require('node:vm');
const fs = require('node:fs');
const path = require('node:path');

function reader(saved, isNarrow = false) {
  class Element {
    constructor() { this.children = []; this.handlers = {}; this.dataset = {}; this.value = ''; }
    addEventListener(name, run) { this.handlers[name] = run; }
    replaceChildren() { this.children = []; }
    append(...children) { this.children.push(...children); }
    querySelectorAll() { return this.children; }
    setAttribute() {}
    removeAttribute() {}
    focus() {}
  }
  const controls = new Map(), messages = [], events = {};
  let state;
  const media = { matches: isNarrow, addEventListener: (_name, run) => { media.change = run; } };
  const window = { matchMedia: () => media, scrollY: 0, scrollTo() {}, addEventListener: (name, run) => { events[name] = run; } };
  const document = { getElementById: id => {
    if (!controls.has(id)) controls.set(id, new Element());
    return controls.get(id);
  }, createElement: () => new Element(), addEventListener() {} };
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../media/help.js'), 'utf8'), {
    document, window, requestAnimationFrame: run => run(), acquireVsCodeApi: () => ({
      getState: () => saved, setState: next => { state = JSON.parse(JSON.stringify(next)); }, postMessage: message => messages.push(message),
    }),
  });
  return { controls, media, messages, state: () => state, receive: data => events.message({ data }), back: () => controls.get('back').handlers.click() };
}
const topics = ['get-started', 'reference', 'diagnostics'].map(id => ({ id, title: id, html: id, markdown: id, keywords: [] }));

test('reading history preserves each active-engine explanation and edition across other topics and restore', () => {
  const live = reader(); live.receive({ type: 'bundle', topics, version: '0.11.7' });
  const explanation = { code: 'E999', markdown: '# Engine-specific text', engineVersion: '0.10.0' };
  const extra = { id: 'check:E999', title: 'E999', html: '<h1>Engine-specific text</h1>', edition: 'Active engine 0.10.0; Help 0.11.7', explanation };
  live.receive({ type: 'open', destination: 'check:E999', extra });
  live.receive({ type: 'open', destination: 'diagnostics' }); live.back();
  assert.equal(live.controls.get('article').innerHTML, extra.html);
  assert.equal(live.controls.get('edition').textContent, extra.edition);
  assert.equal(live.controls.get('permalink').disabled, true, 'an unknown active-engine code has no bundled topic link');
  const saved = live.state();
  assert.equal(saved.history[1].extra, undefined, 'persist source Markdown, never rendered HTML');
  const restored = reader(saved), historyExtras = saved.history.map(entry => entry.explanation ? extra : undefined);
  assert.equal(restored.messages[0].history[1].explanation.markdown, explanation.markdown);
  restored.receive({ type: 'bundle', topics, version: '0.11.7', historyExtras });
  assert.equal(restored.controls.get('article').innerHTML, extra.html);
  assert.equal(restored.controls.get('edition').textContent, extra.edition);
});

// Guards against the topic list covering the article after selection or resize.
test('narrow topics close after selection and wide navigation returns on resize', () => {
  const live = reader(undefined, true); live.receive({ type: 'bundle', topics, version: '0.12.0' });
  const disclosure = live.controls.get('topics');
  assert.equal(disclosure.open, false); disclosure.open = true;
  live.controls.get('contents').children[2].handlers.click();
  assert.equal(disclosure.open, false);
  assert.equal(live.controls.get('article').innerHTML, 'diagnostics');
  live.media.matches = false; live.media.change(); assert.equal(disclosure.open, true);
  live.media.matches = true; live.media.change(); assert.equal(disclosure.open, false);
});
