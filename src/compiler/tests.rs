use crate::{compiler::parser::ModuleParser, CompilationConfig, ExecutorConfig, RwasmExecutor};

#[test]
fn test_fib() {
    let wasm_binary = include_bytes!("../../benchmarks/lib.wasm");
    let config = CompilationConfig::default().with_entrypoint_name("main".into());
    let mut parser = ModuleParser::new(config);
    parser.parse(wasm_binary).unwrap();
    let (rwasm_module, _) = parser.finalize().unwrap();
    println!("{}", rwasm_module);
    let mut vm = RwasmExecutor::new(rwasm_module.into(), ExecutorConfig::new(), ());
    vm.caller().stack_push(43);
    let exit_code = vm.run().unwrap();
    assert_eq!(exit_code, 0);
    let result = vm.caller().stack_pop();
    assert_eq!(result.as_i64(), 433494437);
}

#[test]
fn test_block() {
    let wasm_binary = wat::parse_str(
        r#"
(module
  (global $r externref (ref.null extern))
  (func (export "get-r") (result externref) (global.get $r))
)"#,
    )
    .unwrap();
    let config = CompilationConfig::default()
        .with_entrypoint_name("get-r".into())
        // .with_state_router(StateRouterConfig {
        //     states: Box::new([("fac".into(), 1000)]),
        //     opcode: None,
        // })
        .with_allow_malformed_entrypoint_func_type(true);
    let mut parser = ModuleParser::new(config);
    parser.parse(&wasm_binary).unwrap();
    let (rwasm_module, _) = parser.finalize().unwrap();
    println!("{}", rwasm_module);
    let mut vm = RwasmExecutor::new(rwasm_module.into(), ExecutorConfig::new(), ());
    // {
    //     println!("entrypoint:");
    //     vm.caller().stack_push(-1);
    //     let exit_code = vm.run().unwrap();
    //     assert_eq!(exit_code, 0);
    //     vm.reset(Some(10));
    // }
    println!("\nfunc:");
    // vm.caller().stack_push(0); // hi
    // vm.caller().stack_push(0); // hi
    // vm.caller().stack_push(1); // hi
    // vm.caller().stack_push(1000);
    let exit_code = vm.run().unwrap();
    assert_eq!(exit_code, 0);
    // let _result = vm.caller().stack_pop();
    // let result = vm.caller().stack_pop();
    // assert_eq!(result.as_i64(), 433494437);
}
