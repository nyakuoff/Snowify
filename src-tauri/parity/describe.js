// Canonical description of a JS value, used on both sides of the transport harness so that
// "what the handler received" and "what the renderer got back" compare as plain JSON.
(function (root) {
  function describe(v, seen = new Map()) {
    if (v === undefined) return { t: 'undefined' };
    if (v === null) return { t: 'null' };
    const ty = typeof v;
    if (ty === 'number') return { t: 'number', v: Object.is(v, -0) ? '-0' : String(v) };
    if (ty === 'string') return { t: 'string', v: [...v].map((c) => c.codePointAt(0)) };
    if (ty === 'boolean') return { t: 'boolean', v };
    if (ty === 'bigint') return { t: 'bigint', v: String(v) };
    if (ty === 'symbol') return { t: 'symbol' };
    if (ty === 'function') return { t: 'function' };
    if (seen.has(v)) return { t: 'ref', id: seen.get(v) };
    seen.set(v, seen.size);
    const proto = Object.getPrototypeOf(v);
    const d = {
      t: 'object',
      tag: Object.prototype.toString.call(v),
      ctor: proto === null ? null : (proto.constructor && proto.constructor.name) || '',
    };
    const props = (o, skip) => Reflect.ownKeys(o)
      .filter((k) => typeof k === 'string' && !skip(k))
      .map((k) => {
        const desc = Object.getOwnPropertyDescriptor(o, k);
        return [k, desc.enumerable, 'value' in desc ? describe(desc.value, seen) : { t: 'accessor' }];
      });
    if (v instanceof Date) d.time = String(v.getTime());
    else if (v instanceof RegExp) { d.source = v.source; d.flags = v.flags; }
    else if (v instanceof Map) d.entries = [...v].map(([k, x]) => [describe(k, seen), describe(x, seen)]);
    else if (v instanceof Set) d.values = [...v].map((x) => describe(x, seen));
    else if (v instanceof ArrayBuffer) d.bytes = [...new Uint8Array(v)];
    else if (ArrayBuffer.isView(v)) {
      d.bytes = [...new Uint8Array(v.buffer, v.byteOffset, v.byteLength)];
      d.bufferLength = v.buffer.byteLength;
    } else if (v instanceof Error) {
      d.name = v.name;
      d.message = v.message;
      d.hasStack = typeof v.stack === 'string';
    } else if (v instanceof Number || v instanceof String || v instanceof Boolean) d.prim = describe(v.valueOf(), seen);
    if (Array.isArray(v)) {
      d.length = v.length;
      d.items = Array.from({ length: v.length }, (_, i) => (i in v ? describe(v[i], seen) : { t: 'hole' }));
      d.props = props(v, (k) => k === 'length' || String(k >>> 0) === k);
    } else {
      d.props = props(v, (k) => (v instanceof Error && k === 'stack') || (v instanceof String && (k === 'length' || String(k >>> 0) === k)));
    }
    return d;
  }
  // The renderer-visible shape of a rejection.
  function describeError(e) {
    return {
      isError: e instanceof Error,
      name: e && e.name,
      message: e && e.message,
      ownKeys: e && typeof e === 'object' ? Object.keys(e) : null,
    };
  }
  root.describe = describe;
  root.describeError = describeError;
})(typeof module !== 'undefined' ? module.exports : window);
