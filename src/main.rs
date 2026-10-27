use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env = bonbagi_watcher::args::Env {
        ufvk: std::env::var("BONBAGI_UFVK").ok(),
        devtool: std::env::var("BONBAGI_DEVTOOL").ok(),
        source_date_epoch: std::env::var("SOURCE_DATE_EPOCH").ok(),
    };
    let (code, out, err) = bonbagi_watcher::main_with(&args, &env);
    // Ignore a closed pipe (for example `bonbagi-watcher check ... | head`).
    let _ = std::io::stdout().write_all(out.as_bytes());
    let _ = std::io::stderr().write_all(err.as_bytes());
    std::process::exit(code);
}
