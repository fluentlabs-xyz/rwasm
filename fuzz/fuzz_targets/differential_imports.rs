#![no_main]

//! Differential fuzzing of the rwasm and Wasmtime *strategies* with imports enabled.
//!
//! The `differential` target compares the rwasm VM against a raw Wasmtime instance with
//! `max_imports = 0` and skips every module using `memory.grow`. This target drives both sides
//! through `StrategyDefinition`, links a fixed pool of imports with every `SyscallFuelParams`
//! variant, keeps `memory.grow`, and calls each export up to three times on one instance so
//! state carried between calls is compared too.

use libfuzzer_sys::{
    arbitrary::{self, Result, Unstructured},
    fuzz_target,
};
use rwasm::{
    CompilationConfig, ImportLinker, ImportName, StoreTr, StrategyDefinition, StrategyExecutor,
    SyscallFuelParams, TrapCode, TypedCaller, Value,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
use std::sync::{Arc, Once};
use wasm_smith as smith;
use wasmparser::{Parser, Payload, TypeRef, ValType};

const FUEL_LIMIT: u64 = 20_000_000;
const MAX_MEMORY_PAGES: u32 = 64;
const MAX_CALLS_PER_EXPORT: usize = 3;
const MAX_EXPORTS: usize = 6;

/// The import pool offered to wasm-smith; every generated import is one of these.
const AVAILABLE_IMPORTS: &str = r#"(module
  (import "env" "linear" (func (param i32)))
  (import "env" "quadratic" (func (param i32) (result i32)))
  (import "env" "flat" (func (param i64 i32) (result i64)))
  (import "env" "free" (func))
  (import "env" "wide" (func (param i32 i64) (result i32 i64)))
)"#;

fn linker() -> Arc<ImportLinker> {
    let mut linker = ImportLinker::default();
    linker.insert_function(
        ImportName::new("env", "linear"),
        1,
        SyscallFuelParams::LinearFuel(LinearFuelParams {
            base_fuel: 7,
            param_index: 1,
            word_cost: 3,
        }),
        &[ValType::I32],
        &[],
    );
    linker.insert_function(
        ImportName::new("env", "quadratic"),
        2,
        SyscallFuelParams::QuadraticFuel(QuadraticFuelParams {
            local_depth: 1,
            word_cost: 3,
            divisor: 2,
            fuel_denom_rate: 4,
        }),
        &[ValType::I32],
        &[ValType::I32],
    );
    linker.insert_function(
        ImportName::new("env", "flat"),
        3,
        SyscallFuelParams::Const(11),
        &[ValType::I64, ValType::I32],
        &[ValType::I64],
    );
    linker.insert_function(
        ImportName::new("env", "free"),
        4,
        SyscallFuelParams::None,
        &[],
        &[],
    );
    linker.insert_function(
        ImportName::new("env", "wide"),
        5,
        SyscallFuelParams::Const(2),
        &[ValType::I32, ValType::I64],
        &[ValType::I32, ValType::I64],
    );
    Arc::new(linker)
}

/// Deterministic host behaviour that both strategies must observe identically: results derived
/// from the parameters, one memory write, one host-side fuel charge and one host-side trap.
fn handler(
    caller: &mut TypedCaller<'_, ()>,
    idx: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    match idx {
        1 => {
            let x = params[0].i32().unwrap();
            // a memory write the guest can read back; ignored when out of range
            let _ = caller.memory_write((x as u32 % 4096) as usize, &x.to_le_bytes());
            Ok(())
        }
        2 => {
            let x = params[0].i32().unwrap();
            if x == 0x7fff_ffff {
                return Err(TrapCode::UnreachableCodeReached);
            }
            result[0] = Value::I32(x.wrapping_mul(3).wrapping_add(1));
            Ok(())
        }
        3 => {
            let a = params[0].i64().unwrap();
            let b = params[1].i32().unwrap();
            caller.try_consume_fuel((b as u32 % 64) as u64)?;
            result[0] = Value::I64(a ^ ((b as i64) << 7));
            Ok(())
        }
        4 => Ok(()),
        5 => {
            let a = params[0].i32().unwrap();
            let b = params[1].i64().unwrap();
            result[0] = Value::I32(a.wrapping_sub(b as i32));
            result[1] = Value::I64(b.wrapping_add(a as i64));
            Ok(())
        }
        _ => Err(TrapCode::UnknownExternalFunction),
    }
}

static SETUP: Once = Once::new();

fuzz_target!(|data: &[u8]| {
    SETUP.call_once(|| {
        let _ = env_logger::try_init();
    });
    let _ = execute_one(data);
});

#[derive(Debug, PartialEq)]
struct Observation {
    outcome: Result<Vec<Value>, TrapCode>,
    fuel: Option<u64>,
    memory: Vec<u8>,
}

fn observe(
    executor: &mut StrategyExecutor<()>,
    name: &str,
    params: &[Value],
    results: &[ValType],
) -> Observation {
    let mut result: Vec<Value> = results.iter().map(|ty| Value::default(*ty)).collect();
    let outcome = executor.execute(name, params, &mut result).map(|()| result);
    Observation {
        outcome,
        fuel: executor.remaining_fuel(),
        memory: executor.snapshot_memory().unwrap_or_default(),
    }
}

fn execute_one(data: &[u8]) -> Result<()> {
    let mut u = Unstructured::new(data);

    let mut cfg = smith::Config::default();
    cfg.bulk_memory_enabled = true;
    cfg.multi_value_enabled = true;
    cfg.extended_const_enabled = true;
    cfg.sign_extension_ops_enabled = true;
    cfg.reference_types_enabled = true;
    cfg.tail_call_enabled = true;
    cfg.memory64_enabled = false;
    cfg.relaxed_simd_enabled = false;
    cfg.simd_enabled = false;
    cfg.custom_page_sizes_enabled = false;
    cfg.threads_enabled = false;
    cfg.shared_everything_threads_enabled = false;
    cfg.gc_enabled = false;
    cfg.exceptions_enabled = false;
    cfg.allow_floats = false;
    cfg.available_imports = Some(wat::parse_str(AVAILABLE_IMPORTS).unwrap());
    cfg.max_imports = 5;
    cfg.max_memories = 1;
    cfg.min_memories = 1;
    cfg.max_memory32_bytes = u64::from(MAX_MEMORY_PAGES) * 65536;
    cfg.min_tables = 2;
    cfg.max_tables = 4;
    cfg.min_element_segments = 3;
    cfg.max_table_elements = rwasm::N_MAX_TABLE_SIZE as u64;
    cfg.min_data_segments = 1;
    cfg.min_element_segments = 1;
    cfg.export_everything = true;

    let wasm = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        smith::Module::new(cfg, &mut u)
    })) {
        Ok(Ok(module)) => module.to_bytes(),
        _ => return Err(arbitrary::Error::IncorrectFormat),
    };

    // the Wasmtime strategy needs the memory exported; wasm-smith exports everything
    let (exports, has_memory_export) = exported_functions(&wasm);
    if !has_memory_export || exports.is_empty() {
        return Ok(());
    }

    for (name, params, results) in exports.into_iter().take(MAX_EXPORTS) {
        let config = CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name(name.as_str().into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(true)
            .with_max_allowed_memory_pages(MAX_MEMORY_PAGES)
            .with_import_linker(linker());
        let rwasm = StrategyDefinition::new_as_rwasm(config.clone(), &wasm);
        let wasmtime = StrategyDefinition::new_as_wasmtime(config, &wasm, None);
        let (rwasm, wasmtime) = match (rwasm, wasmtime) {
            (Ok(r), Ok(w)) => (r, w),
            (Err(_), Err(_)) => continue,
            (r, w) => panic!(
                "compile divergence for `{name}`: rwasm={:?} wasmtime={:?}\nwasm={}",
                r.err(),
                w.err(),
                hex::encode(&wasm)
            ),
        };
        let mut executors = Vec::new();
        for definition in [rwasm, wasmtime] {
            executors.push(definition.create_executor(
                linker(),
                (),
                handler,
                Some(FUEL_LIMIT),
                Some(MAX_MEMORY_PAGES),
            ));
        }
        let (mut rwasm, mut wasmtime) = match (executors.remove(0), executors.remove(0)) {
            (Ok(r), Ok(w)) => (r, w),
            (Err(r), Err(w)) => {
                assert_eq!(
                    r,
                    w,
                    "instantiation trap divergence for `{name}`\nwasm={}",
                    hex::encode(&wasm)
                );
                continue;
            }
            (r, w) => panic!(
                "instantiation divergence for `{name}`: rwasm={:?} wasmtime={:?}\nwasm={}",
                r.err(),
                w.err(),
                hex::encode(&wasm)
            ),
        };
        assert_eq!(
            rwasm.remaining_fuel(),
            wasmtime.remaining_fuel(),
            "fuel after instantiation for `{name}`\nwasm={}",
            hex::encode(&wasm)
        );
        let calls = 1 + u.int_in_range(0..=MAX_CALLS_PER_EXPORT as u32 - 1)? as usize;
        for call in 0..calls {
            let args = params
                .iter()
                .map(|ty| match ty {
                    ValType::I32 => Ok(Value::I32(u.arbitrary()?)),
                    ValType::I64 => Ok(Value::I64(u.arbitrary()?)),
                    _ => Err(arbitrary::Error::IncorrectFormat),
                })
                .collect::<Result<Vec<_>>>();
            let Ok(args) = args else { break };
            let lhs = observe(&mut rwasm, &name, &args, &results);
            let rhs = observe(&mut wasmtime, &name, &args, &results);
            // documented residual: the call-depth limits differ (rwasm: 1024 frames / the value
            // stack window, Wasmtime: its native stack), so a recursion overflow happens at a
            // different depth and leaves different fuel behind
            if lhs.outcome == Err(TrapCode::StackOverflow)
                && rhs.outcome == Err(TrapCode::StackOverflow)
            {
                break;
            }
            if lhs != rhs {
                panic!(
                    "divergence on `{name}` call #{call} args={args:?}\n  rwasm    = {:?} fuel={:?} mem_len={} mem_hash={:x}\n  wasmtime = {:?} fuel={:?} mem_len={} mem_hash={:x}\nwasm={}",
                    lhs.outcome,
                    lhs.fuel,
                    lhs.memory.len(),
                    hash(&lhs.memory),
                    rhs.outcome,
                    rhs.fuel,
                    rhs.memory.len(),
                    hash(&rhs.memory),
                    hex::encode(&wasm)
                );
            }
            // a parked interruption cannot happen (no handler interrupts); a trap ends the instance
            if lhs.outcome.is_err() {
                break;
            }
        }
    }
    Ok(())
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

type Export = (String, Vec<ValType>, Vec<ValType>);

/// Exported functions with numeric signatures, and whether a memory is exported.
fn exported_functions(wasm: &[u8]) -> (Vec<Export>, bool) {
    let mut types = Vec::new();
    let mut func_types = Vec::new();
    let mut exports = Vec::new();
    let mut memory_exported = false;
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for ty in section.into_iter() {
                    let wasmparser::Type::Func(ty) = ty.unwrap();
                    types.push((ty.params().to_vec(), ty.results().to_vec()));
                }
            }
            Payload::ImportSection(section) => {
                for import in section.into_iter() {
                    if let TypeRef::Func(index) = import.unwrap().ty {
                        func_types.push(index);
                    }
                }
            }
            Payload::FunctionSection(section) => {
                for index in section.into_iter() {
                    func_types.push(index.unwrap());
                }
            }
            Payload::ExportSection(section) => {
                for export in section.into_iter() {
                    let export = export.unwrap();
                    match export.kind {
                        wasmparser::ExternalKind::Func => {
                            let (params, results) =
                                types[func_types[export.index as usize] as usize].clone();
                            if params
                                .iter()
                                .chain(&results)
                                .all(|ty| matches!(ty, ValType::I32 | ValType::I64))
                            {
                                exports.push((export.name.to_string(), params, results));
                            }
                        }
                        wasmparser::ExternalKind::Memory => memory_exported = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (exports, memory_exported)
}
