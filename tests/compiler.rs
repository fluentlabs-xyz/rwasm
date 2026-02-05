use rwasm::{CompilationConfig, CompilationError, ConstructorParams, RwasmModule};

fn test_compilation(wat_str: &str) -> Result<(RwasmModule, ConstructorParams), CompilationError> {
    let wasm = wat::parse_str(wat_str).expect("valid WAT");
    let config = CompilationConfig::default();
    RwasmModule::compile(config, &wasm)
}

#[test]
fn test_i64_type_split_into_2_x_i32_bug() {
    const WAT: &str = r#"
        (module
          (type (;0;) (func))
          (func (;0;) (type 0)
            i64.const 0
            i64.const 0
            i32.const 0
            if (param i64 i64)
              drop
              i64.const 0
              i64.add
              drop
            else
              drop
              i64.const 0
              i64.add
              drop
            end)
          (export "!" (func 0)))
    "#;
    let _ = test_compilation(WAT);
}

#[test]
fn test_extern_ref_not_supported_as_a_local_bug() {
    const WAT: &str = r#"
        (module
          (type (;0;) (func))
          (global (;0;) (mut i32) i32.const 1000)
          (export "" (func 0))
          (func (;0;) (type 0)
            (local f64 externref)
            global.get 0
            i32.eqz
            if ;; label = @1
              unreachable
            end
            global.get 0
            i32.const 1
            i32.sub
            global.set 0
          )
        (export "!" (func 0)))
    "#;
    let _ = test_compilation(WAT);
}

#[test]
fn test_ref_null_tracked_as_i32_bug() {
    const WAT: &str = r#"
        (module
          (type (;0;) (func (param externref)))
          (type (;1;) (func (result i32)))
          (table (;0;) 760 763 funcref)
          (memory (;0;) 8 9)
          (global (;0;) (mut i32) i32.const 1000)
          (export "" (func 0))
          (export "1" (table 0))
          (export "2" (memory 0))
          (func (;0;) (type 0) (param externref)
            global.get 0
            i32.eqz
            if ;; label = @1
              unreachable
            end
            global.get 0
            i32.const 1
            i32.sub
            global.set 0
            table.size 0
            ref.null extern
            call 0
            table.size 0
            table.size 0
            drop
            drop
            drop
          )
        )
    "#;
    let _ = test_compilation(WAT);
}

/// 32767 i64 locals (32767 = 0xFF 0xFF 0x01 LEB128) - max before DropKeepOutOfBounds
const LOCALS_32767_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
    0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: () -> ()
    0x03, 0x02, 0x01, 0x00, // function section: 1 func, type 0
    0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // export "main"
    0x0a, 0x08, 0x01, 0x06, 0x01, 0xff, 0xff, 0x01, 0x7e, 0x0b, // code: 32767 i64 locals
];

#[test]
fn test_max_locals_single_func() {
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_consume_fuel(true);

    let (module, _) = RwasmModule::compile(config, LOCALS_32767_WASM).expect("compile");

    let input_size = LOCALS_32767_WASM.len();
    let output_size = module.serialize().len();

    eprintln!("\n=== Single Function, 32767 Locals ===");
    eprintln!("Input:  {} bytes", input_size);
    eprintln!(
        "Output: {} bytes ({:.2} MB)",
        output_size,
        output_size as f64 / 1_000_000.0
    );
}
