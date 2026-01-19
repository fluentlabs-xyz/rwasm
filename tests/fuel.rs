use rwasm::{
    always_failing_syscall_handler, CompilationConfig, ExecutionEngine, FuelConfig, FuelCosts,
    ImportLinker, RwasmModule, RwasmStore, Store, Value,
};
use std::time::Instant;

#[test]
fn test_locals_consume_fuel() {
    let fuel_limit = 9999;
    let basic_fuel_consumption = 2;

    let mut test_cases: &mut [(usize, usize)] = &mut [
        (16, 0),
        (32, 0),
        (1000, 0),
        // (1001, 0), //function params size is out of bounds (RwasmModule::compile fails)
    ];
    test_cases
        .iter_mut()
        .for_each(|(p_count, fuel)| *fuel = FuelCosts::fuel_for_locals(*p_count as u32) as usize);
    for (locals_count, fuel_cost) in test_cases.iter().cloned() {
        let mut wat_params: Vec<&str> = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            wat_params.push("i32")
        }
        let wat_params_str = wat_params.join(" ");
        let wat_str = format!(
            r#"
            (module
              (func (export "custom") (param {wat_params_str}) (result i32)
                (i32.const 111)
              )
            )
        "#
        );
        let wasm_binary = wat::parse_str(wat_str).unwrap();
        let config = CompilationConfig::default()
            .with_entrypoint_name("custom".into())
            .with_allow_malformed_entrypoint_func_type(true)
            .with_consume_fuel(true);
        let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
        println!("{}", rwasm_module);
        let mut store = RwasmStore::<()>::default();
        store.set_fuel(Some(fuel_limit));
        let engine = ExecutionEngine::new();
        let mut result = [Value::I32(0); 1];
        let mut params_values = Vec::with_capacity(locals_count);
        for _ in 0..locals_count {
            params_values.push(Value::I32(0));
        }
        engine
            .execute(&mut store, &rwasm_module, &params_values, &mut result)
            .unwrap();
        let remaining_fuel = store.remaining_fuel();
        assert_eq!(
            Some(fuel_limit - basic_fuel_consumption - fuel_cost as u64),
            remaining_fuel
        );
        assert_eq!(result[0].i32().unwrap(), 111);
    }
}

/// Empty function: (module (func (export "main")))
const EMPTY_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: () -> ()
    0x03, 0x02, 0x01, 0x00, // function section: 1 func, type 0
    0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // export "main"
    0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b, // code section: body_size=2, 0 locals, end
];

/// 4096 i64 locals (4096 = 0x80 0x20 LEB128) - max before StackOverflow
/// Body: 1 local decl (1) + count leb128 (2) + type (1) + end (1) = 5 bytes
/// Section: func_count (1) + body_size (1) + body (5) = 7 bytes
const LOCALS_4096_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: () -> ()
    0x03, 0x02, 0x01, 0x00, // function section: 1 func, type 0
    0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // export "main"
    0x0a, 0x07, 0x01, 0x05, 0x01, 0x80, 0x20, 0x7e, 0x0b, // code: 4096 i64 locals
];

/// 32767 i64 locals (32767 = 0xFF 0xFF 0x01 LEB128) - max before DropKeepOutOfBounds
/// Body: 1 local decl (1) + count leb128 (3) + type (1) + end (1) = 6 bytes
/// Section: func_count (1) + body_size (1) + body (6) = 8 bytes
const LOCALS_32767_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: () -> ()
    0x03, 0x02, 0x01, 0x00, // function section: 1 func, type 0
    0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // export "main"
    0x0a, 0x08, 0x01, 0x06, 0x01, 0xff, 0xff, 0x01, 0x7e, 0x0b, // code: 32767 i64 locals
];

fn compile(wasm: &[u8]) -> RwasmModule {
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_consume_fuel(true);
    RwasmModule::compile(config, wasm).expect("compile").0
}

fn execute(module: &RwasmModule) -> u64 {
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        FuelConfig::default(),
    );
    let _ = engine.execute(&mut store, module, &[], &mut []);
    store.fuel_consumed()
}

fn benchmark(module: &RwasmModule, iterations: u32) -> std::time::Duration {
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::<()>::default();

    // Warmup
    for _ in 0..10 {
        let _ = engine.execute(&mut store, module, &[], &mut []);
    }

    // Timed runs
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = engine.execute(&mut store, module, &[], &mut []);
    }
    start.elapsed()
}

#[test]
fn test_locals_fuel_and_runtime() {
    const ITERATIONS: u32 = 1000;

    let empty = compile(EMPTY_WASM);
    let loc4k = compile(LOCALS_4096_WASM);
    let loc32k = compile(LOCALS_32767_WASM);

    let empty_fuel = execute(&empty);
    let loc4k_fuel = execute(&loc4k);
    let loc32k_fuel = execute(&loc32k);

    let empty_time = benchmark(&empty, ITERATIONS) / ITERATIONS;
    let loc4k_time = benchmark(&loc4k, ITERATIONS) / ITERATIONS;
    let loc32k_time = benchmark(&loc32k, ITERATIONS) / ITERATIONS;

    eprintln!("\n=== Locals Fuel & Runtime ===");
    eprintln!("0 locals:     fuel={}, time={:?}", empty_fuel, empty_time);
    eprintln!("4096 locals:  fuel={}, time={:?}", loc4k_fuel, loc4k_time);
    eprintln!("32767 locals: fuel={}, time={:?}", loc32k_fuel, loc32k_time);

    eprintln!(
        "Slowdown: {:.0}x",
        loc4k_time.as_nanos() as f64 / empty_time.as_nanos() as f64
    );
}

// #[test]
// fn test_locals_consume_fuel_custom_wasm() {
//     let fuel_limit = 999;
//     let basic_fuel_consumption = 2;
//
//     let test_cases: &[(Vec<u8>, usize, usize)] = &[
//         (
//             EMPTY_WASM.to_vec(),
//             0,
//             FuelCosts::fuel_for_locals(0) as usize,
//         ),
//         // (
//         //     LOCALS_4096_WASM.to_vec(),
//         //     4096,
//         //     FuelCosts::fuel_for_locals(4096) as usize,
//         // ),
//         // (
//         //     LOCALS_32767_WASM.to_vec(),
//         //     32767,
//         //     FuelCosts::fuel_for_locals(32767) as usize,
//         // ),
//     ];
//     for (wasm_binary, locals_count, fuel_cost_for_locals) in test_cases.iter().cloned() {
//         let mut wat_params: Vec<&str> = Vec::with_capacity(locals_count);
//         for _ in 0..locals_count {
//             wat_params.push("i32")
//         }
//         let config = CompilationConfig::default()
//             .with_entrypoint_name("main".into())
//             .with_allow_malformed_entrypoint_func_type(true)
//             .with_consume_fuel(true);
//         let (rwasm_module, _) = RwasmModule::compile(config, &wasm_binary).unwrap();
//         println!("{}", rwasm_module);
//         let mut store = RwasmStore::<()>::default();
//         store.set_fuel(Some(fuel_limit));
//         let engine = ExecutionEngine::new();
//         let mut result = [Value::I32(0); 1];
//         let mut params_values = Vec::with_capacity(locals_count);
//         for _ in 0..locals_count {
//             params_values.push(Value::I32(0));
//         }
//         engine
//             .execute(&mut store, &rwasm_module, &params_values, &mut result)
//             .unwrap();
//         let remaining_fuel = store.remaining_fuel();
//         assert_eq!(
//             Some(fuel_limit - basic_fuel_consumption - fuel_cost_for_locals as u64),
//             remaining_fuel
//         );
//         assert_eq!(result[0].i32().unwrap(), 111);
//     }
// }
