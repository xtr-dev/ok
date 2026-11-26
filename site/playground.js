// The playground on the landing page: edit an example, run it with the
// WebAssembly build of ok (in playground-worker.js) and show its output.
//
// Each example is a script from examples/ in the repository plus the data
// files it reads; the Pages workflow copies those folders next to this file,
// so the playground always shows the same files as the repository.

import { appendAnsi, parseCommand, runOk, warm } from "./ok-runner.js";

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

const root = document.getElementById("playground");
const picker = root.querySelector(".pg-examples");
const tabs = root.querySelector(".pg-files");
const editor = root.querySelector(".pg-editor");
const commandInput = root.querySelector(".pg-command input");
const runButton = root.querySelector(".pg-run");
const resetButton = root.querySelector(".pg-reset");
const output = root.querySelector(".pg-output");
const status = root.querySelector(".pg-status");

let files = {};           // path -> text, what the next run sees
let created = new Set();  // paths the last run wrote that the example did not have
let current = null;       // path shown in the editor
let example = null;
let running = false;
let ranOnce = false;      // the first run also waits for ok.wasm to load

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

async function run() {
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
  const result = await runOk({ args: parsed.args, env: parsed.env, files });
  running = false;
  runButton.disabled = false;
  for (const [stream, text] of result.out) appendAnsi(output, text, stream);
  if (result.crash) {
    appendAnsi(output, `\n${result.crash}\n`, "err red");
    status.textContent = "stopped";
    return;
  }
  const before = new Set(Object.keys(files));
  files = result.files;
  for (const p of Object.keys(files)) if (!before.has(p)) created.add(p);
  show(files[current] === undefined ? Object.keys(files)[0] : current);
  const ms = Math.round(performance.now() - started);
  status.textContent = `exit ${result.code} · ${ms} ms${created.size ? " · new files are marked in the tabs" : ""}`;
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
root.addEventListener("pointerdown", warm, { once: true });
root.addEventListener("focusin", warm, { once: true });

loadExample(EXAMPLES[0]).catch(showLoadError);
