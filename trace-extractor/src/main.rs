use rwasm::{CompilationConfig, ExecutionEngine, RwasmModule, RwasmStore, Value};

fn trace_steps(
    wasm_bytecode: &[u8],
    entrypoint: &'static str,
    input: &[Value],
    output: &mut [Value],
) -> usize {
    let config = CompilationConfig::default()
        .with_entrypoint_name(entrypoint.into())
        .with_consume_fuel(false);
    let (rwasm_module, _) = RwasmModule::compile(config, wasm_bytecode).unwrap();
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::<()>::default();
    engine
        .execute(&mut store, &rwasm_module, input, output)
        .unwrap();
    store.tracer.logs.len()
}

fn main() {
    // trace evm machine fib32
    let evm_steps = trace_steps(
        include_bytes!("../evm-machine/lib.wasm"),
        "main",
        &[],
        &mut [],
    );
    println!("evm (fib32) trace steps: {}", evm_steps);
    // trace rwasm fib32
    let mut result = [Value::I32(0)];
    let rwasm_fib32_steps = trace_steps(
        include_bytes!("../../benchmarks/lib.wasm"),
        "fib32",
        &[Value::I32(43)],
        &mut result,
    );
    println!(
        "rwasm (fib32) trace steps: {} ({}x)",
        rwasm_fib32_steps,
        evm_steps / rwasm_fib32_steps
    );
    // trace rwasm fib64
    let mut result = [Value::I64(0)];
    let rwasm_fib64_steps = trace_steps(
        include_bytes!("../../benchmarks/lib.wasm"),
        "fib64",
        &[Value::I64(90)],
        &mut result,
    );
    println!(
        "rwasm (fib64) trace steps: {} ({}x)",
        rwasm_fib64_steps,
        evm_steps / rwasm_fib64_steps
    );
    // trace rwasm fib256
    let rwasm_fib256_steps = trace_steps(
        include_bytes!("../../benchmarks/lib.wasm"),
        "fib256",
        &[Value::I32(0), Value::I64(90)],
        &mut [],
    );
    println!(
        "rwasm (fib256) trace steps: {} ({}x)",
        rwasm_fib256_steps,
        evm_steps / rwasm_fib256_steps
    );
}
