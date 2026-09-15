#![no_main]

//! Robustness of `RwasmModule::compile` on malformed input.
//!
//! Every other target compiles valid wasm-smith modules. A deployment hands the compiler raw
//! attacker bytes, and a panic anywhere in the parser, the validator or the translator is a node
//! abort, so this target feeds it (a) arbitrary bytes and (b) valid modules with a few bytes
//! corrupted — close enough to valid to reach deep into validation — and only accepts a
//! `Result`. Both strategies are compiled, since the Wasmtime front end must reject exactly what
//! the rwasm front end rejects.

use libfuzzer_sys::{
    arbitrary::{Result, Unstructured},
    fuzz_target,
};
use rwasm::{CompilationConfig, RwasmModule, StrategyDefinition};
use wasm_smith as smith;

fuzz_target!(|data: &[u8]| {
    let _ = execute_one(data);
});

fn execute_one(data: &[u8]) -> Result<()> {
    let mut u = Unstructured::new(data);
    let wasm: Vec<u8> = if data.starts_with(b"\0asm") {
        data.to_vec()
    } else {
        // a valid module, then a handful of byte corruptions driven by the tail of the input
        let corruptions: u8 = u.arbitrary()?;
        let mut cfg = smith::Config::default();
        cfg.bulk_memory_enabled = true;
        cfg.multi_value_enabled = true;
        cfg.extended_const_enabled = true;
        cfg.sign_extension_ops_enabled = true;
        cfg.reference_types_enabled = true;
        cfg.tail_call_enabled = true;
        cfg.allow_floats = false;
        cfg.max_imports = 0;
        cfg.max_memories = 1;
        cfg.min_memories = 1;
        cfg.max_tables = 2;
        cfg.export_everything = true;
        let mut wasm = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            smith::Module::new(cfg, &mut u)
        })) {
            Ok(Ok(module)) => module.to_bytes(),
            _ => return Ok(()),
        };
        for _ in 0..(corruptions % 8) {
            let at: u32 = u.arbitrary()?;
            let value: u8 = u.arbitrary()?;
            if !wasm.is_empty() {
                let at = (at as usize) % wasm.len();
                wasm[at] = value;
            }
        }
        wasm
    };
    let config = CompilationConfig::default_strategy_compatible()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_allow_start_section(true);
    let rwasm = RwasmModule::compile(config.clone(), &wasm).map(|_| ());
    let wasmtime = StrategyDefinition::new_as_wasmtime(config, &wasm, None).map(|_| ());
    // the Wasmtime strategy runs the rwasm front end first: it may reject more (its own
    // compile), never less
    if rwasm.is_err() {
        assert!(
            wasmtime.is_err(),
            "wasmtime accepted a module rwasm rejected: {:?}\nwasm={}",
            rwasm.err(),
            hex::encode(&wasm)
        );
    }
    Ok(())
}
