// Runs ok.wasm (the WebAssembly build of ok) for the playground on the
// landing page. It runs in a worker so that a script that never ends cannot
// freeze the page: playground.js terminates the worker after a time limit.
//
// Message in:  { warm, wasmUrl } to start loading early, then
//              { wasmUrl, args, env, files }  where files maps a path such as
//              "config/base.yaml" to its bytes (Uint8Array).
// Message out: { code, out, files }  where out is a list of [stream, text]
//              chunks in the order ok wrote them ("out" or "err"), or
//              { crash } when the WebAssembly engine itself stopped.
import {
  WASI,
  File,
  OpenFile,
  ConsoleStdout,
  PreopenDirectory,
  Directory,
} from "https://cdn.jsdelivr.net/npm/@bjorn3/browser_wasi_shim@0.4.2/dist/index.js";

let compiled = null;

async function compile(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`cannot load ${url}: HTTP ${response.status}`);
  return WebAssembly.compile(await response.arrayBuffer());
}

// Build the in-memory directory tree the script sees as its working directory.
function toTree(files) {
  const root = new Map();
  for (const [path, bytes] of Object.entries(files)) {
    const parts = path.split("/");
    let dir = root;
    for (const part of parts.slice(0, -1)) {
      if (!dir.has(part)) dir.set(part, new Directory(new Map()));
      dir = dir.get(part).contents;
    }
    dir.set(parts[parts.length - 1], new File(bytes));
  }
  return root;
}

// Flatten the tree back into path -> bytes after the run, so files the script
// wrote show up in the page.
function fromTree(dir, prefix = "", out = {}) {
  for (const [name, node] of dir) {
    if (node instanceof Directory) fromTree(node.contents, `${prefix}${name}/`, out);
    else if (node instanceof File) out[prefix + name] = node.data;
  }
  return out;
}

self.onmessage = async ({ data }) => {
  const { wasmUrl, args, env, files } = data;
  // { warm } starts the download and compile before the first run.
  if (data.warm) {
    compiled ??= compile(wasmUrl);
    compiled.catch(() => (compiled = null));
    return;
  }
  const out = [];
  const decoders = { out: new TextDecoder(), err: new TextDecoder() };
  const sink = (stream) => (bytes) => {
    const text = decoders[stream].decode(bytes, { stream: true });
    if (text) out.push([stream, text]);
  };
  try {
    compiled ??= compile(wasmUrl);
    const module = await compiled;
    const cwd = new PreopenDirectory(".", toTree(files));
    const fds = [new OpenFile(new File([])), new ConsoleStdout(sink("out")), new ConsoleStdout(sink("err")), cwd];
    const wasi = new WASI(["ok", ...args], Object.entries(env).map(([k, v]) => `${k}=${v}`), fds, { debug: false });
    const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });
    let code;
    try {
      code = wasi.start(instance);
    } catch (e) {
      // A stack overflow surfaces as the engine's RangeError, a Rust panic as
      // a trap after the panic message was written to stderr.
      const crash = e instanceof RangeError
        ? "the script recursed deeper than the browser allows (the native ok goes much deeper)"
        : `ok stopped: ${e.message || e}`;
      self.postMessage({ crash, out });
      return;
    }
    self.postMessage({ code, out, files: fromTree(cwd.dir.contents) });
  } catch (e) {
    compiled = null;
    self.postMessage({ crash: String(e.message || e), out });
  }
};
