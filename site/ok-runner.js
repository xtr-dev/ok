// Runs ok commands in the browser with the WebAssembly build of ok, for the
// playground on the landing page and the Run buttons in the guide (/docs/).
// The work happens in playground-worker.js; a script that never ends is
// stopped by terminating the worker after TIME_LIMIT_MS.

export const TIME_LIMIT_MS = 10000;

const WASM_URL = new URL("ok.wasm", import.meta.url).href;
const WORKER_URL = new URL("playground-worker.js", import.meta.url);

let worker = null;
let busy = false;

/** Start the worker and the download of ok.wasm, so the first run is quick. */
export function warm() {
  if (!worker) {
    worker = new Worker(WORKER_URL, { type: "module" });
    worker.postMessage({ warm: true, wasmUrl: WASM_URL });
  }
  return worker;
}

function stopWorker() {
  worker?.terminate();
  worker = null;
}

/**
 * Run ok with `args` and `env` in a directory holding `files` (path -> text).
 * Resolves to { code, out, files } where out is a list of [stream, text]
 * chunks and files is the directory after the run (path -> text), or to
 * { crash, out } when ok could not finish (time limit, engine error).
 * One run at a time: a second call while one runs resolves to { busy: true }.
 */
export function runOk({ args, env = {}, files = {} }) {
  if (busy) return Promise.resolve({ busy: true });
  busy = true;
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  return new Promise((resolve) => {
    const w = warm();
    const done = (result) => {
      clearTimeout(timer);
      busy = false;
      resolve(result);
    };
    const timer = setTimeout(() => {
      stopWorker();
      done({ crash: `stopped after ${TIME_LIMIT_MS / 1000} s: runs in the browser have a time limit`, out: [] });
    }, TIME_LIMIT_MS);
    w.onmessage = ({ data }) => {
      if (data.crash) {
        stopWorker();
        done({ crash: data.crash, out: data.out });
        return;
      }
      const after = Object.fromEntries(Object.entries(data.files).map(([p, bytes]) => [p, decoder.decode(bytes)]));
      done({ code: data.code, out: data.out, files: after });
    };
    w.onerror = (e) => {
      stopWorker();
      done({ crash: `could not start ok: ${e.message || "unknown error"}`, out: [] });
    };
    w.postMessage({
      wasmUrl: WASM_URL,
      args,
      env,
      files: Object.fromEntries(Object.entries(files).map(([p, t]) => [p, encoder.encode(t)])),
    });
  });
}

// Split a command line like a shell would for simple cases: whitespace
// separates words, single or double quotes group them. Leading NAME=value
// words set environment variables, as they do in a shell.
export function parseCommand(line) {
  const words = [];
  let word = null;
  let quote = null;
  for (const ch of line) {
    if (quote) {
      if (ch === quote) quote = null;
      else word += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      word ??= "";
    } else if (/\s/.test(ch)) {
      if (word !== null) words.push(word);
      word = null;
    } else {
      word = (word ?? "") + ch;
    }
  }
  if (quote) throw new Error(`unclosed ${quote} in the command`);
  if (word !== null) words.push(word);
  const env = {};
  while (words.length && /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[0])) {
    const w = words.shift();
    const eq = w.indexOf("=");
    env[w.slice(0, eq)] = w.slice(eq + 1);
  }
  if (words[0] !== "ok") throw new Error("only ok commands run here: start the command with `ok`");
  return { env, args: words.slice(1) };
}

// ok colours its error messages with ANSI escapes; turn them into spans.
const ANSI_CLASS = { 1: "b", 31: "red", 32: "green", 33: "yellow", 34: "blue", 35: "magenta", 36: "cyan" };
export function appendAnsi(parent, text, base) {
  let classes = [];
  for (const [i, part] of text.split(/\x1b\[([0-9;]*)m/).entries()) {
    if (i % 2 === 1) {
      const codes = part.split(";").filter(Boolean).map(Number);
      if (codes.length === 0 || codes.includes(0)) classes = [];
      classes.push(...codes.map((c) => ANSI_CLASS[c]).filter(Boolean));
    } else if (part) {
      const span = document.createElement("span");
      span.className = [base, ...classes].join(" ");
      span.textContent = part;
      parent.append(span);
    }
  }
}
