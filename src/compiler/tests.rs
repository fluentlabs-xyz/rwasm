use crate::{compiler::parser::ModuleParser, CompilationConfig, ExecutorConfig, RwasmExecutor};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let mut parser = ModuleParser::new(config);
    parser.parse(wasm_binary).unwrap();
    let rwasm_module = parser.finalize().unwrap();
    println!("{}", rwasm_module);
    let mut vm = RwasmExecutor::new(rwasm_module.into(), ExecutorConfig::new(), ());
    vm.caller().stack_push(43);
    let exit_code = vm.run().unwrap();
    assert_eq!(exit_code, 0);
    let result = vm.caller().stack_pop();
    assert_eq!(result.as_i64(), 433494437);
}
