// The IPC value codec shared by shim.js and the transport harness: Electron's contextBridge +
// structured-clone semantics on the way out (`encode`), and their inverse (`decode`).
// Wire format: see src/ipc.rs.
const cloneError = () => new Error('An object could not be cloned.');
const tagOf = (v) => Object.prototype.toString.call(v).slice(8, -1);
const isWellFormed = (s) => !/[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/.test(s);
const units = (s) => Array.from({ length: s.length }, (_, i) => s.charCodeAt(i));
const encStr = (s) => (isWellFormed(s) ? s : { $: 's', v: units(s) });

// One pass over the value (getters run once, as contextBridge does), then shared nodes get ids.
function encode(value) {
  const nodes = new Map();
  function conv(v) {
    if (v === undefined) return { $: 'u' };
    if (v === null || typeof v === 'boolean') return v;
    if (typeof v === 'number') {
      if (Number.isNaN(v)) return { $: 'n', v: 'NaN' };
      if (v === Infinity) return { $: 'n', v: 'Infinity' };
      if (v === -Infinity) return { $: 'n', v: '-Infinity' };
      if (Object.is(v, -0)) return { $: 'n', v: '-0' };
      return v;
    }
    if (typeof v === 'bigint') return { $: 'b', v: String(v) };
    if (typeof v === 'string') return encStr(v);
    if (typeof v !== 'object' || v instanceof Promise) throw cloneError();
    let node = nodes.get(v);
    if (node) { node.refs++; return node; }
    node = { refs: 1, id: -1, out: null };
    nodes.set(v, node);
    const tag = tagOf(v);
    if (Array.isArray(v)) node.out = Array.from({ length: v.length }, (_, i) => conv(v[i]));
    else if (tag === 'Date') { const t = v.getTime(); node.out = { $: 'd', v: Number.isNaN(t) ? 'NaN' : t }; }
    else if (tag === 'RegExp') node.out = { $: 're', s: encStr(v.source), f: v.flags };
    else if (tag === 'Map') node.out = { $: 'm', v: [...v].map(([k, x]) => [conv(k), conv(x)]) };
    else if (tag === 'Set') node.out = { $: 'set', v: [...v].map(conv) };
    else if (tag === 'ArrayBuffer') node.out = { $: 'ab', v: [...new Uint8Array(v)] };
    else if (ArrayBuffer.isView(v)) node.out = { $: 'view', k: tag, b: [...new Uint8Array(v.buffer)], o: v.byteOffset, l: v.byteLength };
    else if (v instanceof Error) node.out = { $: 'err', m: encStr(String(v.message)), st: typeof v.stack === 'string' ? encStr(v.stack) : null };
    else if (tag === 'Number' || tag === 'String' || tag === 'Boolean' || tag === 'BigInt') node.out = { $: 'box', v: conv(v.valueOf()) };
    else node.out = { $: 'o', v: Object.keys(v).map((k) => [encStr(k), conv(v[k])]) };
    return node;
  }
  const root = conv(value);
  let next = 0;
  function emit(x) {
    if (x === null || typeof x !== 'object' || !('refs' in x)) return x;
    if (x.id >= 0) return { $: 'r', i: x.id };
    if (x.refs > 1) { x.id = next++; return { $: 'id', i: x.id, v: body(x.out) }; }
    return body(x.out);
  }
  function body(out) {
    if (Array.isArray(out)) return out.map(emit);
    if (out.$ === 'o' || out.$ === 'm') return { ...out, v: out.v.map(([k, x]) => [emit(k), emit(x)]) };
    if (out.$ === 'set') return { ...out, v: out.v.map(emit) };
    if (out.$ === 'box') return { ...out, v: emit(out.v) };
    return out;
  }
  return emit(root);
}

function decode(wire) {
  const ids = new Map();
  const str = (x) => (typeof x === 'string' ? x : String.fromCharCode(...x.v));
  function dec(x) {
    if (x === null || typeof x !== 'object') return x;
    if (Array.isArray(x)) return x.map(dec);
    switch (x.$) {
      case 'u': return undefined;
      case 'n': return Number(x.v);
      case 'b': return BigInt(x.v);
      case 's': return str(x);
      case 'r': return ids.get(x.i);
      case 'id': return shell(x.v, (o) => ids.set(x.i, o));
      default: return shell(x, () => {});
    }
  }
  // Containers are registered before their children are decoded, so cycles resolve.
  function shell(x, register) {
    if (Array.isArray(x)) { const a = []; register(a); x.forEach((i) => a.push(dec(i))); return a; }
    if (x === null || typeof x !== 'object' || !('$' in x) || ['u', 'n', 'b', 's', 'r'].includes(x.$)) { const v = dec(x); register(v); return v; }
    switch (x.$) {
      case 'o': { const o = {}; register(o); x.v.forEach(([k, v]) => { o[str(k)] = dec(v); }); return o; }
      case 'd': { const d = new Date(x.v === 'NaN' ? NaN : x.v); register(d); return d; }
      case 're': { const r = new RegExp(str(x.s), x.f); register(r); return r; }
      case 'm': { const m = new Map(); register(m); x.v.forEach(([k, v]) => m.set(dec(k), dec(v))); return m; }
      case 'set': { const s = new Set(); register(s); x.v.forEach((v) => s.add(dec(v))); return s; }
      case 'ab': { const b = new Uint8Array(x.v).buffer; register(b); return b; }
      case 'view': {
        const buf = new Uint8Array(x.b).buffer;
        const C = window[x.k];
        const v = x.k === 'DataView' ? new DataView(buf, x.o, x.l) : new C(buf, x.o, x.l / C.BYTES_PER_ELEMENT);
        register(v); return v;
      }
      case 'err': {
        const e = new Error(str(x.m));
        // JavaScriptCore adds own line/column/sourceURL to every Error; V8's clone has none.
        for (const k of ['line', 'column', 'sourceURL']) delete e[k];
        if (x.st !== null) Object.defineProperty(e, 'stack', { value: str(x.st), writable: true, configurable: true });
        register(e); return e;
      }
      case 'box': { const b = Object(dec(x.v)); register(b); return b; }
      default: { const v = dec(x); register(v); return v; }
    }
  }
  return dec(wire);
}
