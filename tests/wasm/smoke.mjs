// Runs the WebAssembly build of ok (the one the website playground uses)
// under Node's WASI and checks that a script can read and write files, that
// errors render, and that `run` explains it is unavailable.
//
//   node tests/wasm/smoke.mjs target/wasm32-wasip1/release/ok.wasm
import { WASI } from "node:wasi";
import { readFile, writeFile, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const [wasmPath] = process.argv.slice(2);
if (!wasmPath) throw new Error("usage: node tests/wasm/smoke.mjs path/to/ok.wasm");

// Each case runs in a child process: a WASI instance can only start once, and
// the child's stdout/stderr are what ok wrote.
if (process.env.OK_WASM_CHILD) {
  const wasi = new WASI({
    version: "preview1",
    args: ["ok", ...JSON.parse(process.env.OK_WASM_CHILD)],
    preopens: { ".": "." },
    returnOnExit: true,
  });
  const module = await WebAssembly.compile(await readFile(wasmPath));
  process.exitCode = wasi.start(await WebAssembly.instantiate(module, wasi.getImportObject()));
} else {
  const dir = await mkdtemp(join(tmpdir(), "ok-wasm-"));
  await writeFile(join(dir, "users.csv"), "name,active\nbo,true\nal,false\ncy,true\n");
  await writeFile(
    join(dir, "main.ok"),
    [
      'let users = read("users.csv") | where .active == "true" | sort .name',
      'write(users, "out/active.json")',
      'print(os.name, read("out/active.json") | map .name)',
      "",
    ].join("\n"),
  );
  const ok = (...args) => {
    const r = spawnSync(process.execPath, ["--no-warnings", import.meta.filename, join(process.cwd(), wasmPath)], {
      cwd: dir,
      env: { ...process.env, OK_WASM_CHILD: JSON.stringify(args), NO_COLOR: "1" },
      encoding: "utf8",
    });
    return { code: r.status, out: r.stdout, err: r.stderr };
  };
  const checks = [
    ["script with files", ok("main.ok"), (r) => r.code === 0 && r.out === 'wasi ["bo", "cy"]\n'],
    ["error rendering", ok("-e", "print(nope)"), (r) => r.code === 1 && r.err.includes("error[name]: unknown name `nope`")],
    ["run is unavailable", ok("-e", "run git status"), (r) => r.code === 1 && r.err.includes("the WebAssembly build of ok cannot start programs")],
    ["fetch is unavailable", ok("-e", 'fetch("https://example.com")'), (r) => r.code === 1 && r.err.includes("cannot open network connections")],
  ];
  let failed = 0;
  for (const [name, result, pass] of checks) {
    if (pass(result)) {
      console.log(`ok   ${name}`);
    } else {
      failed++;
      console.log(`FAIL ${name}: exit ${result.code}\n--- stdout\n${result.out}--- stderr\n${result.err}`);
    }
  }
  process.exitCode = failed ? 1 : 0;
}
