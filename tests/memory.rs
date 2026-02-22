use rwasm::{
    always_failing_syscall_handler, instruction_set, ExecutionEngine, ImportLinker, RwasmModule,
    RwasmModuleBuilder, RwasmStore,
};

fn execute_module(module: &RwasmModule) -> u64 {
    let engine = ExecutionEngine::new();
    let mut store = RwasmStore::new(
        ImportLinker::default().into(),
        (),
        always_failing_syscall_handler,
        None,
        None,
    );
    engine.execute(&mut store, &module, &[], &mut []).unwrap();
    store.fuel_consumed()
}

#[test]
fn test_memory_fuel_ddos_not_possible() {
    let code_section = instruction_set! {
         // memory.grow
        I32Const(1)
        MemoryGrow
        Drop
        // memory.init
        I32Const(0) // d
        I32Const(0) // s
        I32Const(3) // n
        .op_memory_init_checked(None, None, 1u32, true) // 1 fuel cost
        // memory.fill
        I32Const(0) // d
        I32Const(0xff) // val
        I32Const(3) // n
        .op_memory_fill_checked(true) // 1 fuel cost
        // memory.copy
        I32Const(0) // d
        I32Const(0xff) // s
        I32Const(3) // n
        .op_memory_copy_checked(true) // 1 fuel cost
        // always terminate
        Return
    };
    let rwasm_module = RwasmModuleBuilder::new(code_section)
        .with_data_section(&[0x01, 0x02, 0x03])
        .build();
    println!("{}", rwasm_module);
    let fuel_consumed = execute_module(&rwasm_module);
    assert_eq!(fuel_consumed, 3);
}
