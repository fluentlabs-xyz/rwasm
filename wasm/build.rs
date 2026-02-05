// build.rs
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn main() {
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

    // Equivalent to:
    // cargo b --release --target=wasm32-unknown-unknown --no-default-features
    //
    // NOTE: "cargo b" is likely an alias; here we call "cargo build".
    let status = Command::new("cargo")
        .current_dir(&manifest_dir)
        .env(GUARD, "1")
        .env("CARGO_TARGET_DIR", &inner_target_dir)
        .arg("build")
        .arg("--release")
        .arg("--target=wasm32-unknown-unknown")
        .arg("--no-default-features")
        .status()
        .expect("failed to spawn `cargo build` for wasm32-unknown-unknown");

    if !status.success() {
        panic!("inner `cargo build` failed with status: {status}");
    }

    // Locate the produced .wasm.
    // Cargo output file name is usually `<crate_name>.wasm` with '-' replaced by '_'.
    let pkg_name = env::var("CARGO_PKG_NAME").unwrap_or_else(|_| "wasm".to_string());
    let candidate = format!("{}.wasm", pkg_name.replace('-', "_"));

    let built_wasm = inner_target_dir
        .join("wasm32-unknown-unknown")
        .join("release")
        .join(&candidate);

    // Fallback to the hardcoded name from your Makefile if needed.
    let built_wasm = if built_wasm.exists() {
        built_wasm
    } else {
        let fallback = inner_target_dir
            .join("wasm32-unknown-unknown")
            .join("release")
            .join("wasm.wasm");
        if fallback.exists() {
            fallback
        } else {
            panic!(
                "could not find built wasm artifact. Tried:\n- {}\n- {}",
                inner_target_dir
                    .join("wasm32-unknown-unknown/release")
                    .join(&candidate)
                    .display(),
                inner_target_dir
                    .join("wasm32-unknown-unknown/release/wasm.wasm")
                    .display()
            );
        }
    };

    // Equivalent to:
    // cp ./target/wasm32-unknown-unknown/release/wasm.wasm ./lib.wasm
    let lib_wasm = manifest_dir.join("lib.wasm");
    fs::copy(&built_wasm, &lib_wasm).unwrap_or_else(|e| {
        panic!(
            "failed to copy {} -> {}: {e}",
            built_wasm.display(),
            lib_wasm.display()
        )
    });

    // Equivalent to:
    // wasm2wat ./lib.wasm > ./lib.wat || true
    let lib_wat = manifest_dir.join("lib.wat");
    if let Err(e) = write_wat_best_effort(&lib_wasm, &lib_wat) {
        eprintln!("cargo:warning=wasm2wat step failed (ignored, like `|| true`): {e}");
    }
}

fn write_wat_best_effort(input_wasm: &Path, output_wat: &Path) -> Result<(), String> {
    let file = fs::File::create(output_wat)
        .map_err(|e| format!("create {}: {e}", output_wat.display()))?;

    let mut cmd = Command::new("wasm2wat");
    cmd.arg(input_wasm)
        .stdout(Stdio::from(file))
        .stderr(Stdio::inherit());

    match cmd.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("wasm2wat exited with status: {status}")),
        Err(e) => Err(format!("failed to run wasm2wat: {e}")),
    }
}
