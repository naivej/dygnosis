/* global acquireVsCodeApi, document, window, requestAnimationFrame */
(() => {
  'use strict';
  const api = acquireVsCodeApi();
  const saved = api.getState();
  const history = Array.isArray(saved?.history) ? saved.history.filter(entry => typeof entry?.destination === 'string').slice(-100).map(entry => ({ destination: entry.destination, scroll: Number.isFinite(entry.scroll) && entry.scroll >= 0 ? entry.scroll : 0, explanation: entry.explanation })) : [];
  const state = { history: history.length ? history : [{ destination: 'get-started', scroll: 0 }], index: Number.isInteger(saved?.index) && saved.index >= 0 && saved.index < history.length ? saved.index : 0, query: typeof saved?.query === 'string' ? saved.query.slice(0, 300) : '' };
  const controls = Object.fromEntries(['edition', 'back', 'forward', 'permalink', 'search', 'clear', 'contents', 'main', 'breadcrumb', 'results', 'article', 'notice', 'enlarged', 'close-image'].map(id => [id, document.getElementById(id)]));
  let topics = [], version = '';
  const node = (tag, text) => { const result = document.createElement(tag); result.textContent = text; return result; };
  const remember = () => api.setState({ ...state, history: state.history.map(({ destination, scroll, explanation }) => ({ destination, scroll, explanation })) });
  const current = () => state.history[state.index];
  function render(focus = false) {
    const destination = current().destination, [id, anchor] = destination.split('#');
    const topic = current().extra ?? topics.find(topic => topic.id === id) ?? topics.find(topic => topic.id === 'reference');
    if (!topic) return;
    controls.edition.textContent = topic.edition ?? `Edition ${version}`;
    controls.breadcrumb.textContent = `Help › ${topic.title}`;
    controls.article.innerHTML = topic.html;
    controls.permalink.disabled = !topics.some(topic => topic.id === id);
    controls.permalink.title = controls.permalink.disabled ? 'This check is not in this Help edition. A topic link is unavailable.' : '';
    controls.back.disabled = state.index === 0; controls.forward.disabled = state.index === state.history.length - 1;
    for (const button of controls.contents.querySelectorAll('button')) { if (button.dataset.topic === id) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current'); }
    search();
    requestAnimationFrame(() => {
      if (anchor && !current().scroll) document.getElementById(anchor)?.scrollIntoView(); else window.scrollTo(0, current().scroll ?? 0);
      if (focus) controls.main.focus({ preventScroll: true });
    });
    remember();
  }
  function open(destination, extra) {
    if (!topics.some(topic => topic.id === destination.split('#')[0]) && extra?.id !== destination.split('#')[0]) return;
    current().scroll = window.scrollY;
    if (current().destination !== destination || JSON.stringify(current().explanation) !== JSON.stringify(extra?.explanation)) { state.history = state.history.slice(0, state.index + 1); state.history.push({ destination, scroll: 0, extra, explanation: extra?.explanation }); state.history = state.history.slice(-100); state.index = state.history.length - 1; }
    state.query = ''; controls.search.value = ''; render(true);
  }
  function search() {
    state.query = controls.search.value.slice(0, 300);
    controls.results.replaceChildren();
    const query = state.query.trim().toLowerCase(); controls.article.hidden = !!query;
    if (query) {
      const words = query.split(/\s+/);
      const found = topics.filter(topic => words.every(word => `${topic.title} ${topic.keywords.join(' ')} ${topic.markdown}`.toLowerCase().includes(word))).slice(0, 50);
      controls.results.append(node('p', found.length ? `${found.length} matching ${found.length === 1 ? 'topic' : 'topics'}` : 'No results. Try a feature name, command, setting key or diagnostic code.'));
      for (const topic of found) {
        const button = node('button', topic.title); button.type = 'button'; button.addEventListener('click', () => open(topic.id));
        const text = topic.markdown.replace(/[#`*[\]]/g, ' ').replace(/\s+/g, ' '), position = text.toLowerCase().indexOf(words[0]);
        button.append(node('small', ` — ${text.slice(Math.max(0, position - 30), Math.max(0, position - 30) + 160)}`)); controls.results.append(button);
      }
    }
    remember();
  }
  controls.search.value = typeof state.query === 'string' ? state.query : '';
  controls.search.addEventListener('input', search);
  controls.clear.addEventListener('click', () => { controls.search.value = ''; search(); controls.search.focus(); });
  controls.permalink.addEventListener('click', () => api.postMessage({ type: 'permalink', destination: current().destination }));
  for (const [id, step] of [['back', -1], ['forward', 1]]) controls[id].addEventListener('click', () => {
    current().scroll = window.scrollY; state.index = Math.min(state.history.length - 1, Math.max(0, state.index + step)); render(true);
  });
  window.addEventListener('scroll', () => { current().scroll = window.scrollY; remember(); }, { passive: true });
  document.addEventListener('click', event => {
    const link = event.target.closest('[data-link]');
    if (link) {
      event.preventDefault(); const target = link.dataset.link;
      if (target.startsWith('help:')) open(target.slice(5));
      else if (target.startsWith('check:')) open(target);
      else if (target.startsWith('tool:') || target.startsWith('option:')) open(target);
      else if (target.startsWith('#')) open(`${current().destination.split('#')[0]}${target}`, current().extra);
      else api.postMessage({ type: 'link', target });
    }
    const copy = event.target.closest('[data-copy]');
    if (copy) api.postMessage({ type: 'copy', text: copy.parentElement.querySelector('code').textContent });
    const image = event.target.closest('.image');
    if (image) { const original = image.querySelector('img'), enlarged = controls.enlarged.querySelector('img'); enlarged.src = original.src; enlarged.alt = original.alt; controls.enlarged.showModal(); }
  });
  controls['close-image'].addEventListener('click', () => controls.enlarged.close());
  window.addEventListener('message', event => {
    const message = event.data;
    if (message.type === 'bundle') {
      topics = message.topics; version = message.version; controls.contents.replaceChildren();
      for (const [index, entry] of state.history.entries()) entry.extra = message.historyExtras?.[index];
      for (const topic of topics.filter(topic => !topic.id.includes(':'))) { const button = node('button', topic.title); button.type = 'button'; button.dataset.topic = topic.id; button.addEventListener('click', () => open(topic.id)); controls.contents.append(button); }
      render();
    } else if (message.type === 'open') open(message.destination, message.extra);
    else if (message.type === 'notice') controls.notice.textContent = message.text;
  });
  api.postMessage({ type: 'ready', history: state.history.map(({ destination, explanation }) => ({ destination, explanation })) });
})();
