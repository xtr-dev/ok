// The playground on the landing page: edit an example, run it with the
// WebAssembly build of ok (in playground-worker.js) and show its output.
//
// Each example is a script from examples/ in the repository plus the data
// files it reads; the Pages workflow copies those folders next to this file,
// so the playground always shows the same files as the repository.

const EXAMPLES = [
  {
    title: "Summarise a log",
    dir: "examples/log-summary",
    files: ["log-summary.ok", "app.log"],
    command: "ok log-summary.ok app.log --top 3",
  },
  {
    title: "CSV to JSON",
    dir: "examples/csv-to-json",
    files: ["csv-to-json.ok", "users.csv"],
    command: "ok csv-to-json.ok users.csv --verbose",
  },
  {
    title: "Merge YAML config",
    dir: "examples/config-render",
    files: ["render-config.ok", "config/base.yaml", "config/production.yaml", "config/dev.yaml"],
    command: "DB_PASSWORD=example ok render-config.ok production",
  },
  {
    title: "A helpful error",
    inline: {
      "prices.ok": [
        "# ok points at the mistake and says how to fix it.",
        "# Correct the typo and press Run again.",
        "let prices = [12.5, 8, 30]",
        "let total = prices | sum",
        'print("total: {totl}")',
        "",
      ].join("\n"),
    },
    command: "ok prices.ok",
  },
];

const TIME_LIMIT_MS = 10000;

const root = document.getElementById("playground");
const picker = root.querySelector(".pg-examples");
const tabs = root.querySelector(".pg-files");
const editor = root.querySelector(".pg-editor");
const commandInput = root.querySelector(".pg-command input");
const runButton = root.querySelector(".pg-run");
const resetButton = root.querySelector(".pg-reset");
const output = root.querySelector(".pg-output");
const status = root.querySelector(".pg-status");

const encoder = new TextEncoder();
const decoder = new TextDecoder();

let files = {};           // path -> text, what the next run sees
let created = new Set();  // paths the last run wrote that the example did not have
let current = null;       // path shown in the editor
let example = null;
let worker = null;
let running = false;
let ranOnce = false;      // the first run also waits for ok.wasm to load

const WASM_URL = new URL("ok.wasm", import.meta.url).href;

function startWorker() {
  if (!worker) {
    worker = new Worker(new URL("playground-worker.js", import.meta.url), { type: "module" });
    worker.postMessage({ warm: true, wasmUrl: WASM_URL });
  }
  return worker;
}

async function loadExample(ex) {
  example = ex;
  for (const b of picker.querySelectorAll("button[data-title]")) b.setAttribute("aria-pressed", String(b.dataset.title === ex.title));
  if (ex.inline) {
    files = { ...ex.inline };
  } else {
    const texts = await Promise.all(
      ex.files.map((f) => fetch(`${ex.dir}/${f}`).then((r) => (r.ok ? r.text() : Promise.reject(new Error(`cannot load ${f}`))))),
    );
    files = Object.fromEntries(ex.files.map((f, i) => [f, texts[i]]));
  }
  created = new Set();
  commandInput.value = ex.command;
  output.replaceChildren();
  status.textContent = "";
  show(Object.keys(files)[0]);
}

function show(path) {
  current = path;
  editor.value = files[path] ?? "";
  editor.setAttribute("aria-label", `Contents of ${path}`);
  tabs.replaceChildren(
    ...Object.keys(files).map((p) => {
      const b = document.createElement("button");
      b.type = "button";
      b.setAttribute("role", "tab");
      b.textContent = p;
      b.setAttribute("aria-selected", String(p === path));
      if (created.has(p)) b.classList.add("new");
      b.addEventListener("click", () => show(p));
      return b;
    }),
  );
}

// Split a command line like a shell would for simple cases: whitespace
// separates words, single or double quotes group them. Leading NAME=value
// words set environment variables, as they do in a shell.
function parseCommand(line) {
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
  if (words[0] !== "ok") throw new Error("the playground runs ok commands only: start the command with `ok`");
  return { env, args: words.slice(1) };
}

// ok colours its error messages with ANSI escapes; turn them into spans.
const ANSI_CLASS = { 1: "b", 31: "red", 32: "green", 33: "yellow", 34: "blue", 35: "magenta", 36: "cyan" };
function appendAnsi(parent, text, base) {
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

function run() {
  if (running) return;
  let parsed;
  try {
    parsed = parseCommand(commandInput.value);
  } catch (e) {
    output.replaceChildren();
    appendAnsi(output, e.message + "\n", "err");
    status.textContent = "";
    return;
  }
  running = true;
  runButton.disabled = true;
  output.replaceChildren();
  status.textContent = ranOnce ? "running…" : "loading ok (about 2 MB, only the first time) and running…";
  ranOnce = true;
  const started = performance.now();
  const w = startWorker();
  const timer = setTimeout(() => {
    w.terminate();
    worker = null;
    finish(`stopped after ${TIME_LIMIT_MS / 1000} s: the playground has a time limit`);
  }, TIME_LIMIT_MS);
  const finish = (note) => {
    clearTimeout(timer);
    running = false;
    runButton.disabled = false;
    status.textContent = note;
  };
  w.onmessage = ({ data }) => {
    for (const [stream, text] of data.out) appendAnsi(output, text, stream);
    if (data.crash) {
      appendAnsi(output, `\n${data.crash}\n`, "err red");
      worker?.terminate();
      worker = null;
      finish("crashed");
      return;
    }
    const before = new Set(Object.keys(files));
    files = Object.fromEntries(Object.entries(data.files).map(([p, bytes]) => [p, decoder.decode(bytes)]));
    for (const p of Object.keys(files)) if (!before.has(p)) created.add(p);
    show(files[current] === undefined ? Object.keys(files)[0] : current);
    const ms = Math.round(performance.now() - started);
    finish(`exit ${data.code} · ${ms} ms${created.size ? " · new files are marked in the tabs" : ""}`);
  };
  w.onerror = (e) => {
    w.terminate();
    worker = null;
    appendAnsi(output, `the playground could not start: ${e.message || "unknown error"}\n`, "err red");
    finish("");
  };
  w.postMessage({
    wasmUrl: WASM_URL,
    args: parsed.args,
    env: parsed.env,
    files: Object.fromEntries(Object.entries(files).map(([p, t]) => [p, encoder.encode(t)])),
  });
}

for (const ex of EXAMPLES) {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = ex.title;
  b.dataset.title = ex.title;
  b.addEventListener("click", () => loadExample(ex).catch(showLoadError));
  picker.insertBefore(b, resetButton);
}

function showLoadError(e) {
  output.replaceChildren();
  appendAnsi(output, `${e.message}\n`, "err red");
}

editor.addEventListener("input", () => {
  files[current] = editor.value;
});
editor.addEventListener("keydown", (e) => {
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    run();
  }
});
commandInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") run();
});
runButton.addEventListener("click", run);
resetButton.addEventListener("click", () => loadExample(example).catch(showLoadError));
// Start loading ok as soon as someone reaches for the playground, so the
// first Run does not wait for the whole download.
root.addEventListener("pointerdown", startWorker, { once: true });
root.addEventListener("focusin", startWorker, { once: true });

loadExample(EXAMPLES[0]).catch(showLoadError);
