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

fn leb128(mut n: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (n & 0x7F) as u8;
        n >>= 7;
        if n != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if n == 0 {
            break;
        }
    }
    out
}

/// Build module with N functions, each with 32767 i64 locals
fn build_max_locals_module(num_funcs: u32) -> Vec<u8> {
    let num_funcs_leb = leb128(num_funcs);

    // Function section: num_funcs × type index 0
    let func_section_size = num_funcs_leb.len() + num_funcs as usize;
    let func_section_size_leb = leb128(func_section_size as u32);

    // Code section: each function body is 6 bytes (1 local decl, 3-byte count, type, end)
    let body: &[u8] = &[0x06, 0x01, 0xff, 0xff, 0x01, 0x7e, 0x0b]; // size=6, 1 decl, 32767, i64, end
    let code_section_size = num_funcs_leb.len() + (num_funcs as usize * body.len());
    let code_section_size_leb = leb128(code_section_size as u32);

    let mut wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section
    ];

    // Function section
    wasm.push(0x03);
    wasm.extend_from_slice(&func_section_size_leb);
    wasm.extend_from_slice(&num_funcs_leb);
    for _ in 0..num_funcs {
        wasm.push(0x00);
    }

    // Export section (export first func as "main")
    wasm.extend_from_slice(&[0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00]);

    // Code section
    wasm.push(0x0a);
    wasm.extend_from_slice(&code_section_size_leb);
    wasm.extend_from_slice(&num_funcs_leb);
    for _ in 0..num_funcs {
        wasm.extend_from_slice(body);
    }

    wasm
}

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

#[test]
fn test_max_locals_max_funcs() {
    let num_funcs: &[(u32, Result<(), CompilationError>)] = &[
        // (10, Ok(())),
        (10, Err(CompilationError::CompiledBytecodeExceedsMaxSize)),
        // (1000, Err(CompilationError::CompiledBytecodeExceedsMaxSize)),
    ];
    for (num_funcs, expected_compile_result) in num_funcs.iter() {
        let wasm = build_max_locals_module(*num_funcs);
        let config = CompilationConfig::default()
            .with_entrypoint_name("main".into())
            .with_consume_fuel(true);

        let compile_result = RwasmModule::compile(config, &wasm);
        let (module, _) = match expected_compile_result {
            Ok(_) => compile_result.expect("compile OK"),
            Err(_) => {
                assert!(compile_result.is_err(), "expected compile error");
                continue;
            }
        };

        let input_size = wasm.len();
        let output_size = module.serialize().len();

        eprintln!("\n=== {} Functions × 32767 Locals ===", num_funcs);
        eprintln!(
            "Input:  {} bytes ({:.2} MB)",
            input_size,
            input_size as f64 / 1_000_000.0
        );
        eprintln!(
            "Output: {} bytes ({:.2} GB)",
            output_size,
            output_size as f64 / 1_000_000_000.0
        );
    }
}
