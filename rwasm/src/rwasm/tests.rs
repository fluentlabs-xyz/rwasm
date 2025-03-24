use crate::{
    core::ImportLinker,
    engine::{bytecode::Instruction, RwasmConfig, StateRouterConfig},
    module::ImportName,
    rwasm::{BinaryFormat, RwasmModule},
    AsContextMut,
    Caller,
    Config,
    Engine,
    Func,
    Linker,
    Module,
    Store,
};

#[derive(Default, Debug, Clone)]
struct HostState {
    exit_code: i32,
    state: u32,
}

#[cfg(feature = "std")]
pub fn trace_bytecode(module: &Module, engine: &Engine) {
    let import_len = module.imports.len_funcs;
    for fn_index in 0..module.funcs.len() {
        if fn_index == module.funcs.len() - 1 {
            println!("# entrypoint {}", fn_index);
        } else if fn_index < import_len {
            println!("# imported func {}", fn_index);
        } else {
            println!("# func {}", fn_index);
        }
        let func_body = module.compiled_funcs.get(fn_index).unwrap();
        for instr in engine.instr_vec(*func_body) {
            println!("{:?}", instr);
        }
    }
    println!()
}

const SYS_HALT_CODE: u32 = 1010;
const SYS_STATE_CODE: u32 = 1011;

fn create_import_linker() -> ImportLinker {
    let mut import_linker = ImportLinker::default();
    import_linker.insert_function(ImportName::new("env", "_sys_halt"), SYS_HALT_CODE, 1);
    import_linker.insert_function(ImportName::new("env", "_sys_state"), SYS_STATE_CODE, 1);
    import_linker
}

fn execute_binary(wat: &str, host_state: HostState, config: Config) -> HostState {
    let wasm_binary = wat::parse_str(wat).unwrap();
    // compile rWASM module from WASM binary
    let rwasm_module = RwasmModule::compile_with_config(&wasm_binary, &config).unwrap();
    // lets encode/decode rWASM module
    let mut encoded_rwasm_module = Vec::new();
    rwasm_module
        .write_binary_to_vec(&mut encoded_rwasm_module)
        .unwrap();
    let rwasm_module = RwasmModule::read_from_slice(&encoded_rwasm_module).unwrap();
    // init engine and module
    let engine = Engine::new(&config);
    let module = rwasm_module.to_module(&engine);
    // trace bytecode for debug purposes
    trace_bytecode(&module, &engine);
    // execute translated rwasm
    let mut store = Store::new(&engine, host_state);
    store.add_fuel(1_000_000).unwrap();
    let mut linker = Linker::<HostState>::new(&engine);
    let sys_halt_func = Func::wrap::<_, _, _, false>(
        store.as_context_mut(),
        |mut caller: Caller<'_, HostState>, exit_code: i32| {
            caller.data_mut().exit_code = exit_code;
        },
    );
    let sys_state_func = Func::wrap::<_, _, _, false>(
        store.as_context_mut(),
        |mut caller: Caller<'_, HostState>| -> u32 { caller.data_mut().state },
    );
    engine.register_trampoline(store.inner.wrap_stored(SYS_HALT_CODE.into()), sys_halt_func);
    engine.register_trampoline(
        store.inner.wrap_stored(SYS_STATE_CODE.into()),
        sys_state_func,
    );
    linker.define("env", "_sys_halt", sys_halt_func).unwrap();
    linker.define("env", "_sys_state", sys_state_func).unwrap();
    // run start entrypoint
    let instance = linker
        .instantiate(&mut store, &module)
        .unwrap()
        .start(&mut store)
        .unwrap();
    let main_func = instance.get_func(&mut store, "main").unwrap();
    main_func.call(&mut store, &[], &mut []).unwrap();
    store.data().clone()
}

fn execute_binary_default(wat: &str) -> HostState {
    let config = RwasmModule::default_config(Some(create_import_linker()));
    execute_binary(wat, HostState::default(), config)
}

#[test]
fn test_memory_section() {
    execute_binary_default(
        r#"
    (module
      (type (;0;) (func))
      (func (;0;) (type 0)
        i32.const 0
        i64.load offset=0
        drop
        return
        )
      (memory (;0;) 17)
      (export "main" (func 0))
      (data (;0;) (i32.const 1048576) "Hello, World"))
        "#,
    );
}

#[test]
fn test_execute_br_and_drop_keep() {
    execute_binary_default(
        r#"
    (module
      (type (;0;) (func))
      (func (;0;) (type 0)
        i32.const 7
        (block $my_block
          i32.const 100
          i32.const 20
          i32.const 3
          br $my_block
          )
        i32.const 3
        i32.add
        return
        )
      (memory (;0;) 17)
      (export "main" (func 0)))
        "#,
    );
}

#[test]
fn test_executed_nested_function_calls() {
    execute_binary_default(
        r#"
    (module
      (type (;0;) (func))
      (func (;0;) (type 0)
        i32.const 100
        i32.const 20
        i32.add
        i32.const 20
        i32.add
        drop
        )
      (func (;1;) (type 0)
        call 0
        )
      (memory (;0;) 17)
      (export "main" (func 1)))
        "#,
    );
}

#[test]
fn test_recursive_main_call() {
    execute_binary_default(
        r#"
    (module
      (type (;0;) (func))
      (func (;0;) (type 0)
        (block $my_block
          global.get 0
          i32.const 3
          i32.gt_u
          br_if $my_block
          global.get 0
          i32.const 1
          i32.add
          global.set 0
          call 0
          )
        )
      (global (;0;) (mut i32) (i32.const 0))
      (export "main" (func 0)))
        "#,
    );
}

#[test]
fn test_execute_simple_add_program() {
    execute_binary_default(
        r#"
    (module
      (func $main
        global.get 0
        global.get 1
        call $add
        global.get 2
        call $add
        drop
        )
      (func $add (param $lhs i32) (param $rhs i32) (result i32)
        local.get $lhs
        local.get $rhs
        i32.add
        )
      (global (;0;) i32 (i32.const 100))
      (global (;1;) i32 (i32.const 20))
      (global (;2;) i32 (i32.const 3))
      (export "main" (func $main)))
        "#,
    );
}

#[test]
fn test_exit_code() {
    let host_state = execute_binary_default(
        r#"
    (module
      (type (;0;) (func (param i32)))
      (type (;1;) (func))
      (import "env" "_sys_halt" (func (;0;) (type 0)))
      (func (;1;) (type 1)
        i32.const 123
        call 0)
      (memory (;0;) 17)
      (export "memory" (memory 0))
      (export "main" (func 1)))
        "#,
    );
    assert_eq!(host_state.exit_code, 123);
}

#[test]
fn test_call_indirect() {
    execute_binary_default(
        r#"
    (module
      (type $check (func (param i32) (param i32) (result i32)))
      (table funcref (elem $add))
      (func $main
        i32.const 100
        i32.const 20
        i32.const 0
        call_indirect (type $check)
        drop
        )
      (func $add (type $check)
        local.get 0
        local.get 1
        i32.add
        )
      (export "main" (func $main)))
        "#,
    );
}

#[test]
fn test_passive_data_section() {
    execute_binary_default(
        r#"
    (module
      (type (;0;) (func))
      (func (;0;) (type 0)
        return
        )
      (memory (;0;) 17)
      (export "main" (func 0))
      (data "Hello, World"))
        "#,
    );
}

#[test]
fn test_passive_elem_section() {
    execute_binary_default(
        r#"
(module
  (table 1 funcref)
  (func $main
    return
    )
  (func $f1 (result i32)
   i32.const 42
   )
  (func $f2 (result i32)
   i32.const 100
   )
  (elem func $f1)
  (elem func $f2)
  (export "main" (func $main)))
    "#,
    );
}

#[test]
fn test_locals() {
    let host_state = execute_binary_default(
        r#"
    (module
      (type (;0;) (func))
      (func (;0;) (type 0)
        (local i32)
        return)
      (memory (;0;) 1)
      (export "memory" (memory 0))
      (export "main" (func 0)))
        "#,
    );
    assert_eq!(host_state.exit_code, 0);
}

#[test]
fn test_state_router() {
    let wat = r#"
    (module
      (type (;0;) (func (param i32)))
      (type (;1;) (func))
      (type (;2;) (func (result i32)))
      (import "env" "_sys_halt" (func (;0;) (type 0)))
      (import "env" "_sys_state" (func (;1;) (type 2)))
      (func (;2;) (type 1)
        i32.const 100
        call 0)
      (func (;3;) (type 1)
        i32.const 200
        call 0)
      (memory (;0;) 1)
      (export "memory" (memory 0))
      (export "main" (func 2))
      (export "deploy" (func 3)))
        "#;

    const STATE_DEPLOY: u32 = 11;
    const STATE_MAIN: u32 = 22;

    let mut config = RwasmModule::default_config(None);
    config.rwasm_config(RwasmConfig {
        state_router: Some(StateRouterConfig {
            states: Box::new([
                ("deploy".to_string(), STATE_DEPLOY),
                ("main".to_string(), STATE_MAIN),
            ]),
            opcode: Instruction::Call(SYS_STATE_CODE.into()),
        }),
        entrypoint_name: None,
        import_linker: Some(create_import_linker()),
        wrap_import_functions: true,
        translate_drop_keep: false,
        use_32bit_mode: false,
    });
    // run with deployment state (a result is 200)
    let mut host_state = HostState::default();
    host_state.state = STATE_DEPLOY;
    host_state = execute_binary(wat, host_state, config.clone());
    assert_eq!(host_state.exit_code, 200);
    // run with the main state (a result is 100)
    host_state.state = STATE_MAIN;
    host_state = execute_binary(wat, host_state, config);
    assert_eq!(host_state.exit_code, 100);
}
