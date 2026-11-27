//! Entry point of the `ok` binary. Kept thin: all behaviour lives in the
//! `okrun` library so it can be tested without spawning processes.

/// Stack size of the interpreter thread. A tree-walking evaluator recurses
/// once per nested call and expression. A large stack lets deep (but
/// bounded) recursion in scripts work the same on every OS, whose default
/// main-thread stacks differ (1 MiB on Windows, 8 MiB on Linux).
#[cfg(not(target_os = "wasi"))]
const STACK_SIZE: usize = 256 * 1024 * 1024;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // WASI has no threads, so the WebAssembly build runs on the main thread.
    // Its stack size is set at link time (see `.cargo/config.toml`).
    #[cfg(target_os = "wasi")]
    let code = okrun::cli::main(args);
    #[cfg(not(target_os = "wasi"))]
    let code = std::thread::Builder::new()
        .name("ok".into())
        .stack_size(STACK_SIZE)
        .spawn(move || okrun::cli::main(args))
        // A failed join means the interpreter panicked. 101 is the exit code
        // Rust itself uses for a panic.
        .map(|h| h.join().unwrap_or(101))
        .unwrap_or_else(|e| {
            eprintln!("error: cannot start the interpreter thread: {e}");
            1
        });
    std::process::exit(code);
}
