// Inspect the rendered Changes webview in an isolated native VS Code test host.
const assert = require("node:assert/strict");
const { clearTimeout } = require("node:timers");

async function readHostJson(route) {
  const port = process.env.DYGNOSIS_HOST_CDP_PORT;
  assert.ok(port, "Required native visual checks need the isolated loopback CDP port");
  const response = await globalThis.fetch(`http://127.0.0.1:${port}/${route}`, { signal: globalThis.AbortSignal.timeout(10000) });
  assert.ok(response.ok, `Native CDP discovery failed: ${route} (${response.status})`);
  return response.json();
}

async function nativeWorkbench(callback) {
  const targets = await readHostJson("json/list");
  const workbench = targets.find(target => target.type === "page" && target.url.includes("workbench"));
  assert.ok(workbench?.webSocketDebuggerUrl, "The isolated native host must expose its workbench");
  const browser = await readHostJson("json/version");
  assert.ok(browser.webSocketDebuggerUrl, "The isolated native host must expose its browser target");
  const socket = new globalThis.WebSocket(browser.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    const finish = error => {
      clearTimeout(timer);
      socket.removeEventListener("open", opened);
      socket.removeEventListener("error", failed);
      socket.removeEventListener("close", closed);
      if (error) { socket.close(); reject(error); }
      else resolve();
    };
    const opened = () => finish();
    const failed = () => finish(new Error("Native CDP WebSocket failed to open"));
    const closed = () => finish(new Error("Native CDP WebSocket closed before opening"));
    const timer = setTimeout(() => finish(new Error("Native CDP timed out opening WebSocket")), 10000);
    socket.addEventListener("open", opened);
    socket.addEventListener("error", failed);
    socket.addEventListener("close", closed);
  });
  let next = 0;
  let workbenchSession;
  const pending = new Map();
  const listeners = new Set();
  socket.addEventListener("message", event => {
    const response = JSON.parse(event.data);
    if (response.method) {
      for (const listener of listeners) listener(response);
      return;
    }
    const entry = pending.get(response.id);
    if (!entry) return;
    pending.delete(response.id); clearTimeout(entry.timer);
    if (response.error) entry.reject(new Error(JSON.stringify(response.error)));
    else entry.resolve(response.result);
  });
  const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = ++next;
    const timer = setTimeout(() => {
      pending.delete(id); reject(new Error(`Native CDP timed out: ${method}`));
    }, 10000);
    pending.set(id, { resolve, reject, timer });
    const targetSession = sessionId ?? (method.startsWith("Target.") ? undefined : workbenchSession);
    socket.send(JSON.stringify({ id, method, params, ...(targetSession ? { sessionId: targetSession } : {}) }));
  });
  send.onEvent = listener => { listeners.add(listener); return () => listeners.delete(listener); };
  try {
    workbenchSession = (await send("Target.attachToTarget", { targetId: workbench.id, flatten: true })).sessionId;
    return await callback(send);
  } finally {
    socket.close();
    listeners.clear();
    for (const entry of pending.values()) {
      clearTimeout(entry.timer); entry.reject(new Error("Native CDP connection closed"));
    }
  }
}

async function nativeChanges(callback) {
  return nativeWorkbench(async send => {
    const contexts = new Map(), sessions = new Map();
    send.onEvent(event => {
      const sessionId = event.sessionId;
      if (event.method === "Runtime.executionContextCreated") {
        const context = event.params.context;
        // Runtime reports same-process nested iframe worlds that the outer
        // target's frame tree can omit. The page world has the actual DOM.
        if (context.auxData?.isDefault) contexts.set(`${sessionId ?? "page"}:${context.id}`, { sessionId, contextId: context.id });
      } else if (event.method === "Runtime.executionContextDestroyed") {
        contexts.delete(`${sessionId ?? "page"}:${event.params.executionContextId}`);
      } else if (event.method === "Runtime.executionContextsCleared") {
        for (const [key, context] of contexts) if (context.sessionId === sessionId) contexts.delete(key);
      }
    });
    const findContext = async () => {
      // Electron's Target.getTargets omits some visible webview targets. The
      // isolated endpoint lists them; Runtime supplies their nested page worlds.
      const inventory = await readHostJson("json/list");
      const allFrames = inventory.filter(target => target.type === "iframe");
      assert.ok(allFrames.length <= 32, "Native Changes discovery is limited to 32 iframe targets");
      const visible = await send("Runtime.evaluate", {
        expression: '[...document.querySelectorAll("iframe")].filter(frame=>{const r=frame.getBoundingClientRect(),s=getComputedStyle(frame);return r.width>0&&r.height>0&&s.visibility!=="hidden"&&s.display!=="none"}).map(frame=>frame.src)', returnByValue: true,
      });
      const frames = allFrames.filter(target => visible.result?.value?.includes(target.url));
      const present = new Set(frames.map(target => target.id));
      for (const [targetId, sessionId] of sessions) {
        if (present.has(targetId)) continue;
        sessions.delete(targetId);
        for (const [key, context] of contexts) if (context.sessionId === sessionId) contexts.delete(key);
      }
      for (const target of frames) {
        if (!sessions.has(target.id)) {
          const attached = await send("Target.attachToTarget", { targetId: target.id, flatten: true });
          sessions.set(target.id, attached.sessionId);
        }
      }
      for (const sessionId of [undefined, ...sessions.values()]) await send("Runtime.enable", {}, sessionId);
      assert.ok(contexts.size <= 256, "Native Changes discovery is limited to 256 page worlds");
      for (const context of [...contexts.values()].reverse()) {
        let result;
        try {
          result = await send("Runtime.evaluate", {
            contextId: context.contextId,
            expression: 'Boolean(document.getElementById("changeComparison"))', returnByValue: true,
          }, context.sessionId);
        } catch { continue; } // A hidden/reloaded webview can destroy a world.
        if (result.result?.value) return context;
      }
      const documents = [];
      for (const context of contexts.values()) {
        try {
          const result = await send("Runtime.evaluate", {
            contextId: context.contextId,
            expression: '({url:location.href,title:document.title,frames:document.querySelectorAll("iframe").length,body:document.body?.textContent.slice(0,350)})', returnByValue: true,
          }, context.sessionId);
          documents.push({ ...context, value: result.result?.value });
        } catch { /* The failure inventory can omit a world closed during capture. */ }
      }
      const error = new Error("Required native visual checks must reach the rendered Changes control frame");
      error.code = "NATIVE_FRAME_PENDING";
      error.discovery = { frames: allFrames.map(frame => ({ id: frame.id, url: frame.url })), visible: visible.result?.value, documents };
      throw error;
    };
    let context = await findContext();
    const evaluate = async expression => {
      const result = await send("Runtime.evaluate", {
        contextId: context.contextId, expression, returnByValue: true, awaitPromise: true,
      }, context.sessionId);
      assert.equal(result.exceptionDetails, undefined, `Native Changes evaluation failed: ${expression}`);
      return result.result?.value;
    };
    return await callback({ send, evaluate, refreshContext: async () => { context = await findContext(); } });
  });
}

module.exports = { nativeWorkbench, nativeChanges };
