#![no_main]

//! Self-differential fuzzing of the rwasm interruption/resume machinery.
//!
//! Two rwasm executors run the same module with the same inputs. The first answers every syscall
//! inline. The second interrupts on every syscall (`TrapCode::InterruptionCalled`), after which
//! the driver applies the same host effects (memory write, fuel charge) and resumes with the same
//! result values. The two runs must end with the same outcome, the same results, the same
//! remaining fuel and the same memory — a host implements cross-contract calls on this path, so
//! any state the resumed run gets wrong is a consensus bug with no Wasmtime oracle to catch it.

use libfuzzer_sys::{
    arbitrary::{self, Result, Unstructured},
    fuzz_target,
};
use rwasm::{
    CompilationConfig, ImportLinker, ImportName, StoreTr, StrategyDefinition, StrategyExecutor,
    SyscallFuelParams, TrapCode, TypedCaller, Value,
};
use rwasm_fuel_policy::{LinearFuelParams, QuadraticFuelParams};
use std::{
    cell::RefCell,
    sync::{Arc, Once},
};
use wasm_smith as smith;
use wasmparser::{Parser, Payload, TypeRef, ValType};

const FUEL_LIMIT: u64 = 20_000_000;
const MAX_MEMORY_PAGES: u32 = 64;
const MAX_CALLS_PER_EXPORT: usize = 3;
const MAX_EXPORTS: usize = 6;
const MAX_INTERRUPTIONS: usize = 200;

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

/// The host effect of a syscall: a memory write, a fuel charge and result values. Applied inline
/// by the reference run and by the driver before resuming the interrupted run.
struct Effect {
    memory_write: Option<(usize, Vec<u8>)>,
    fuel: u64,
    results: Vec<Value>,
}

fn effect(idx: u32, params: &[Value]) -> Effect {
    match idx {
        1 => {
            let x = params[0].i32().unwrap();
            Effect {
                memory_write: Some(((x as u32 % 4096) as usize, x.to_le_bytes().to_vec())),
                fuel: 0,
                results: vec![],
            }
        }
        2 => {
            let x = params[0].i32().unwrap();
            Effect {
                memory_write: None,
                fuel: 0,
                results: vec![Value::I32(x.wrapping_mul(3).wrapping_add(1))],
            }
        }
        3 => {
            let a = params[0].i64().unwrap();
            let b = params[1].i32().unwrap();
            Effect {
                memory_write: None,
                fuel: (b as u32 % 64) as u64,
                results: vec![Value::I64(a ^ ((b as i64) << 7))],
            }
        }
        4 => Effect {
            memory_write: None,
            fuel: 0,
            results: vec![],
        },
        5 => {
            let a = params[0].i32().unwrap();
            let b = params[1].i64().unwrap();
            Effect {
                memory_write: None,
                fuel: 0,
                results: vec![
                    Value::I32(a.wrapping_sub(b as i32)),
                    Value::I64(b.wrapping_add(a as i64)),
                ],
            }
        }
        _ => unreachable!("unknown syscall {idx}"),
    }
}

fn apply<S: StoreTr<()>>(store: &mut S, effect: &Effect) -> Result<(), TrapCode> {
    if let Some((offset, bytes)) = &effect.memory_write {
        let _ = store.memory_write(*offset, bytes);
    }
    if effect.fuel > 0 {
        store.try_consume_fuel(effect.fuel)?;
    }
    Ok(())
}

/// Reference: answers inline.
fn inline_handler(
    caller: &mut TypedCaller<'_, ()>,
    idx: u32,
    params: &[Value],
    result: &mut [Value],
) -> Result<(), TrapCode> {
    let effect = effect(idx, params);
    apply(caller, &effect)?;
    result.clone_from_slice(&effect.results);
    Ok(())
}

thread_local! {
    /// The syscall the interrupting run parked on: `(idx, params)`.
    static PARKED: RefCell<Option<(u32, Vec<Value>)>> = const { RefCell::new(None) };
}

/// Interrupts on every syscall; the driver replays the effect and resumes.
fn interrupting_handler(
    _caller: &mut TypedCaller<'_, ()>,
    idx: u32,
    params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    PARKED.with(|parked| *parked.borrow_mut() = Some((idx, params.to_vec())));
    Err(TrapCode::InterruptionCalled)
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
    interruptions: usize,
}

fn run_inline(
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
        interruptions: 0,
    }
}

fn run_resumed(
    executor: &mut StrategyExecutor<()>,
    name: &str,
    params: &[Value],
    results: &[ValType],
) -> Option<Observation> {
    let mut result: Vec<Value> = results.iter().map(|ty| Value::default(*ty)).collect();
    let mut outcome = executor.execute(name, params, &mut result);
    let mut interruptions = 0;
    while outcome == Err(TrapCode::InterruptionCalled) {
        interruptions += 1;
        if interruptions > MAX_INTERRUPTIONS {
            // too long to compare; cancel and skip this case
            return None;
        }
        let (idx, syscall_params) = PARKED
            .with(|parked| parked.borrow_mut().take())
            .expect("an interruption records the syscall it parked on");
        let effect = effect(idx, &syscall_params);
        // a host-side fuel failure aborts the call the same way an inline handler's does
        if let Err(trap) = apply(executor, &effect) {
            // the reference run traps inside the handler; mirror that by cancelling the parked
            // execution and reporting the trap
            match executor {
                StrategyExecutor::Rwasm { store, .. } => store.reset(true),
                #[allow(unreachable_patterns)]
                _ => unreachable!(),
            }
            outcome = Err(trap);
            break;
        }
        outcome = executor.resume(&effect.results, &mut result);
    }
    Some(Observation {
        outcome: outcome.map(|()| result),
        fuel: executor.remaining_fuel(),
        memory: executor.snapshot_memory().unwrap_or_default(),
        interruptions,
    })
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
    // an interruption inside the start function has no instance handle to resume through
    cfg.allow_start_export = false;
    cfg.available_imports = Some(wat::parse_str(AVAILABLE_IMPORTS).unwrap());
    cfg.max_imports = 5;
    cfg.max_memories = 1;
    cfg.min_memories = 1;
    cfg.max_memory32_bytes = u64::from(MAX_MEMORY_PAGES) * 65536;
    cfg.min_tables = 1;
    cfg.max_tables = 1;
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
    let exports = exported_functions(&wasm);
    if exports.is_empty() {
        return Ok(());
    }

    for (name, params, results) in exports.into_iter().take(MAX_EXPORTS) {
        let config = CompilationConfig::default()
            .with_entrypoint_name(name.as_str().into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_builtins_consume_fuel(true)
            .with_max_allowed_memory_pages(MAX_MEMORY_PAGES)
            .with_import_linker(linker());
        let Ok(definition) = StrategyDefinition::new_as_rwasm(config, &wasm) else {
            continue;
        };
        let reference = definition.create_executor(
            linker(),
            (),
            inline_handler,
            Some(FUEL_LIMIT),
            Some(MAX_MEMORY_PAGES),
        );
        let resumed = definition.create_executor(
            linker(),
            (),
            interrupting_handler,
            Some(FUEL_LIMIT),
            Some(MAX_MEMORY_PAGES),
        );
        let (mut reference, mut resumed) = match (reference, resumed) {
            (Ok(a), Ok(b)) => (a, b),
            _ => continue,
        };
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
            let lhs = run_inline(&mut reference, &name, &args, &results);
            let Some(rhs) = run_resumed(&mut resumed, &name, &args, &results) else {
                break;
            };
            if lhs.outcome != rhs.outcome || lhs.fuel != rhs.fuel || lhs.memory != rhs.memory {
                panic!(
                    "resume divergence on `{name}` call #{call} args={args:?} after {} interruptions\n  inline  = {:?} fuel={:?} mem_len={} mem_hash={:x}\n  resumed = {:?} fuel={:?} mem_len={} mem_hash={:x}\nwasm={}",
                    rhs.interruptions,
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

fn exported_functions(wasm: &[u8]) -> Vec<Export> {
    let mut types = Vec::new();
    let mut func_types = Vec::new();
    let mut exports = Vec::new();
    for payload in Parser::new(0).parse_all(wasm) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for rec_group in section.into_iter() {
                    for ty in rec_group.unwrap().into_types() {
                        let ty = ty.unwrap_func();
                        types.push((ty.params().to_vec(), ty.results().to_vec()));
                    }
                }
            }
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
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
                    if export.kind == wasmparser::ExternalKind::Func {
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
                }
            }
            _ => {}
        }
    }
    exports
}
