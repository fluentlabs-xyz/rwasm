use crate::{compiler::parser::ModuleParser, CompilationConfig, ExecutorConfig, RwasmExecutor};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let mut parser = ModuleParser::new(config);
    parser.parse(wasm_binary).unwrap();
    let (rwasm_module, _) = parser.finalize().unwrap();
    println!("{}", rwasm_module);
    let mut vm = RwasmExecutor::new(
        rwasm_module.into(),
        ExecutorConfig::new().fuel_enabled(true),
        (),
    );
    vm.caller().stack_push(43);
    vm.run().unwrap();
    let result = vm.caller().stack_pop();
    assert_eq!(result.as_i64(), 433494437);
    println!("fuel_consumed: {}", vm.fuel_consumed());
    assert_eq!(vm.fuel_consumed(), 1253);
}

#[test]
#[ignore]
fn test_block() {
    let wasm_binary = wat::parse_str(
        r#"
(module
    (func (export "i32.trunc_sat_f64_s") (param $x f64) (result i32) (i32.trunc_sat_f64_s (local.get $x)))
)"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("i32.trunc_sat_f64_s".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let mut parser = ModuleParser::new(config);
    parser.parse(&wasm_binary).unwrap();
    let (rwasm_module, _) = parser.finalize().unwrap();
    println!("{}", rwasm_module);
    let mut vm = RwasmExecutor::new(rwasm_module.into(), ExecutorConfig::new(), ());
    println!("\nfunc:");
    vm.run().unwrap();
}
