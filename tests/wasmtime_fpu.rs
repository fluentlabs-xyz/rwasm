use wasmtime::{Engine, Instance, Module, Store, TypedFunc};

pub fn run_main<Results>(wat: &str) -> anyhow::Result<Results>
where
    Results: wasmtime::WasmResults,
{
    let engine = Engine::default();
    let module = Module::new(&engine, wat)?;
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[])?;
    let run: TypedFunc<(), Results> = instance.get_typed_func(&mut store, "main")?;
    run.call(&mut store, ())
}

#[test]
fn test_wasmtime_disabled_f32_sqrt() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f32)
                f32.const 9.0
                f32.sqrt
            )
        )
    "#;
    let result = run_main::<f32>(wat);
    let trap = result
        .err()
        .expect("execution should fail")
        .downcast_ref::<wasmtime::Trap>()
        .expect("execution should fail with a trap")
        .clone();
    matches!(trap, wasmtime::Trap::DisabledOpcode);
    Ok(())
}

#[test]
fn test_wasmtime_disabled_f64_div() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f64)
                f64.const 9.0
                f64.const 3.0
                f64.div
            )
        )
    "#;
    let result = run_main::<f64>(wat);
    let trap = result
        .err()
        .expect("execution should fail")
        .downcast_ref::<wasmtime::Trap>()
        .expect("execution should fail with a trap")
        .clone();
    matches!(trap, wasmtime::Trap::DisabledOpcode);
    Ok(())
}

#[test]
fn test_wasmtime_f32_const() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f32)
                f32.const 9.0
            )
        )
    "#;
    let result = run_main::<f32>(wat);
    matches!(result, Ok(9.0));
    Ok(())
}

#[test]
fn test_wasmtime_f64_const() -> anyhow::Result<()> {
    let wat = r#"
        (module
            (func (export "main") (result f64)
                f64.const 9.0
            )
        )
    "#;
    let result = run_main::<f64>(wat);
    matches!(result, Ok(9.0));
    Ok(())
}
