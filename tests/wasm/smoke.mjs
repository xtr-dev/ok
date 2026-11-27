// Runs the WebAssembly build of ok (the one the website playground uses)
// under Node's WASI and checks that a script can read and write files, that
// errors render, and that `run` explains it is unavailable. It also covers
// the file functions whose std implementation differs on WASI: imports and
// mounts (no canonicalize), the project layouts and `ok self promote`, `touch`, temporary files (no system temp
// directory), `tar` (no tar header metadata) and `os.exe`.
//
//   node tests/wasm/smoke.mjs target/wasm32-wasip1/release/ok.wasm
import { WASI } from "node:wasi";
import { readFile, writeFile, mkdtemp, mkdir, access } from "node:fs/promises";
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
    // Project mode needs a trust store; keep it inside the test directory
    // and trust the test project without a prompt.
    env: { OK_CONFIG_DIR: "/.ok-config", OK_TRUST: "1" },
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
  await mkdir(join(dir, "lib"));
  await writeFile(join(dir, "lib", "greet.ok"), 'fn greet(name) { "hello {name}" }\n');
  await writeFile(join(dir, "imports.ok"), 'import "./lib/greet.ok"\nprint(greet("wasi"))\n');
  await mkdir(join(dir, "proj", "web"), { recursive: true });
  await writeFile(
    join(dir, "proj", "ok.ok"),
    'import "web/ok.ok" as web\n\n# Build everything\ntask build needs [web.bundle] {\n  print("server built")\n}\n',
  );
  await writeFile(join(dir, "proj", "web", "ok.ok"), '# Bundle the frontend\ntask bundle {\n  print("bundle written")\n}\n');
  // The .ok/ layout: a root file and a namespace in .ok/, a mount from
  // there; and a single .ok file that `ok self promote` moves into .ok/.
  await mkdir(join(dir, "dotproj", ".ok"), { recursive: true });
  await mkdir(join(dir, "dotproj", "web"));
  await writeFile(join(dir, "dotproj", ".ok", "ok.ok"), 'import "../web/ok.ok" as web\n\ntask build needs [web.bundle, db.migrate] {\n  print("built")\n}\n');
  await writeFile(join(dir, "dotproj", ".ok", "db.ok"), 'task migrate {\n  print("migrated")\n}\n');
  await writeFile(join(dir, "dotproj", "web", "ok.ok"), 'task bundle {\n  print("bundled")\n}\n');
  await mkdir(join(dir, "single"));
  await writeFile(join(dir, "single", ".ok"), 'task hello {\n  print("hello")\n}\n');
  await writeFile(
    join(dir, "files.ok"),
    [
      'touch("dist/.keep")',
      'touch("dist/.keep")',
      'let t = tempfile(ext: "txt")',
      'write("temp\\n", t)',
      'let d = tempdir()',
      'write("in dir\\n", d / "f.txt")',
      'write("packed\\n", "src/a.txt")',
      'tar("src", "out/src.tar.gz")',
      'tar("src", "out/src.tar")',
      'untar("out/src.tar.gz", "x")',
      'untar("out/src.tar", "y")',
      'print(exists("dist/.keep"), read(t), read(d / "f.txt"), read("x/src/a.txt"), read("y/src/a.txt"), os.exe == "")',
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
    ["import", ok("imports.ok"), (r) => r.code === 0 && r.out === "hello wasi\n"],
    ["missing import", ok("-e", 'import "./nope.ok"'), (r) => r.code === 1 && r.err.includes('cannot import "./nope.ok": no such file')],
    ["mount", ok("-C", "proj", "--yes", "build"), (r) => r.code === 0 && r.out === "bundle written\nserver built\n"],
    ["the .ok/ layout", ok("-C", "dotproj", "-j", "1", "build"), (r) => r.code === 0 && r.out === "bundled\nmigrated\nbuilt\n"],
    ["a single .ok file", ok("-C", "single", "hello"), (r) => r.code === 0 && r.out === "hello\n"],
    ["ok self promote", ok("-C", "single", "self", "promote"), (r) => r.code === 0 && r.out.startsWith("moved .ok to .ok/ok.ok\n")],
    ["after promote", ok("-C", "single", "hello"), (r) => r.code === 0 && r.out === "hello\n"],
    [
      "touch, temp files, tar",
      ok("files.ok"),
      (r) => r.code === 0 && r.out === "true temp\n in dir\n packed\n packed\n true\n",
    ],
  ];
  // Temporary files live in .ok-tmp in the working directory on WASI and
  // are removed, with the directory, when the script ends.
  const tempGone = await access(join(dir, ".ok-tmp")).then(() => false, () => true);
  checks.push(["temp directory removed", { code: tempGone ? 0 : 1, out: "", err: ".ok-tmp is left behind\n" }, (r) => r.code === 0]);
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
