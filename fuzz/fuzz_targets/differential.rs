#![no_main]

//! Differential fuzz target inspired by Wasmtime's `fuzz_targets/differential.rs`.
//!
//! Key changes from Wasmtime's version:
//! - Only compare **rwasm vs wasmtime**
//! - No `ALLOWED_*` env vars
//! - Module generation uses `wasm-smith` constrained to the currently supported rwasm subset
//!
//! Reference: `https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/fuzz/fuzz_targets/differential.rs`

use anyhow::Context;
use libfuzzer_sys::{
    arbitrary::{self, Result, Unstructured},
    fuzz_target,
};
use rwasm::{
    CompilationConfig, CompilationError, ExecutionEngine, ExternRef, FuncRef, RwasmModule,
    RwasmStore, StoreTr, TrapCode, Value,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering::SeqCst},
    Once,
};
use wasm_smith as smith;
use wasmparser::{Parser, Payload, ValType as ParserValType};
use wasmtime::{
    Engine, Extern, ExternRef as WasmtimeExternRef, FuncType, Instance, Module, Ref, Store, Val,
};

/// Upper limit on the number of invocations for each WebAssembly function.
const NUM_INVOCATIONS: usize = 5;
const MAX_EXPORTS: usize = 8;

/// Starting fuel for both engines in each differential run.
///
/// We compare *remaining* fuel after execution, which implies consumed fuel must match too.
const FUEL_LIMIT: u64 = 50_000_000;

/// How many table elements we snapshot/compare (nullness only) for each exported table.
///
/// Keeping this bounded prevents pathological slowdown when tables grow very large.
const TABLE_NULLNESS_PREFIX_ELEMS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
enum DiffValue {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    FuncRef { null: bool },
    ExternRef { null: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiffValueType {
    I32,
    I64,
    F32,
    F64,
    FuncRef,
    ExternRef,
}

impl TryFrom<ParserValType> for DiffValueType {
    type Error = ();

    fn try_from(value: ParserValType) -> std::result::Result<Self, Self::Error> {
        match value {
            ParserValType::I32 => Ok(Self::I32),
            ParserValType::I64 => Ok(Self::I64),
            ParserValType::F32 => Ok(Self::F32),
            ParserValType::F64 => Ok(Self::F64),
            ParserValType::FuncRef => Ok(Self::FuncRef),
            ParserValType::ExternRef => Ok(Self::ExternRef),
            _ => Err(()),
        }
    }
}

impl TryFrom<wasmtime::ValType> for DiffValueType {
    type Error = ();

    fn try_from(value: wasmtime::ValType) -> std::result::Result<Self, Self::Error> {
        match value {
            wasmtime::ValType::I32 => Ok(Self::I32),
            wasmtime::ValType::I64 => Ok(Self::I64),
            wasmtime::ValType::F32 => Ok(Self::F32),
            wasmtime::ValType::F64 => Ok(Self::F64),
            wasmtime::ValType::Ref(r) => match r.heap_type() {
                wasmtime::HeapType::Func => Ok(Self::FuncRef),
                wasmtime::HeapType::Extern => Ok(Self::ExternRef),
                _ => Err(()),
            },
            _ => Err(()),
        }
    }
}

fn arbitrary_diff_value_of_type(u: &mut Unstructured<'_>, ty: DiffValueType) -> Result<DiffValue> {
    Ok(match ty {
        DiffValueType::I32 => DiffValue::I32(u.arbitrary::<i32>()?),
        DiffValueType::I64 => DiffValue::I64(u.arbitrary::<i64>()?),
        DiffValueType::F32 => DiffValue::F32(u.arbitrary::<u32>()?),
        DiffValueType::F64 => DiffValue::F64(u.arbitrary::<u64>()?),
        DiffValueType::FuncRef => DiffValue::FuncRef {
            null: u.arbitrary::<bool>()?,
        },
        DiffValueType::ExternRef => DiffValue::ExternRef {
            null: u.arbitrary::<bool>()?,
        },
    })
}

/// Only run once when the fuzz target loads.
static SETUP: Once = Once::new();

/// Statistics about what's actually getting executed during fuzzing.
static STATS: RuntimeStats = RuntimeStats::new();

fuzz_target!(|data: &[u8]| {
    SETUP.call_once(|| {
        let _ = env_logger::try_init();
    });

    // Errors in `execute_one` are typically "not enough bytes" from `Unstructured`;
    // ignore them for fuzzing throughput.
    let _ = execute_one(data);
});

fn execute_one(data: &[u8]) -> Result<()> {
    let mut u = Unstructured::new(data);

    STATS.bump_attempts();

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

    // Keep generated modules inside the currently supported differential subset.
    gen_cfg.max_imports = 0;
    gen_cfg.max_memories = 1;
    gen_cfg.min_memories = 1;
    gen_cfg.min_tables = 1;
    // Keep table model inside currently stable rwasm differential subset.
    gen_cfg.max_tables = 1;
    gen_cfg.export_everything = true;

    STATS.wasm_smith_modules.fetch_add(1, SeqCst);
    let wasm = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        smith::Module::new(gen_cfg, &mut u)
    })) {
        Ok(Ok(module)) => module.to_bytes(),
        Ok(Err(_)) => return Err(arbitrary::Error::IncorrectFormat),
        Err(_) => return Err(arbitrary::Error::IncorrectFormat),
    };

    log::trace!("generated wasm bytes: {}", hex::encode(&wasm));

    // We parse exports/types once; this is used by the rwasm-side snapshots.
    // If we can't parse the module, we can't reliably snapshot/compare side effects.
    let export_map = match parse_export_map(&wasm) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };

    // Instantiate RHS once to enumerate exported functions, matching Wasmtime's harness.
    // Note: rwasm currently recompiles per-export (entrypoint-by-name), so for semantic
    // comparability we will instantiate RHS fresh inside each evaluation below.
    let exports: Vec<(String, wasmtime::FuncType)> = {
        let rhs_store = create_wasmtime_store();
        let rhs_module = match wasmtime::Module::new(rhs_store.engine(), &wasm) {
            Ok(m) => m,
            Err(_e) => {
                return Ok(());
            }
        };
        // Prefer enumerating exports from an instantiated module, but if instantiation fails
        // (e.g. start-function traps) we can still enumerate export types from the compiled Module.
        match RawWasmtimeInstance::new(rhs_store, rhs_module.clone()) {
            Ok(mut rhs) => rhs
                .exported_functions()
                .into_iter()
                .take(MAX_EXPORTS)
                .collect(),
            Err(_) => wasmtime_module_exported_functions(&rhs_module)
                .into_iter()
                .take(MAX_EXPORTS)
                .collect(),
        }
    };

    if exports.is_empty() {
        return Ok(());
    }

    // Call each exported function with different sets of arguments.
    'outer: for (name, signature) in exports {
        let mut invocations = 0usize;
        loop {
            // Generate DiffValue args from the signature.
            let arguments = match signature
                .params()
                .map(|ty| {
                    let ty = ty
                        .try_into()
                        .map_err(|_| arbitrary::Error::IncorrectFormat)?;
                    arbitrary_diff_value_of_type(&mut u, ty)
                })
                .collect::<Result<Vec<_>>>()
            {
                Ok(args) => args,
                Err(_) => continue 'outer,
            };

            let result_tys = match signature
                .results()
                .map(|ty| {
                    DiffValueType::try_from(ty).map_err(|_| arbitrary::Error::IncorrectFormat)
                })
                .collect::<Result<Vec<_>>>()
            {
                Ok(tys) => tys,
                Err(_) => continue 'outer,
            };

            // Build a fresh RHS Wasmtime instance for this evaluation.
            // This keeps semantics comparable with rwasm's "entrypoint does init + call" execution model.
            let rhs_store = create_wasmtime_store();
            let rhs_module = match wasmtime::Module::new(rhs_store.engine(), &wasm) {
                Ok(m) => m,
                Err(_e) => {
                    return Ok(());
                }
            };
            let mut rhs = match RawWasmtimeInstance::new(rhs_store, rhs_module) {
                Ok(i) => i,
                Err(_e) => {
                    return Ok(());
                }
            };

            // Run the Wasmtime-style oracle for rwasm vs Wasmtime.
            let ok = differential_rwasm_vs_wasmtime(
                &wasm,
                &export_map,
                &mut rhs,
                &name,
                &arguments,
                &result_tys,
            )
            .map_err(|_| arbitrary::Error::IncorrectFormat)?;
            if !ok {
                break 'outer;
            }

            invocations += 1;
            STATS.total_invocations.fetch_add(1, SeqCst);
            STATS.successes.fetch_add(1, SeqCst);

            if invocations > NUM_INVOCATIONS || u.is_empty() {
                break;
            }
        }
    }

    Ok(())
}

fn wasmtime_module_exported_functions(module: &Module) -> Vec<(String, FuncType)> {
    module
        .exports()
        .filter_map(|e| match e.ty() {
            wasmtime::ExternType::Func(f) => Some((e.name().to_string(), f)),
            _ => None,
        })
        .collect()
}

/// Wasmtime-style differential oracle: compare one export invocation and then compare exported state.
///
/// Returns `Ok(true)` if further evaluations can continue, or `Ok(false)` if the RHS instance
/// is considered "drifted" (e.g. OOM) and fuzzing should stop for this module.
fn differential_rwasm_vs_wasmtime(
    wasm: &[u8],
    export_map: &ExportMap,
    rhs: &mut RawWasmtimeInstance,
    name: &str,
    args: &[DiffValue],
    result_tys: &[DiffValueType],
) -> anyhow::Result<bool> {
    log::debug!("Evaluating: `{name}` with {args:?}");

    // LHS: rwasm evaluation (compile with `name` as entrypoint).
    let lhs_results = match run_rwasm_one(wasm, name, args, result_tys, export_map) {
        Ok(Some((results, snap, fuel_remaining))) => Ok((results, snap, fuel_remaining)),
        Ok(None) => return Ok(true), // module requires unsupported functionality in current rwasm
        Err(trap) => Err(trap),
    };
    log::debug!(
        " -> lhs results on rwasm: {:?}",
        lhs_results.as_ref().map(|(r, _, _)| r)
    );

    // RHS: Wasmtime evaluation.
    let rhs_results = rhs.evaluate(name, args, result_tys);
    log::debug!(" -> rhs results on wasmtime: {:?}", &rhs_results);

    // Mirror Wasmtime oracle: if Wasmtime hit OOM, stop without declaring mismatch.
    if rhs.is_oom() {
        return Ok(false);
    }

    match (lhs_results, rhs_results) {
        // LHS ok, RHS ok: compare return values and then compare exported state.
        (
            Ok((lhs_vals, lhs_snap, lhs_fuel_remaining)),
            Ok(Some((rhs_vals, rhs_fuel_remaining))),
        ) => {
            if lhs_vals != rhs_vals {
                panic!(
                    "diff results: export={name} args={args:?} result_tys={result_tys:?}\n\
                     rwasm={lhs_vals:?}\n\
                     wasmtime={rhs_vals:?}\n"
                );
            }

            let lhs_consumed = FUEL_LIMIT.saturating_sub(lhs_fuel_remaining);
            let rhs_consumed = FUEL_LIMIT.saturating_sub(rhs_fuel_remaining);
            if lhs_consumed != rhs_consumed {
                panic!(
                    "diff fuel: export={name} args={args:?} result_tys={result_tys:?}\n\
                     rwasm_remaining={lhs_fuel_remaining} rwasm_consumed={lhs_consumed}\n\
                     wasmtime_remaining={rhs_fuel_remaining} wasmtime_consumed={rhs_consumed}\n"
                );
            }

            // Compare exported globals (by name+type).
            //
            // IMPORTANT: globals must always be compared (no silent skipping), otherwise we can miss
            // real engine mismatches.
            let exported_globals = rhs.exported_globals();
            for (global, ty) in exported_globals.iter().cloned() {
                log::debug!("Comparing global `{global}`");
                let lhs_val = lhs_snap
                    .get_global(&global, ty, export_map)
                    .unwrap_or_else(|| {
                        panic!(
                        "state-compare skipped global: export={name} global={global} ty={ty:?} \
                         (lhs snapshot could not resolve it)"
                    )
                    });
                let rhs_val = rhs.get_global(&global, ty).unwrap();
                assert_eq!(lhs_val, rhs_val);
            }

            // Compare exported memories (full bytes), matching Wasmtime's strategy.
            let exported_memories = rhs.exported_memories();
            for (memory, shared) in exported_memories.iter().cloned() {
                log::debug!("Comparing memory `{memory}`");
                let idx = *export_map
                    .exported_memories
                    .get(&memory)
                    .unwrap_or_else(|| {
                        panic!("state-compare missing memory in export map: {memory}")
                    });
                if shared {
                    panic!(
                        "state-compare cannot compare shared memory export={name} memory={memory} idx={idx}"
                    );
                }
                if idx != 0 {
                    panic!(
                        "state-compare cannot compare non-zero memory index export={name} memory={memory} idx={idx}"
                    );
                }
                let lhs_mem = lhs_snap.get_memory(&memory, shared, export_map).unwrap_or_else(|| {
                    panic!(
                        "state-compare skipped memory: export={name} memory={memory} shared={shared} idx={idx} \
                         (lhs snapshot could not resolve it)"
                    )
                });
                let rhs_mem = rhs.get_memory(&memory, shared).unwrap();
                if lhs_mem != rhs_mem {
                    eprintln!("rwasm memory is    {} bytes long", lhs_mem.len());
                    eprintln!("wasmtime memory is {} bytes long", rhs_mem.len());
                    panic!("memories have differing values");
                }
            }

            // Compare exported tables (bounded nullness prefix), so we catch mismatches in table
            // mutations that might not directly affect return values/memory/globals.
            //
            // Note: we intentionally compare only null/non-null state of elements (not exact funcref
            // identity), which is stable across engines and matches what rwasm can snapshot cheaply.
            for (table, _) in export_map.exported_tables.iter() {
                log::debug!("Comparing table `{table}`");
                let Some(lhs_tbl) = lhs_snap.get_table_nullness_prefix(table, export_map) else {
                    // This can happen when generated modules still exercise a table layout outside
                    // the currently comparable rwasm subset.
                    STATS.unsupported_modules.fetch_add(1, SeqCst);
                    log::debug!(
                        "skipping module: unresolved lhs table snapshot export={name} table={table}"
                    );
                    return Ok(true);
                };
                let Some(rhs_tbl) =
                    wasmtime_table_nullness_prefix(rhs, table, TABLE_NULLNESS_PREFIX_ELEMS)
                else {
                    STATS.unsupported_modules.fetch_add(1, SeqCst);
                    log::debug!(
                        "skipping module: unresolved rhs table snapshot export={name} table={table}"
                    );
                    return Ok(true);
                };
                assert_eq!(lhs_tbl, rhs_tbl);
            }

            Ok(true)
        }
        // LHS trap, RHS trap: considered equivalent (coarser than Wasmtime's Trap equality).
        (Err(_), Err(_)) => Ok(true),

        // If Wasmtime side cannot represent the invocation in the currently comparable subset,
        // skip this case instead of reporting a false mismatch.
        (_, Ok(None)) => Ok(true),

        // LHS trap, RHS ok: mismatch.
        (Err(lhs_trap), Ok(Some(_))) => {
            panic!("diff: export={name} wasmtime=Ok rwasm_trap={lhs_trap:?} args={args:?} result_tys={result_tys:?}\n")
        }

        // LHS ok, RHS trap: mismatch.
        (Ok(_), Err(rhs_err)) => {
            panic!("diff: export={name} wasmtime_trap={rhs_err:?} rwasm=Ok args={args:?} result_tys={result_tys:?}\n")
        }
    }
}

fn run_rwasm_one(
    wasm: &[u8],
    export: &str,
    args: &[DiffValue],
    results_t: &[DiffValueType],
    export_map: &ExportMap,
) -> Result<Option<(Vec<DiffValue>, RwasmSnapshot, u64)>, TrapCode> {
    let config = CompilationConfig::default()
        .with_entrypoint_name(export.into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_allow_start_section(true)
        .with_consume_fuel(true);

    let (module, _) = match RwasmModule::compile(config, wasm) {
        Ok(x) => x,
        Err(e) => {
            if is_unsupported_rwasm_compilation_error(&e) {
                STATS.unsupported_modules.fetch_add(1, SeqCst);
                return Ok(None);
            }
            // If this is not an expected unsupported feature class, keep it loud.
            panic!(
                "compile-diff: export={export} rwasm_compilation_error={e:?} ({e}) args={args:?} result_tys={results_t:?}\n"
            );
        }
    };

    let engine = ExecutionEngine::default();
    let mut store = RwasmStore::<()>::default();
    store.reset_fuel(FUEL_LIMIT);

    let fallback_funcref_idx = export_map.exported_funcs.get(export).copied();
    let params: Vec<Value> = match args
        .iter()
        .map(|v| diff_value_to_rwasm(v, fallback_funcref_idx))
        .collect::<Option<Vec<_>>>()
    {
        Some(p) => p,
        None => {
            STATS.unsupported_modules.fetch_add(1, SeqCst);
            return Ok(None);
        }
    };
    let mut results: Vec<Value> = results_t.iter().map(zero_rwasm_from_diff_type).collect();

    engine.execute(&mut store, &module, &params, &mut results)?;

    let vals = results
        .iter()
        .zip(results_t.iter())
        .map(|(v, t)| rwasm_value_to_diff(v, t))
        .collect::<Option<Vec<_>>>()
        .ok_or(TrapCode::IllegalOpcode)?;

    let snap = RwasmSnapshot::new(export_map, &store);
    let fuel_remaining = store.remaining_fuel().unwrap_or(0);
    Ok(Some((vals, snap, fuel_remaining)))
}

fn is_unsupported_rwasm_compilation_error(err: &CompilationError) -> bool {
    matches!(
        err,
        CompilationError::NotSupportedExtension
            | CompilationError::NotSupportedImportType
            | CompilationError::NotSupportedFuncType
            | CompilationError::MalformedImportFunctionType
            | CompilationError::UnresolvedImportFunction
            | CompilationError::NonDefaultMemoryIndex
            | CompilationError::NotSupportedLocalType
            | CompilationError::NotSupportedGlobalType
            | CompilationError::StartSectionsAreNotAllowed
            | CompilationError::MissingEntrypoint
    )
}

/// Creates a Wasmtime store, with signal-based traps disabled on macOS.
///
/// On macOS (or when the `disable-signals` feature is enabled), this disables
/// signal-based traps because libfuzzer installs its own signal handlers for
/// detecting "deadly signals". When wasmtime uses signal-based traps (the default),
/// wasm traps like `unreachable` or divide-by-zero generate hardware signals
/// (SIGSEGV, SIGFPE) that wasmtime normally catches. However, libfuzzer may
/// intercept these signals first, causing false positive "deadly signal" crashes
/// that don't reproduce when re-running artifacts.
///
/// With signals_based_traps disabled, wasmtime uses explicit bounds checks and
/// trap instructions instead, avoiding the signal handler conflict.
#[cfg(any(target_os = "macos", feature = "disable-signals"))]
fn create_wasmtime_store() -> Store<()> {
    let mut wasmtime_config = wasmtime::Config::new();
    wasmtime_config.consume_fuel(true);
    wasmtime_config.signals_based_traps(false);
    // When signals_based_traps is disabled, spectre mitigations must also be disabled.
    // This is because spectre mitigations rely on faults from out-of-bounds accesses
    // which won't be caught without signal handlers.
    // SAFETY: These are valid cranelift settings.
    unsafe {
        wasmtime_config.cranelift_flag_set("enable_heap_access_spectre_mitigation", "false");
        wasmtime_config.cranelift_flag_set("enable_table_access_spectre_mitigation", "false");
    }
    let engine = Engine::new(&wasmtime_config).unwrap();
    let mut store = Store::new(&engine, ());
    store
        .set_fuel(FUEL_LIMIT)
        .expect("failed to set wasmtime fuel for differential fuzzing");
    store
}

/// Creates a Wasmtime store using the default configuration.
#[cfg(not(any(target_os = "macos", feature = "disable-signals")))]
fn create_wasmtime_store() -> Store<()> {
    let mut wasmtime_config = wasmtime::Config::new();
    wasmtime_config.consume_fuel(true);
    let engine = Engine::new(&wasmtime_config).unwrap();
    let mut store = Store::new(&engine, ());
    store
        .set_fuel(FUEL_LIMIT)
        .expect("failed to set wasmtime fuel for differential fuzzing");
    store
}

#[derive(Debug, Clone)]
struct RwasmSnapshot {
    /// Full linear memory snapshot (rwasm only supports memory 0).
    memory_full: Vec<u8>,
    /// Exported globals snapshot keyed by global index.
    globals_by_index: Vec<(u32, DiffValueType, DiffValue)>,

    /// Per-table snapshot for differential comparisons: `(table_index, size, nullness_prefix)`.
    ///
    /// `nullness_prefix` contains `0/1` bytes describing whether each element is null (0) or
    /// non-null (1) for the first `TABLE_NULLNESS_PREFIX_ELEMS` elements.
    tables_nullness_prefix: Vec<(u32, u32, Vec<u8>)>,
}

impl RwasmSnapshot {
    fn new(export_map: &ExportMap, store: &RwasmStore<()>) -> Self {
        let memory_full = store.memory_snapshot();
        let tables_nullness_prefix =
            store.table_snapshots_nullness_prefix(TABLE_NULLNESS_PREFIX_ELEMS);

        let mut globals_by_index: Vec<(u32, DiffValueType, DiffValue)> = Vec::new();
        for (idx, kind) in export_map.global_kinds.iter().enumerate() {
            let idx = idx as u32;
            let (ty, val) = match kind {
                GlobalKind::I32 => {
                    let bits = store.global_word_bits(idx * 2);
                    (DiffValueType::I32, DiffValue::I32(bits as i32))
                }
                GlobalKind::F32 => {
                    let bits = store.global_word_bits(idx * 2);
                    (DiffValueType::F32, DiffValue::F32(bits))
                }
                GlobalKind::I64 => {
                    // rwasm stores i64/f64 globals as two 32-bit words but with the high word first:
                    // - word 0 at `idx*2` is the high 32 bits
                    // - word 1 at `idx*2+1` is the low 32 bits
                    let hi = store.global_word_bits(idx * 2) as u64;
                    let lo = store.global_word_bits(idx * 2 + 1) as u64;
                    let v = ((hi << 32) | lo) as i64;
                    (DiffValueType::I64, DiffValue::I64(v))
                }
                GlobalKind::F64 => {
                    let hi = store.global_word_bits(idx * 2) as u64;
                    let lo = store.global_word_bits(idx * 2 + 1) as u64;
                    let bits = (hi << 32) | lo;
                    (DiffValueType::F64, DiffValue::F64(bits))
                }
                GlobalKind::FuncRef => {
                    let bits = store.global_word_bits(idx * 2);
                    (
                        DiffValueType::FuncRef,
                        DiffValue::FuncRef { null: bits == 0 },
                    )
                }
                GlobalKind::ExternRef => {
                    let bits = store.global_word_bits(idx * 2);
                    (
                        DiffValueType::ExternRef,
                        DiffValue::ExternRef { null: bits == 0 },
                    )
                }
            };
            globals_by_index.push((idx, ty, val));
        }
        globals_by_index.sort_by_key(|(idx, _, _)| *idx);

        Self {
            memory_full,
            globals_by_index,
            tables_nullness_prefix,
        }
    }

    fn get_global(
        &self,
        name: &str,
        ty: DiffValueType,
        export_map: &ExportMap,
    ) -> Option<DiffValue> {
        let idx = *export_map.exported_globals.get(name)?;
        self.globals_by_index
            .iter()
            // `DiffValueType` (from wasmtime-fuzzing) intentionally does not implement `PartialEq`,
            // so compare by discriminant (variant identity).
            .find(|(i, t, _)| {
                *i == idx && core::mem::discriminant(t) == core::mem::discriminant(&ty)
            })
            .map(|(_, _, v)| v.clone())
    }

    fn get_memory(&self, name: &str, shared: bool, export_map: &ExportMap) -> Option<Vec<u8>> {
        if shared {
            return None;
        }
        let idx = *export_map.exported_memories.get(name)?;
        if idx != 0 {
            return None;
        }
        Some(self.memory_full.clone())
    }

    fn get_table_nullness_prefix(
        &self,
        name: &str,
        export_map: &ExportMap,
    ) -> Option<(u32, Vec<u8>)> {
        let idx = *export_map.exported_tables.get(name)?;
        self.tables_nullness_prefix
            .iter()
            .find(|(i, _, _)| *i == idx)
            .map(|(_, size, prefix)| (*size, prefix.clone()))
    }
}

/// Like `wasmtime-fuzzing`'s `WasmtimeInstance`, but with access to the underlying `Store`/`Instance`
/// so we can read exported tables for the table oracle.
struct RawWasmtimeInstance {
    store: Store<()>,
    instance: Instance,
}

impl RawWasmtimeInstance {
    fn new(mut store: Store<()>, module: Module) -> anyhow::Result<Self> {
        let instance = Instance::new(&mut store, &module, &[])
            .context("unable to instantiate module in wasmtime")?;
        Ok(Self { store, instance })
    }

    fn exported_functions(&mut self) -> Vec<(String, FuncType)> {
        let exported_functions = self
            .instance
            .exports(&mut self.store)
            .map(|e| (e.name().to_owned(), e.into_func()))
            .filter_map(|(n, f)| f.map(|f| (n, f)))
            .collect::<Vec<_>>();
        exported_functions
            .into_iter()
            .map(|(n, f)| (n, f.ty(&self.store)))
            .collect()
    }

    fn exported_globals(&mut self) -> Vec<(String, DiffValueType)> {
        let globals = self
            .instance
            .exports(&mut self.store)
            .filter_map(|e| {
                let name = e.name();
                e.into_global().map(|g| (name.to_string(), g))
            })
            .collect::<Vec<_>>();

        globals
            .into_iter()
            .filter_map(|(name, global)| {
                DiffValueType::try_from(global.ty(&self.store).content().clone())
                    .map(|ty| (name, ty))
                    .ok()
            })
            .collect()
    }

    fn exported_memories(&mut self) -> Vec<(String, bool)> {
        self.instance
            .exports(&mut self.store)
            .filter_map(|e| {
                let name = e.name();
                match e.into_extern() {
                    Extern::Memory(_) => Some((name.to_string(), false)),
                    Extern::SharedMemory(_) => Some((name.to_string(), true)),
                    _ => None,
                }
            })
            .collect()
    }

    fn is_oom(&self) -> bool {
        // OOM detection isn't available through the public API in this harness.
        false
    }

    fn evaluate(
        &mut self,
        function_name: &str,
        arguments: &[DiffValue],
        _results: &[DiffValueType],
    ) -> anyhow::Result<Option<(Vec<DiffValue>, u64)>> {
        let function = self
            .instance
            .get_func(&mut self.store, function_name)
            .expect("unable to access exported function");

        let arguments: Vec<_> = match arguments
            .iter()
            .map(|v| diff_value_to_wasmtime_val(&mut self.store, v, Some(&function)))
            .collect::<Option<Vec<_>>>()
        {
            Some(args) => args,
            None => return Ok(None),
        };
        let ty = function.ty(&self.store);
        let mut results = vec![Val::I32(0); ty.results().len()];
        function.call(&mut self.store, &arguments, &mut results)?;

        let results = results
            .into_iter()
            .map(wasmtime_val_to_diff_value)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| anyhow::anyhow!("unsupported result type"))?;

        let remaining_fuel = self
            .store
            .get_fuel()
            .map_err(|e| anyhow::anyhow!("failed to read wasmtime fuel: {e}"))?;

        Ok(Some((results, remaining_fuel)))
    }

    fn get_global(&mut self, name: &str, _ty: DiffValueType) -> Option<DiffValue> {
        let g = self.instance.get_global(&mut self.store, name)?;
        wasmtime_val_to_diff_value(g.get(&mut self.store))
    }

    fn get_memory(&mut self, name: &str, shared: bool) -> Option<Vec<u8>> {
        Some(if shared {
            let memory = self
                .instance
                .get_shared_memory(&mut self.store, name)
                .unwrap();
            memory.data().iter().map(|i| unsafe { *i.get() }).collect()
        } else {
            self.instance
                .get_memory(&mut self.store, name)
                .unwrap()
                .data(&self.store)
                .to_vec()
        })
    }
}

fn wasmtime_table_nullness_prefix(
    rhs: &mut RawWasmtimeInstance,
    table_name: &str,
    max_elems: usize,
) -> Option<(u32, Vec<u8>)> {
    // NOTE: this intentionally accesses Wasmtime's table element *nullness* only (0/1),
    // not the exact identity of funcref entries (which can vary by engine/allocator).
    let table = rhs.instance.get_table(&mut rhs.store, table_name)?;
    let size: u64 = table.size(&rhs.store);
    let size_u32: u32 = size.try_into().ok()?;
    let n = core::cmp::min(size as usize, max_elems);
    let mut prefix = Vec::with_capacity(n);
    for i in 0..n {
        let v: Ref = table.get(&mut rhs.store, i as u64)?;
        let is_non_null = match v {
            Ref::Func(None) => false,
            Ref::Func(Some(_)) => true,
            Ref::Extern(None) => false,
            Ref::Extern(Some(_)) => true,
            // Unknown/unsupported ref type.
            _ => return None,
        };
        prefix.push(if is_non_null { 1 } else { 0 });
    }
    Some((size_u32, prefix))
}

fn diff_value_to_wasmtime_val(
    store: &mut Store<()>,
    v: &DiffValue,
    fallback_funcref: Option<&wasmtime::Func>,
) -> Option<Val> {
    Some(match v {
        DiffValue::I32(x) => Val::I32(*x),
        DiffValue::I64(x) => Val::I64(*x),
        DiffValue::F32(bits) => Val::F32(*bits),
        DiffValue::F64(bits) => Val::F64(*bits),
        DiffValue::FuncRef { null } => {
            if *null {
                Val::FuncRef(None)
            } else {
                // Use a deterministic in-module function reference for non-null funcref args.
                Val::FuncRef(Some(fallback_funcref?.clone()))
            }
        }
        DiffValue::ExternRef { null } => {
            if *null {
                Val::ExternRef(None)
            } else {
                // Deterministic non-null externref payload.
                let r = WasmtimeExternRef::new(store, 1u32).ok()?;
                Val::ExternRef(Some(r))
            }
        }
    })
}

fn wasmtime_val_to_diff_value(v: Val) -> Option<DiffValue> {
    Some(match v {
        Val::I32(x) => DiffValue::I32(x),
        Val::I64(x) => DiffValue::I64(x),
        Val::F32(bits) => DiffValue::F32(bits),
        Val::F64(bits) => DiffValue::F64(bits),
        Val::FuncRef(r) => DiffValue::FuncRef { null: r.is_none() },
        Val::ExternRef(r) => DiffValue::ExternRef { null: r.is_none() },
        _ => return None,
    })
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum GlobalKind {
    I32,
    I64,
    F32,
    F64,
    FuncRef,
    ExternRef,
}

#[derive(Debug, Default, Clone)]
struct ExportMap {
    // name -> func index
    exported_funcs: std::collections::BTreeMap<String, u32>,
    // name -> global index
    exported_globals: std::collections::BTreeMap<String, u32>,
    // name -> memory index
    exported_memories: std::collections::BTreeMap<String, u32>,
    // name -> table index
    exported_tables: std::collections::BTreeMap<String, u32>,
    // global index -> kind (only for core numeric globals we can snapshot)
    global_kinds: Vec<GlobalKind>,
}

fn parse_export_map(wasm: &[u8]) -> Result<ExportMap, ()> {
    let mut out = ExportMap::default();
    for payload in Parser::new(0).parse_all(wasm) {
        let payload = payload.map_err(|_| ())?;
        match payload {
            // IMPORTANT: Exported global indices include *imported* globals first.
            // If we only record kinds from `GlobalSection`, `lhs_snap.get_global` will fail to
            // resolve exported imported globals, and globals won't be compared.
            Payload::ImportSection(s) => {
                for imp in s {
                    let imp = imp.map_err(|_| ())?;
                    if let wasmparser::TypeRef::Global(g) = imp.ty {
                        let kind = match g.content_type {
                            wasmparser::ValType::I32 => Some(GlobalKind::I32),
                            wasmparser::ValType::I64 => Some(GlobalKind::I64),
                            wasmparser::ValType::F32 => Some(GlobalKind::F32),
                            wasmparser::ValType::F64 => Some(GlobalKind::F64),
                            wasmparser::ValType::FuncRef => Some(GlobalKind::FuncRef),
                            wasmparser::ValType::ExternRef => Some(GlobalKind::ExternRef),
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            out.global_kinds.push(kind);
                        }
                    }
                }
            }
            Payload::GlobalSection(s) => {
                for g in s {
                    let g = g.map_err(|_| ())?;
                    let kind = match g.ty.content_type {
                        wasmparser::ValType::I32 => Some(GlobalKind::I32),
                        wasmparser::ValType::I64 => Some(GlobalKind::I64),
                        wasmparser::ValType::F32 => Some(GlobalKind::F32),
                        wasmparser::ValType::F64 => Some(GlobalKind::F64),
                        wasmparser::ValType::FuncRef => Some(GlobalKind::FuncRef),
                        wasmparser::ValType::ExternRef => Some(GlobalKind::ExternRef),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        out.global_kinds.push(kind);
                    }
                }
            }
            Payload::ExportSection(s) => {
                for e in s {
                    let e = e.map_err(|_| ())?;
                    match e.kind {
                        wasmparser::ExternalKind::Func => {
                            out.exported_funcs.insert(e.name.to_string(), e.index);
                        }
                        wasmparser::ExternalKind::Global => {
                            out.exported_globals.insert(e.name.to_string(), e.index);
                        }
                        wasmparser::ExternalKind::Memory => {
                            out.exported_memories.insert(e.name.to_string(), e.index);
                        }
                        wasmparser::ExternalKind::Table => {
                            out.exported_tables.insert(e.name.to_string(), e.index);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn diff_value_to_rwasm(v: &DiffValue, fallback_funcref_idx: Option<u32>) -> Option<Value> {
    Some(match v {
        DiffValue::I32(x) => Value::I32(*x),
        DiffValue::I64(x) => Value::I64(*x),
        DiffValue::F32(bits) => Value::F32(rwasm::F32::from_bits(*bits)),
        DiffValue::F64(bits) => Value::F64(rwasm::F64::from_bits(*bits)),
        DiffValue::FuncRef { null } => {
            if *null {
                Value::FuncRef(FuncRef::null())
            } else {
                // Use deterministic exported callee index when available.
                Value::FuncRef(FuncRef::new(fallback_funcref_idx?))
            }
        }
        DiffValue::ExternRef { null } => {
            if *null {
                Value::ExternRef(ExternRef::null())
            } else {
                Value::ExternRef(ExternRef::new(1u32))
            }
        }
    })
}

fn zero_rwasm_from_diff_type(t: &DiffValueType) -> Value {
    match t {
        DiffValueType::I32 => Value::I32(0),
        DiffValueType::I64 => Value::I64(0),
        DiffValueType::F32 => Value::F32(rwasm::F32::from_bits(0)),
        DiffValueType::F64 => Value::F64(rwasm::F64::from_bits(0)),
        DiffValueType::FuncRef => Value::FuncRef(FuncRef::null()),
        DiffValueType::ExternRef => Value::ExternRef(ExternRef::null()),
    }
}

fn rwasm_value_to_diff(v: &Value, t: &DiffValueType) -> Option<DiffValue> {
    Some(match (t, v) {
        (DiffValueType::I32, Value::I32(x)) => DiffValue::I32(*x),
        (DiffValueType::I64, Value::I64(x)) => DiffValue::I64(*x),
        (DiffValueType::F32, Value::F32(x)) => DiffValue::F32(x.to_bits()),
        (DiffValueType::F64, Value::F64(x)) => DiffValue::F64(x.to_bits()),
        (DiffValueType::FuncRef, Value::FuncRef(x)) => DiffValue::FuncRef { null: x.is_null() },
        (DiffValueType::ExternRef, Value::ExternRef(x)) => {
            DiffValue::ExternRef { null: x.is_null() }
        }
        _ => return None,
    })
}

#[derive(Default)]
struct RuntimeStats {
    attempts: AtomicUsize,
    total_invocations: AtomicUsize,
    successes: AtomicUsize,
    unsupported_modules: AtomicUsize,
    wasm_smith_modules: AtomicUsize,
    single_instruction_modules: AtomicUsize,
}

impl RuntimeStats {
    const fn new() -> RuntimeStats {
        RuntimeStats {
            attempts: AtomicUsize::new(0),
            total_invocations: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
            unsupported_modules: AtomicUsize::new(0),
            wasm_smith_modules: AtomicUsize::new(0),
            single_instruction_modules: AtomicUsize::new(0),
        }
    }

    fn bump_attempts(&self) {
        let attempts = self.attempts.fetch_add(1, SeqCst);
        if attempts == 0 || attempts % 1_000 != 0 {
            return;
        }
        let successes = self.successes.load(SeqCst);
        let unsupported = self.unsupported_modules.load(SeqCst);
        println!(
            "=== Execution rate ({} successes / {} attempted modules): {:.02}% ===",
            successes,
            attempts,
            successes as f64 / attempts as f64 * 100f64,
        );
        println!("\tunsupported-by-rwasm modules skipped: {unsupported}");
        let wasm_smith = self.wasm_smith_modules.load(SeqCst);
        let single_inst = self.single_instruction_modules.load(SeqCst);
        let total = wasm_smith + single_inst;
        if total > 0 {
            println!(
                "\twasm-smith: {:.02}%, single-inst: {:.02}%",
                wasm_smith as f64 / total as f64 * 100f64,
                single_inst as f64 / total as f64 * 100f64,
            );
        }
    }
}
