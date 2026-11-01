// Renderer half of the transport harness. Runs unchanged against Electron (window.probe from a
// contextBridge preload) and against Tauri (window.probe from the shim's transport).
(async () => {
  const corpus = window.__PROBE_CORPUS_JSON__ || await (await fetch('../../corpus/transport.json')).json();
  const d = Object.getOwnPropertyDescriptor(window, 'probe');
  const results = {
    apiShape: {
      frozen: Object.isFrozen(probe), sealed: Object.isSealed(probe), extensible: Object.isExtensible(probe),
      writable: d.writable, configurable: d.configurable, enumerable: d.enumerable,
      memberDesc: (({ writable, enumerable, configurable }) => ({ writable, enumerable, configurable }))(Object.getOwnPropertyDescriptor(probe, 'echo')),
      fnLength: probe.echo.length, fnName: probe.echo.name, protoIsObject: Object.getPrototypeOf(probe) === Object.prototype,
    },
    invoke: [], arity: [], throws: [], events: [], eventFails: [], rawEventArgs: null, sendSync: [] };
  // Tauri's page CSP (the app's) has no 'unsafe-eval'; there the expressions arrive compiled.
  const compiled = window.__PROBE_VALUES__;
  const evaluate = (expr) => {
    try { return compiled ? compiled[corpus.values.indexOf(expr)]() : eval(expr); } catch { return undefined; }
  };

  for (const [i, expr] of corpus.values.entries()) {
    try {
      results.invoke.push({ ok: describe(await probe.echo(evaluate(expr), i)) });
    } catch (e) {
      results.invoke.push({ err: describeError(e) });
    }
  }
  for (const args of [[], [undefined], [1, undefined], [undefined, undefined, undefined]]) {
    results.arity.push(await probe.echoArity(...args));
  }
  for (let i = 0; i < corpus.throws.length; i++) {
    try { await probe.throwAt(i); results.throws.push({ resolved: true }); } catch (e) { results.throws.push(describeError(e)); }
  }
  for (const expr of corpus.values) {
    try { results.sendSync.push({ ret: describe(probe.send(evaluate(expr))) }); } catch (e) { results.sendSync.push({ err: describeError(e) }); }
  }
  try { await probe.missing(); results.missingHandler = 'resolved'; } catch (e) { results.missingHandler = describeError(e); }
  await new Promise((resolve) => {
    probe.onEv((i, v) => { results.events[i] = describe(v); });
    probe.onEvFail((i, m) => { results.eventFails.push([i, m]); });
    probe.onRaw((...args) => {
      results.rawEventArgs = { count: args.length, keys: Object.keys(args[0] || {}).sort(), senderType: typeof (args[0] && args[0].sender), senderId: args[0] && args[0].senderId, ports: args[0] && Array.isArray(args[0].ports) && args[0].ports.length };
      resolve();
    });
    probe.emitAll();
  });
  results.events = Array.from({ length: corpus.values.length }, (_, i) => results.events[i] || null);
  await probe.done(JSON.stringify(results));
})().catch((e) => probe.done(JSON.stringify({ fatal: String(e && e.stack) })));
