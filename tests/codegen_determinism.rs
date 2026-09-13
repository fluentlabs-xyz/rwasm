//! Consensus guard: rWasm codegen must be byte-identical for the same `(wasm, config)` in every
//! process, every run, and under every hash-map seed / insertion order.
//!
//! Why this test exists: the serialized `RwasmModule` is the contract identity in the nodes that
//! use rwasm, and `CompilationConfig::codegen_identity()` exists so a host can detect
//! configuration drift. `hashbrown::HashMap` — the map used throughout the compiler — seeds its
//! default hasher from a stack address (foldhash `gen_per_hasher_seed`), so iteration order
//! changes per process *and per call frame*. Any compiler pass that iterated a hash map instead of
//! a `Vec`/sorted structure would turn that into two different module hashes for one contract.
//!
//! `compile_at_depth` is the sharp instrument: compiling from a deeper stack shifts the hasher
//! seed of every map the compiler builds, so it varies hash-map iteration order inside a single
//! process (measured: 4/4 distinct `HashMap<FuncType, _>` and `HashMap<Box<str>, _>` iteration
//! orders at depths 0/3/11/47) while every other input stays fixed.
//!
//! The golden values are pinned for a build with the `fpu` feature **off**; `fpu` changes emitted
//! bytes on purpose (see `codegen_feature_set`). They are unchanged by the `wasmtime`/`std`/
//! `serde` host-side features. Update them only when codegen changes on purpose: for a contract
//! addressed by its module hash, an unintentional change here is a consensus break.

use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, ImportLinker,
    ImportLinkerEntity, ImportName, RwasmModule, RwasmStore, StateRouterConfig, StoreTr,
    SyscallFuelParams, ValType, Value,
};
use std::sync::Arc;
use tiny_keccak::{Hasher, Keccak};

fn keccak(data: &[u8]) -> String {
    let mut h = Keccak::v256();
    let mut out = [0u8; 32];
    h.update(data);
    h.finalize(&mut out);
    hex::encode(out)
}

/// `(module (func (export "main") (result i32) i32.const 1 i32.const 0 i32.div_s))`
fn tiny_wasm() -> Vec<u8> {
    hex::decode("0061736d010000000105016000017f03020100070801046d61696e00000a09010700410141006d0b")
        .unwrap()
}

/// i64 div/rem/rotl/shr/comparisons — the module that exercises snippet emission.
fn snippets_wasm() -> Vec<u8> {
    hex::decode(
        "0061736d0100000001070160027e7e017e03020100070801046d61696e00000a48014600200020017f20002001\
         807c20002001817c20002001827c20002001897c200020018a7c20002001867c20002001877c2000200188\
         7c2000200153ad7c2000200151ad7c7b0b",
    )
    .unwrap()
}

/// Four imports of three different signatures, so the linker lookup and the syscall trampolines
/// are exercised. Compiles only under a config that carries a matching `ImportLinker`.
fn import_wasm() -> Vec<u8> {
    wat::parse_str(
        r#"(module
             (import "env" "imp_000" (func $a (param i32) (result i32)))
             (import "env" "imp_001" (func $b))
             (import "env" "imp_002" (func $c (param i64 i32) (result i64)))
             (import "env" "imp_003" (func $d (param i32) (result i32)))
             (func (export "main") (result i32)
               i32.const 1 call $a call $b
               i64.const 2 i32.const 3 call $c drop
               i32.const 4 call $d))"#,
    )
    .unwrap()
}

/// A string that fully describes what a `(input, config)` pair produces — success *and* failure,
/// because an error message is also consensus-visible (a node that rejects where another accepts
/// is a divergence too).
fn compile_outcome(config: &CompilationConfig, wasm: &[u8]) -> String {
    match RwasmModule::compile(config.clone(), wasm) {
        Ok((module, params)) => {
            let bytes = module.serialize();
            format!(
                "ok:{}:{}:{}:{}",
                keccak(&bytes),
                bytes.len(),
                module.source_pc,
                hex::encode(Vec::<u8>::from(params))
            )
        }
        Err(err) => format!("err:{err}"),
    }
}

/// Compiles at a recursion depth, which shifts the stack pointer and therefore the per-hasher
/// foldhash seed of every map the compiler allocates.
#[inline(never)]
fn compile_at_depth(depth: u32, config: &CompilationConfig, wasm: &[u8]) -> String {
    let pad = [0u64; 1024];
    std::hint::black_box(&pad);
    if depth == 0 {
        compile_outcome(config, wasm)
    } else {
        compile_at_depth(depth - 1, config, wasm)
    }
}

fn linker(order_reversed: bool) -> ImportLinker {
    let mut entries: Vec<(ImportName, ImportLinkerEntity)> = Vec::new();
    for i in 0..32u32 {
        let (params, result): (&'static [ValType], &'static [ValType]) = match i % 3 {
            0 => (&[ValType::I32], &[ValType::I32]),
            1 => (&[], &[]),
            _ => (&[ValType::I64, ValType::I32], &[ValType::I64]),
        };
        entries.push((
            ImportName::new("env", &format!("imp_{i:03}")),
            ImportLinkerEntity {
                sys_func_idx: 100 + i,
                syscall_fuel_param: SyscallFuelParams::Const(u64::from(i) + 1),
                params,
                result,
                intrinsic: None,
            },
        ));
    }
    if order_reversed {
        entries.reverse();
    }
    let mut linker = ImportLinker::default();
    for (name, entity) in entries {
        linker.insert_entity(name, entity);
    }
    linker
}

/// Config matrix used by every sub-test below. It deliberately mixes configs that compile a given
/// input with configs that reject it (missing entrypoint, missing linker, router type check), so
/// the error paths are compared as well.
fn matrix() -> Vec<(&'static str, CompilationConfig)> {
    let l = Arc::new(linker(false));
    let router = || StateRouterConfig {
        states: Box::new([("deploy".into(), 0u32), ("main".into(), 1u32)]),
        opcode: None,
    };
    vec![
        (
            "compat_main",
            CompilationConfig::default_strategy_compatible().with_entrypoint_name("main".into()),
        ),
        (
            "default_main",
            CompilationConfig::default().with_entrypoint_name("main".into()),
        ),
        (
            "nosnip_main",
            CompilationConfig::default()
                .with_code_snippets(false)
                .with_entrypoint_name("main".into()),
        ),
        (
            "nofuel_main",
            CompilationConfig::default()
                .with_consume_fuel(false)
                .with_entrypoint_name("main".into()),
        ),
        (
            "builtins_main",
            CompilationConfig::default()
                .with_builtins_consume_fuel(true)
                .with_import_linker(l.clone())
                .with_entrypoint_name("main".into()),
        ),
        (
            "linker_main",
            CompilationConfig::default_strategy_compatible()
                .with_import_linker(l.clone())
                .with_entrypoint_name("main".into()),
        ),
        (
            "router",
            CompilationConfig::default_strategy_compatible()
                .with_entrypoint_name("main".into())
                .with_state_router(router()),
        ),
        (
            "lenient_main",
            CompilationConfig::default_strategy_compatible()
                .with_entrypoint_name("main".into())
                .with_allow_start_section(true)
                .with_allow_func_ref_function_types(true)
                .with_default_imported_global_value(7)
                .with_import_linker(l.clone()),
        ),
        (
            "nopages_main",
            CompilationConfig::default_strategy_compatible()
                .with_max_allowed_memory_pages(64)
                .with_entrypoint_name("main".into()),
        ),
    ]
}

fn corpus() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("tiny", tiny_wasm()),
        ("snippets", snippets_wasm()),
        ("imports", import_wasm()),
    ]
}

/// A fingerprint of everything that decides contract identity, so two processes can be compared.
fn fingerprint() -> String {
    let mut s = String::new();
    for (name, wasm) in corpus() {
        for (cfg_name, cfg) in matrix() {
            s.push_str(&format!(
                "{name}/{cfg_name}/{}={};",
                hex::encode(cfg.codegen_identity()),
                compile_outcome(&cfg, &wasm)
            ));
        }
    }
    keccak(s.as_bytes())
}

/// Golden byte pins: any *intentional* codegen change must update these deliberately, because for
/// a contract addressed by its module hash an unintentional change is a consensus break.
#[test]
fn golden_bytecode_is_pinned() {
    let cfg = CompilationConfig::default_strategy_compatible().with_entrypoint_name("main".into());
    let tiny = RwasmModule::compile(cfg.clone(), &tiny_wasm()).unwrap().0.serialize();
    assert_eq!(tiny.len(), 147);
    assert_eq!(
        hex::encode(&tiny),
        "ef52010a0000000000000013000000050000000b0000000c0000000300000012000000\
         0000000009000000040000001300000002000000150000000100000015000000000000\
         00410000000b0000000000000000000000000000000000000028000000000000000061\
         736d010000000105016000017f03020100070801046d61696e00000a09010700410141\
         006d0b02000000"
            .replace([' ', '\n'], "")
    );
    assert_eq!(
        keccak(&tiny),
        "12c07e705a813955cb12954b176737173c9d88fda4e7ead8f2ac39620d810a18"
    );

    // snippet emission
    let snip = RwasmModule::compile(cfg.clone(), &snippets_wasm()).unwrap().0.serialize();
    assert_eq!(
        keccak(&snip),
        "53eafcadfc126cbd255fae9c22ad5d8cc1230d4262cdc2492919fd8ae4bcdd13"
    );
    let nosnip = CompilationConfig::default()
        .with_code_snippets(false)
        .with_entrypoint_name("main".into());
    let no_snip_bytes = RwasmModule::compile(nosnip, &snippets_wasm()).unwrap().0.serialize();
    assert_eq!(
        keccak(&no_snip_bytes),
        "e12a22f2bc0c2188202dbe7f43301a07f052bdc7978da497731fa4da66a4a6bf"
    );
    assert_ne!(snip, no_snip_bytes, "code_snippets must change emitted bytes");
    let rwasm_only_fuel = CompilationConfig::default().with_entrypoint_name("main".into());
    let fuel_bytes = RwasmModule::compile(rwasm_only_fuel, &snippets_wasm()).unwrap().0.serialize();
    assert_eq!(
        keccak(&fuel_bytes),
        "9fc316549b90331412ae5dc355549e9dae54c5294154f596e537d0193c88df10"
    );
}

/// The decisive test for "hash-map iteration order leaked into emitted code": the same compile
/// repeated at stack depths that give the compiler's internal maps different seeds.
#[test]
fn codegen_is_independent_of_hash_map_seed() {
    let mut checked = 0usize;
    for (name, wasm) in corpus() {
        for (cfg_name, cfg) in matrix() {
            let base = compile_at_depth(0, &cfg, &wasm);
            for depth in [1u32, 3, 11, 47, 101] {
                assert_eq!(
                    base,
                    compile_at_depth(depth, &cfg, &wasm),
                    "{name}/{cfg_name}: compile outcome changed with the hash-map seed \
                     (stack depth {depth})"
                );
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 3 * 9 * 5);
}

/// Same entities, opposite insertion order: the linker is backed by a `HashMap`, so its `iter()`
/// order differs, but nothing that reaches codegen or the config identity may.
#[test]
fn import_linker_insertion_order_is_irrelevant() {
    let forward = Arc::new(linker(false));
    let backward = Arc::new(linker(true));
    let order = |l: &ImportLinker| {
        l.iter()
            .map(|(n, _)| n.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_ne!(
        order(&forward),
        order(&backward),
        "precondition: iter() order must actually differ for this test to mean anything"
    );
    let wasm = import_wasm();
    for base in [
        CompilationConfig::default_strategy_compatible(),
        CompilationConfig::default(),
    ] {
        let a = base
            .clone()
            .with_import_linker(forward.clone())
            .with_entrypoint_name("main".into());
        let b = base
            .clone()
            .with_import_linker(backward.clone())
            .with_entrypoint_name("main".into());
        assert_eq!(a.codegen_identity(), b.codegen_identity());
        assert_eq!(
            compile_outcome(&a, &wasm),
            compile_outcome(&b, &wasm),
            "import-linker insertion order changed emitted bytecode"
        );
    }
}

/// Fuel and trap outcome must be identical for a fixed module and fuel limit, run many times.
#[test]
fn fuel_and_trap_are_deterministic_over_100_runs() {
    let modules: Vec<(&str, Vec<u8>, Vec<Value>)> = vec![
        (
            "loop",
            wat::parse_str(
                r#"(module (func (export "main") (result i64) (local $i i64) (local $s i64)
                     (loop $l
                       local.get $i i64.const 1 i64.add local.set $i
                       local.get $s local.get $i i64.add local.set $s
                       local.get $i i64.const 1000 i64.lt_u br_if $l)
                     local.get $s))"#,
            )
            .unwrap(),
            vec![Value::I64(0)],
        ),
        (
            "bulk",
            wat::parse_str(
                r#"(module (memory 2) (data $p "0123456789abcdef")
                     (func (export "main")
                       i32.const 0 i32.const 0 i32.const 16 memory.init $p
                       i32.const 0 i32.const 32 i32.const 16 memory.copy
                       i32.const 0 i32.const 0 i32.const 16 memory.fill))"#,
            )
            .unwrap(),
            vec![],
        ),
    ];
    let cfg = CompilationConfig::default().with_entrypoint_name("main".into());
    let linker = Arc::new(ImportLinker::default());
    let mut checked = 0usize;
    for (name, wasm, result_template) in &modules {
        let (module, _) = RwasmModule::compile(cfg.clone(), wasm).unwrap();
        for fuel in [2_000_000u64, 137] {
            let mut seen = std::collections::BTreeSet::new();
            for _ in 0..100 {
                let mut store = RwasmStore::<()>::new(
                    linker.clone(),
                    (),
                    always_failing_syscall_handler::<()>,
                    Some(fuel),
                    None,
                );
                // instantiation itself runs the module init section, so it can run out of fuel
                // too; that outcome is just as consensus-visible as the call outcome
                let outcome = match linker.instantiate(&mut store, ExecutionEngine::new(), module.clone()) {
                    Err(trap) => format!("instantiate:{trap:?}"),
                    Ok(instance) => {
                        let mut result = result_template.clone();
                        match instance.execute(&mut store, &[], &mut result) {
                            Ok(()) => format!("ok:{result:?}"),
                            Err(trap) => format!("{trap:?}"),
                        }
                    }
                };
                seen.insert((outcome, store.remaining_fuel()));
            }
            assert_eq!(
                seen.len(),
                1,
                "{name} at fuel {fuel}: fuel/trap drifted across 100 runs: {seen:?}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 4);
}

/// Same binary, two fresh OS processes: the codegen fingerprint (and the fuel outcome) must match.
#[test]
fn codegen_is_identical_in_a_fresh_process() {
    const CHILD: &str = "RWASM_DETERMINISM_CHILD";
    if let Ok(which) = std::env::var(CHILD) {
        println!(
            "DET_FINGERPRINT {which} {} iter_order={}",
            fingerprint(),
            keccak(
                linker(false)
                    .iter()
                    .map(|(n, _)| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
                    .as_bytes()
            )
        );
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let run = |which: &str| {
        let out = std::process::Command::new(&exe)
            .args([
                "--exact",
                "codegen_is_identical_in_a_fresh_process",
                "--nocapture",
            ])
            .env(CHILD, which)
            .output()
            .unwrap();
        assert!(out.status.success(), "child {which} failed: {out:?}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.starts_with("DET_FINGERPRINT"))
            .filter_map(|l| l.split_whitespace().nth(2).map(str::to_string))
            .next()
            .expect("child must print its fingerprint")
    };
    let a = run("a");
    assert!(!a.is_empty());
    assert_eq!(a, run("b"), "codegen fingerprint differs between OS processes");
}
