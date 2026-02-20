//! Convert a `cargo-fuzz` artifact (raw fuzzer input bytes) into the generated `.wasm`.
//!
//! Why this exists:
//! The differential fuzz target interprets the fuzzer input as:
//!   bytes -> `Unstructured` -> `Config` -> { wasm-smith | single-inst } -> `.wasm`
//! so the fuzzer artifact is not itself a WebAssembly binary (it won't start with `\0asm`).

use libfuzzer_sys::arbitrary::{Arbitrary, Unstructured};
use std::env;
use std::fs;
use std::path::PathBuf;
use wasmtime_fuzzing::generators::{CompilerStrategy, Config, SingleInstModule};

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

    // Mirror the fuzz target's generation steps.
    let mut config: Config = Config::arbitrary(&mut u)
        .map_err(|e| anyhow::anyhow!("failed to decode Config from artifact: {e:?}"))?;

    // rwasm doesn't support the component model proposals.
    config.module_config.component_model_async = false;
    config.module_config.component_model_async_builtins = false;
    config.module_config.component_model_async_stackful = false;
    config.module_config.component_model_error_context = false;

    // Disable additional Wasm proposals via the underlying wasm-smith config knobs that
    // `wasmtime-fuzzing` will mirror into `wasmtime::Config`.
    //
    // Keep bulk-memory + multi-value on (they're core-ish and rwasm supports them),
    // but turn off the rest of the non-MVP extensions for now.
    {
        let cfg = &mut config.module_config.config;
        cfg.bulk_memory_enabled = true;
        cfg.multi_value_enabled = true;
        cfg.extended_const_enabled = true;
        cfg.sign_extension_ops_enabled = true;
        cfg.reference_types_enabled = true;
        cfg.tail_call_enabled = true;

        cfg.wide_arithmetic_enabled = false;
        cfg.memory64_enabled = false;
        cfg.relaxed_simd_enabled = false;
        cfg.simd_enabled = false;
        cfg.custom_page_sizes_enabled = false;
        cfg.threads_enabled = false;
        cfg.shared_everything_threads_enabled = false;
        cfg.gc_enabled = false;
        cfg.exceptions_enabled = false;
        // Do not use multi memory proposal
        cfg.max_memories = 1;

        // export everything
        cfg.export_everything = true;

        // Ensure broad coverage by ensuring memory and table ops
        // encountered often.
        cfg.min_tables = 1;
        cfg.max_tables = 2;
        cfg.min_memories = 1;
        // Allow invalid funcs to catch compilation differences
        // cfg.allow_invalid_funcs = true
    }
    // Use Cranelift to support tail-call
    config.wasmtime.compiler_strategy = CompilerStrategy::CraneliftNative;
    config.set_differential_config();

    let wasm = match u.int_in_range::<u8>(0..=1) {
        Ok(0) => {
            let module = config
                .generate(&mut u, Some(1000))
                .map_err(|e| anyhow::anyhow!("wasm-smith generation failed: {e:?}"))?;
            module.to_bytes()
        }
        Ok(_) => {
            let module = SingleInstModule::new(&mut u, &config.module_config)
                .map_err(|e| anyhow::anyhow!("single-inst generation failed: {e:?}"))?;
            module.to_bytes()
        }
        Err(e) => return Err(anyhow::anyhow!("artifact did not contain generator selector: {e:?}")),
    };

    fs::write(&out_path, &wasm)?;
    eprintln!(
        "wrote {} bytes to {}",
        wasm.len(),
        out_path.to_string_lossy()
    );
    eprintln!("tip: run `wasm-tools print {}` to see WAT", out_path.to_string_lossy());
    Ok(())
}
