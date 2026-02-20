#![no_main]

//! Differential fuzz target inspired by Wasmtime's `fuzz_targets/differential.rs`.
//!
//! Key changes from Wasmtime's version:
//! - Only compare **rwasm vs wasmtime**
//! - No `ALLOWED_*` env vars (always choose between wasm-smith and single-inst)
//! - Module generation is either **wasm-smith** or **single-inst**
//!
//! Reference: `https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/fuzz/fuzz_targets/differential.rs`

use anyhow::Context;
use libfuzzer_sys::{
    arbitrary::{self, Result, Unstructured},
    fuzz_target,
};
use rwasm::{
    CompilationConfig, ExecutionEngine, ExternRef, FuncRef, RwasmModule, RwasmStore, StoreTr,
    TrapCode, Value,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering::SeqCst},
    Once,
};
use wasmparser::{Parser, Payload};
use wasmtime::{
    Engine, Extern, ExternRef as WasmtimeExternRef, FuncType, Instance, Module, Ref, Store, Val,
};
use wasmtime_fuzzing::{
    generators::{CompilerStrategy, Config, DiffValue, DiffValueType, SingleInstModule},
    oracles::{dummy, log_wasm, StoreLimits},
};

/// Upper limit on the number of invocations for each WebAssembly function.
const NUM_INVOCATIONS: usize = 5;
const MAX_EXPORTS: usize = 8;

/// How many table elements we snapshot/compare (nullness only) for each exported table.
///
/// Keeping this bounded prevents pathological slowdown when tables grow very large.
const TABLE_NULLNESS_PREFIX_ELEMS: usize = 256;

/// Only run once when the fuzz target loads.
static SETUP: Once = Once::new();

/// Statistics about what's actually getting executed during fuzzing.
static STATS: RuntimeStats = RuntimeStats::new();

fuzz_target!(|data: &[u8]| {
    SETUP.call_once(|| {
        // Mirrors Wasmtime's harness: initialize fuzzing infrastructure once.
        wasmtime_fuzzing::init_fuzzing();
        let _ = env_logger::try_init();
    });

    // Errors in `execute_one` are typically "not enough bytes" from `Unstructured`;
    // ignore them for fuzzing throughput.
    let _ = execute_one(data);
});

fn execute_one(data: &[u8]) -> Result<()> {
    let mut u = Unstructured::new(data);

    STATS.bump_attempts();

    // Generate a Wasmtime fuzzing configuration suitable for differential execution.
    let mut config: Config = u.arbitrary()?;

    // rwasm doesn't support the component model proposals.
    config.module_config.component_model_async = false;
    config.module_config.component_model_async_builtins = false;
    config.module_config.component_model_async_stackful = false;
    config.module_config.component_model_error_context = false;

    // Disable additional Wasm proposals via the underlying wasm-smith config knobs that
    // `wasmtime-fuzzing` will mirror into `wasmtime::Config`.
    //
    // Keep bulk-memory + multi-value + others on (they're core-ish and rwasm supports them),
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
    }
    // Use Cranelift to support tail-call
    config.wasmtime.compiler_strategy = CompilerStrategy::CraneliftNative;

    config.set_differential_config();

    // Build a module either via wasm-smith or as a single-instruction module.
    let build_wasm_smith_module = |u: &mut Unstructured, config: &Config| -> Result<Vec<u8>> {
        STATS.wasm_smith_modules.fetch_add(1, SeqCst);
        // `wasmtime-fuzzing` / `wasm-smith` generation is not supposed to panic, but in practice it
        // occasionally does (e.g. internal unwraps in generator code). Treat those as "skip input"
        // so the fuzzer reports only real engine bugs/diffs.
        let module = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            config.generate(u, Some(10000))
        })) {
            Ok(res) => res?,
            Err(_) => return Err(arbitrary::Error::IncorrectFormat),
        };
        Ok(module.to_bytes())
    };
    let build_single_inst_module = |u: &mut Unstructured, config: &Config| -> Result<Vec<u8>> {
        STATS.single_instruction_modules.fetch_add(1, SeqCst);
        let module = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            SingleInstModule::new(u, &config.module_config)
        })) {
            Ok(res) => res?,
            Err(_) => return Err(arbitrary::Error::IncorrectFormat),
        };
        Ok(module.to_bytes())
    };

    let wasm = match u.int_in_range::<u8>(0..=1)? {
        0 => build_wasm_smith_module(&mut u, &config)?,
        _ => build_single_inst_module(&mut u, &config)?,
    };

    log_wasm(&wasm);

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
        let rhs_store = create_wasmtime_store(&config);
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
                    DiffValue::arbitrary_of_type(&mut u, ty)
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
            let rhs_store = create_wasmtime_store(&config);
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
        Ok(Some((results, snap))) => Ok((results, snap)),
        Ok(None) => return Ok(true), // rwasm can't compile supported subset -> skip
        Err(trap) => Err(trap),
    };
    log::debug!(
        " -> lhs results on rwasm: {:?}",
        lhs_results.as_ref().map(|(r, _)| r)
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
        (Ok((lhs_vals, lhs_snap)), Ok(Some(rhs_vals))) => {
            if lhs_vals != rhs_vals {
                panic!(
                    "diff results: export={name} args={args:?} result_tys={result_tys:?}\n\
                     rwasm={lhs_vals:?}\n\
                     wasmtime={rhs_vals:?}\n"
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
                let lhs_tbl = lhs_snap
                    .get_table_nullness_prefix(table, export_map)
                    .unwrap_or_else(|| {
                        panic!(
                            "state-compare skipped table: export={name} table={table} \
                         (lhs snapshot could not resolve it)"
                        )
                    });
                let rhs_tbl =
                    wasmtime_table_nullness_prefix(rhs, table, TABLE_NULLNESS_PREFIX_ELEMS)
                        .unwrap_or_else(|| {
                            panic!(
                                "state-compare skipped table: export={name} table={table} \
                             (rhs snapshot could not resolve/unsupported table ref type)"
                            )
                        });
                assert_eq!(lhs_tbl, rhs_tbl);
            }

            Ok(true)
        }
        // LHS trap, RHS trap: considered equivalent (coarser than Wasmtime's Trap equality).
        (Err(_), Err(_)) => Ok(true),

        // LHS trap, RHS ok: mismatch.
        (Err(lhs_trap), Ok(_)) => {
            panic!("diff: export={name} wasmtime=Ok rwasm_trap={lhs_trap:?} args={args:?} result_tys={result_tys:?}\n")
        }

        // LHS ok, RHS trap: mismatch.
        (Ok(_), Err(rhs_err)) => {
            panic!("diff: export={name} wasmtime_trap={rhs_err:?} rwasm=Ok args={args:?} result_tys={result_tys:?}\n")
        }

        // Wasmtime-side engines can return Ok(None) for unsupported signatures; for WasmtimeInstance
        // this shouldn't happen, but keep it for parity with the upstream oracle.
        (Ok(_), Ok(None)) => Ok(true),
    }
}

fn run_rwasm_one(
    wasm: &[u8],
    export: &str,
    args: &[DiffValue],
    results_t: &[DiffValueType],
    export_map: &ExportMap,
) -> Result<Option<(Vec<DiffValue>, RwasmSnapshot)>, TrapCode> {
    let config = CompilationConfig::default()
        .with_entrypoint_name(export.into())
        .with_allow_malformed_entrypoint_func_type(true)
        .with_consume_fuel(false);

    let (module, _) = match RwasmModule::compile(config, wasm) {
        Ok(x) => x,
        Err(e) => {
            // Treat this as a compilation differential (Wasmtime already compiled+instantiated,
            // otherwise we would have returned early before selecting exports).
            panic!(
                "compile-diff: export={export} rwasm_compilation_error={e:?} ({e}) args={args:?} result_tys={results_t:?}\n"
            );
        }
    };

    let engine = ExecutionEngine::default();
    let mut store = RwasmStore::<()>::default();
    store.reset_fuel(u64::MAX);

    let params: Vec<Value> = args
        .iter()
        .map(diff_value_to_rwasm)
        .collect::<Option<_>>()
        .ok_or(TrapCode::IllegalOpcode)?;
    let mut results: Vec<Value> = results_t.iter().map(zero_rwasm_from_diff_type).collect();

    engine.execute(&mut store, &module, &params, &mut results)?;

    let vals = results
        .iter()
        .zip(results_t.iter())
        .map(|(v, t)| rwasm_value_to_diff(v, t))
        .collect::<Option<Vec<_>>>()
        .ok_or(TrapCode::IllegalOpcode)?;

    let snap = RwasmSnapshot::new(export_map, &store);
    Ok(Some((vals, snap)))
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
fn create_wasmtime_store(config: &Config) -> Store<StoreLimits> {
    let mut wasmtime_config = config.to_wasmtime();
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
    let mut store = Store::new(&engine, StoreLimits::new());
    config.configure_store(&mut store);
    store
}

/// Creates a Wasmtime store using the default configuration.
#[cfg(not(any(target_os = "macos", feature = "disable-signals")))]
fn create_wasmtime_store(config: &Config) -> Store<StoreLimits> {
    config.to_store()
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
    store: Store<StoreLimits>,
    instance: Instance,
}

impl RawWasmtimeInstance {
    fn new(mut store: Store<StoreLimits>, module: Module) -> anyhow::Result<Self> {
        let instance = dummy::dummy_linker(&mut store, &module)
            .and_then(|l| l.instantiate(&mut store, &module))
            .context("unable to instantiate module in wasmtime (dummy linker)")?;
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
        // `StoreLimits::is_oom()` exists in `wasmtime-fuzzing` but is private.
        // We can't reliably detect OOM via the public API here, so conservatively
        // report "not OOM" and let mismatches surface normally.
        false
    }

    fn evaluate(
        &mut self,
        function_name: &str,
        arguments: &[DiffValue],
        _results: &[DiffValueType],
    ) -> anyhow::Result<Option<Vec<DiffValue>>> {
        let arguments: Vec<_> = arguments
            .iter()
            .map(|v| diff_value_to_wasmtime_val(&mut self.store, v))
            .collect::<Option<_>>()
            .ok_or_else(|| anyhow::anyhow!("unsupported argument type"))?;

        let function = self
            .instance
            .get_func(&mut self.store, function_name)
            .expect("unable to access exported function");
        let ty = function.ty(&self.store);
        let mut results = vec![Val::I32(0); ty.results().len()];
        function.call(&mut self.store, &arguments, &mut results)?;

        let results = results
            .into_iter()
            .map(wasmtime_val_to_diff_value)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| anyhow::anyhow!("unsupported result type"))?;

        Ok(Some(results))
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

fn diff_value_to_wasmtime_val(store: &mut Store<StoreLimits>, v: &DiffValue) -> Option<Val> {
    Some(match v {
        DiffValue::I32(x) => Val::I32(*x),
        DiffValue::I64(x) => Val::I64(*x),
        DiffValue::F32(bits) => Val::F32(*bits),
        DiffValue::F64(bits) => Val::F64(*bits),
        DiffValue::FuncRef { null } => {
            // Compare funcref values by nullness only.
            //
            // Creating a deterministic non-null funcref requires selecting a specific function
            // from the module instance, which this helper doesn't have access to.
            if *null {
                Val::FuncRef(None)
            } else {
                return None;
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
        _ => return None,
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

fn diff_value_to_rwasm(v: &DiffValue) -> Option<Value> {
    Some(match v {
        DiffValue::I32(x) => Value::I32(*x),
        DiffValue::I64(x) => Value::I64(*x),
        DiffValue::F32(bits) => Value::F32(rwasm::F32::from_bits(*bits)),
        DiffValue::F64(bits) => Value::F64(rwasm::F64::from_bits(*bits)),
        DiffValue::FuncRef { null } => {
            if *null {
                Value::FuncRef(FuncRef::null())
            } else {
                // Deterministic non-null placeholder. We only compare nullness across engines.
                Value::FuncRef(FuncRef::new(1u32))
            }
        }
        DiffValue::ExternRef { null } => {
            if *null {
                Value::ExternRef(ExternRef::null())
            } else {
                Value::ExternRef(ExternRef::new(1u32))
            }
        }
        _ => return None,
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
        _ => Value::I32(0),
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
    wasm_smith_modules: AtomicUsize,
    single_instruction_modules: AtomicUsize,
}

impl RuntimeStats {
    const fn new() -> RuntimeStats {
        RuntimeStats {
            attempts: AtomicUsize::new(0),
            total_invocations: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
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
        println!(
            "=== Execution rate ({} successes / {} attempted modules): {:.02}% ===",
            successes,
            attempts,
            successes as f64 / attempts as f64 * 100f64,
        );
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
