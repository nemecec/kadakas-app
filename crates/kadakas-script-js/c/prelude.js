// The JavaScript side of the Kadakas script module. It runs first in every run, in the sandbox.
// It makes `kadakas`, `console` and a fixed random sequence, and wraps the user script.
"use strict";
(function (global) {
  const host = global.__host;
  delete global.__host;

  const KINDS = [
    [Float64Array, "f64"],
    [Float32Array, "f32"],
    [Int32Array, "i32"],
    [Uint32Array, "u32"],
    [Uint8Array, "u8"],
  ];
  const CTORS = { f64: Float64Array, f32: Float32Array, i32: Int32Array, u32: Uint32Array, u8: Uint8Array };

  function kindOf(v) {
    for (let i = 0; i < KINDS.length; i++) {
      if (v instanceof KINDS[i][0]) return KINDS[i][1];
    }
    return null;
  }

  // Replaces every typed array in `v` by a marker {"$t": kind, i: index, n: stride} and puts
  // its bytes in `bufs`. Arrays that start with a number are not looked into (a typed array
  // is not mixed into a list of numbers). Containers are copied; `v` is not changed.
  function pack(v, bufs) {
    if (v === null || typeof v !== "object") return v;
    const kind = kindOf(v);
    if (kind !== null) {
      const bytes = v.buffer.slice(v.byteOffset, v.byteOffset + v.byteLength);
      const m = { $t: kind, i: bufs.length };
      if (typeof v.stride === "number" && v.stride > 1) m.n = v.stride;
      bufs.push(bytes);
      return m;
    }
    if (Array.isArray(v)) {
      if (v.length > 0 && typeof v[0] === "number") return v;
      const out = new Array(v.length);
      for (let i = 0; i < v.length; i++) out[i] = pack(v[i], bufs);
      return out;
    }
    if (typeof v.toJSON === "function") return v;
    const out = {};
    for (const k of Object.keys(v)) {
      const x = v[k];
      if (typeof x === "function" || x === undefined) continue;
      out[k] = pack(x, bufs);
    }
    return out;
  }

  function unpack(v, bufs) {
    if (v === null || typeof v !== "object") return v;
    if (Array.isArray(v)) {
      if (v.length > 0 && typeof v[0] === "number") return v;
      for (let i = 0; i < v.length; i++) v[i] = unpack(v[i], bufs);
      return v;
    }
    if (typeof v.$t === "string" && typeof v.i === "number") {
      const ctor = CTORS[v.$t];
      const arr = new ctor(bufs[v.i]);
      arr.stride = typeof v.n === "number" ? v.n : 1;
      return arr;
    }
    for (const k of Object.keys(v)) v[k] = unpack(v[k], bufs);
    return v;
  }

  let calls = 0;
  let maxCalls = 1000000;

  function call(method, params, options) {
    if (typeof method !== "string") throw new TypeError("kadakas.call: the method must be a string");
    if (++calls > maxCalls) throw new Error("the script made more than " + maxCalls + " calls");
    const bufs = [];
    const body = params === undefined ? {} : pack(params, bufs);
    if (options && options.typed) {
      if (body === null || typeof body !== "object" || Array.isArray(body)) {
        throw new TypeError("kadakas.call: the parameters must be an object");
      }
      body.$typed = true;
    }
    const r = host.call(method, JSON.stringify(body), bufs);
    const status = r[0];
    const text = r[1];
    if (status !== "ok") {
      let info;
      try {
        info = JSON.parse(text);
      } catch (e) {
        info = { message: text };
      }
      const err = new Error(info.message);
      err.code = info.code;
      err.data = info.data;
      err.method = method;
      throw err;
    }
    const value = JSON.parse(text);
    if (r.length > 2) return unpack(value, r.slice(2));
    return value;
  }

  const kadakas = {
    call,
    // Marks a typed array as a list of points: stride 2 for [x, y], 3 for [x, y, z].
    points(arr, stride) {
      arr.stride = stride === undefined ? 2 : stride;
      return arr;
    },
    // A Float64Array of points from a list of [x, y] (or [x, y, z]).
    pointsFrom(list, stride) {
      const n = stride === undefined ? 2 : stride;
      const arr = new Float64Array(list.length * n);
      for (let i = 0; i < list.length; i++) for (let k = 0; k < n; k++) arr[i * n + k] = list[i][k];
      arr.stride = n;
      return arr;
    },
  };
  Object.freeze(kadakas);
  Object.defineProperty(global, "kadakas", { value: kadakas, enumerable: false });

  function fmt(v) {
    if (typeof v === "string") return v;
    if (v instanceof Error) return v.stack ? v.stack : String(v);
    if (kindOf(v) !== null) return v.constructor.name + "(" + v.length + ")";
    try {
      const s = JSON.stringify(v);
      return s === undefined ? String(v) : s;
    } catch (e) {
      return String(v);
    }
  }
  const log = (...a) => host.log(a.map(fmt).join(" "));
  global.console = { log, info: log, warn: log, error: log, debug: log };

  // A fixed random sequence (sfc32): the same seed gives the same numbers.
  function seeded(seed) {
    let a = 0x9e3779b9 | 0;
    let b = 0x243f6a88 | 0;
    let c = 0xb7e15162 | 0;
    let d = seed | 0;
    const next = () => {
      a >>>= 0; b >>>= 0; c >>>= 0; d >>>= 0;
      let t = (a + b) | 0;
      a = b ^ (b >>> 9);
      b = (c + (c << 3)) | 0;
      c = (c << 21) | (c >>> 11);
      d = (d + 1) | 0;
      t = (t + d) | 0;
      c = (c + t) | 0;
      return (t >>> 0) / 4294967296;
    };
    for (let i = 0; i < 12; i++) next();
    return next;
  }

  global.__run = function (code, argsJson, options) {
    const opts = options || {};
    if (typeof opts.max_calls === "number") maxCalls = opts.max_calls;
    Math.random = seeded(typeof opts.seed === "number" ? opts.seed : 0);
    try {
      const args = JSON.parse(argsJson);
      const fn = (0, eval)('(function (args) { "use strict"; ' + code + "\n})");
      const value = fn(args);
      const bufs = [];
      const body = pack(value === undefined ? null : value, bufs);
      const text = JSON.stringify(body === undefined ? null : body);
      return ["ok", text === undefined ? "null" : text, String(calls)].concat(bufs);
    } catch (e) {
      let info;
      if (e instanceof Error) {
        info = { message: String(e.message), name: e.name, stack: String(e.stack || "") };
        if (e.method !== undefined) info.method = e.method;
        if (e.code !== undefined) info.code = e.code;
        if (e.data !== undefined) info.data = e.data;
      } else {
        info = { message: "the script threw " + fmt(e) };
      }
      let text;
      try {
        text = JSON.stringify(info);
      } catch (e2) {
        text = JSON.stringify({ message: info.message });
      }
      return ["error", text, String(calls)];
    }
  };
})(globalThis);
