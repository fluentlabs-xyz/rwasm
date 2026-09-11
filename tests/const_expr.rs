//! `extended-const` lets a constant expression be an arbitrarily long operator chain. The compiler
//! must translate such globals and segment offsets without recursing on the operator count, since
//! compilation runs before any fuel is charged and a native stack overflow aborts the process.

use rwasm::{CompilationConfig, ExecutionEngine, ImportLinker, RwasmModule, RwasmStore, Value};

/// A constant expression `i32.const 0` followed by `operators` times `i32.const 1; i32.add`.
fn add_chain(operators: usize) -> String {
    let mut expr = String::from("i32.const 0");
    for _ in 0..operators {
        expr.push_str(" i32.const 1 i32.add");
    }
    expr
}

fn run_main_i32(wat: &str) -> i32 {
    let wasm = wat::parse_str(wat).expect("valid WAT");
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (module, _) = RwasmModule::compile(config, &wasm).expect("module compiles");
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), module)
        .unwrap();
    let mut result = [Value::I32(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    result[0].i32().unwrap()
}

#[test]
fn global_with_a_long_extended_const_chain_compiles() {
    // tens of thousands of operators overflowed the compiler stack before; go well beyond that
    const OPERATORS: usize = 200_000;
    let wat = format!(
        r#"(module
            (global $g i32 {chain})
            (func (export "main") (result i32) global.get $g)
        )"#,
        chain = add_chain(OPERATORS)
    );
    assert_eq!(run_main_i32(&wat), OPERATORS as i32);
}

#[test]
fn segment_offsets_with_long_extended_const_chains_compile() {
    const OPERATORS: usize = 50_000;
    let wat = format!(
        r#"(module
            (memory 1)
            (table 2 funcref)
            (data (offset {chain}) "\2a")
            (elem (offset {elem_chain}) $one)
            (func $one (result i32) i32.const 1)
            (func (export "main") (result i32)
                i32.const {offset}
                i32.load8_u
                i32.const 1
                table.get 0
                ref.is_null
                i32.add
            )
        )"#,
        chain = add_chain(OPERATORS),
        elem_chain = add_chain(1),
        offset = OPERATORS,
    );
    // the data byte landed at `OPERATORS` and the element landed at index 1 (non-null)
    assert_eq!(run_main_i32(&wat), 42);
}
