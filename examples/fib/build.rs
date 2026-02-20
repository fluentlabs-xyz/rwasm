// build.rs
use std::{env, path::PathBuf, process::Command};

fn main() {
    let target_family = env::var("CARGO_CFG_TARGET_FAMILY").unwrap();
    if target_family == "wasm" {
        return;
    }

    // Re-run the build script when your Rust sources or manifest change.
    // (You can add more rerun-if-changed lines if you have generated inputs.)
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=lib.rs");

    // Guard against recursion: we invoke `cargo build` from inside build.rs,
    // which would normally re-run build.rs again.
    const GUARD: &str = "BUILD_RS_WASM_INNER";
    if env::var_os(GUARD).is_some() {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    // Keep the inner cargo build artifacts isolated from the outer build.
    let inner_target_dir = out_dir.join("wasm-target");
    let wasm_output_dir = inner_target_dir.join("wasm32-unknown-unknown/release/fib.wasm");

    // Equivalent to:
    // cargo b --bin fib --release --target=wasm32-unknown-unknown --no-default-features
    //
    // NOTE: "cargo b" is likely an alias; here we call "cargo build".
    let status = Command::new("cargo")
        .current_dir(&manifest_dir)
        .env(GUARD, "1")
        .env("CARGO_TARGET_DIR", &inner_target_dir)
        .arg("build")
        .arg("--bin")
        .arg("fib")
        .arg("--release")
        .arg("--target=wasm32-unknown-unknown")
        .arg("--no-default-features")
        .status()
        .expect("failed to spawn `cargo build` for wasm32-unknown-unknown");

    if !status.success() {
        panic!("inner `cargo build` failed with status: {status}");
    }

    println!(
        "cargo:rustc-env=OUTPUT_WASM_PATH={}",
        wasm_output_dir.display()
    );
}
