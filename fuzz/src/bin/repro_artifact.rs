//! Convert a `cargo-fuzz` artifact (raw fuzzer input bytes) into the generated `.wasm`.
//!
//! Why this exists:
//! The differential fuzz target interprets the fuzzer input as:
//!   bytes -> `Unstructured` -> `wasm-smith` config -> generated `.wasm`
//! so the fuzzer artifact is not itself a WebAssembly binary (it won't start with `\0asm`).

use libfuzzer_sys::arbitrary::Unstructured;
use std::env;
use std::fs;
use std::path::PathBuf;
use wasm_smith as smith;

fn main() -> anyhow::Result<()> {
    // Usage:
    //   cargo run --manifest-path fuzz/Cargo.toml --bin repro_artifact -- <artifact> [out.wasm]
    let mut args = env::args().skip(1);
    let artifact_path: PathBuf = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: repro_artifact <artifact_path> [out.wasm]"))?;
    let out_path: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("repro.wasm"));

    let data = fs::read(&artifact_path)?;
    let mut u = Unstructured::new(&data);

    // Mirror the fuzz target's module generation constraints.
    let mut gen_cfg = smith::Config::default();
    gen_cfg.bulk_memory_enabled = true;
    gen_cfg.multi_value_enabled = true;
    gen_cfg.extended_const_enabled = true;
    gen_cfg.sign_extension_ops_enabled = true;
    gen_cfg.reference_types_enabled = true;
    gen_cfg.tail_call_enabled = true;

    gen_cfg.memory64_enabled = false;
    gen_cfg.relaxed_simd_enabled = false;
    gen_cfg.simd_enabled = false;
    gen_cfg.custom_page_sizes_enabled = false;
    gen_cfg.threads_enabled = false;
    gen_cfg.shared_everything_threads_enabled = false;
    gen_cfg.gc_enabled = false;
    gen_cfg.exceptions_enabled = false;

    gen_cfg.max_imports = 0;
    gen_cfg.max_memories = 1;
    gen_cfg.min_memories = 1;
    gen_cfg.min_tables = 1;
    gen_cfg.max_tables = 2;
    gen_cfg.export_everything = true;

    let module = smith::Module::new(gen_cfg, &mut u)
        .map_err(|e| anyhow::anyhow!("wasm-smith generation failed: {e:?}"))?;
    let wasm = module.to_bytes();

    fs::write(&out_path, &wasm)?;
    eprintln!(
        "wrote {} bytes to {}",
        wasm.len(),
        out_path.to_string_lossy()
    );
    eprintln!(
        "tip: run `wasm-tools print {}` to see WAT",
        out_path.to_string_lossy()
    );
    Ok(())
}
