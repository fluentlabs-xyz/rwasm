use rwasm::{CompilationConfig, RwasmModule};

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

fn build_max_locals_module(num_funcs: u32) -> Vec<u8> {
    let num_funcs_leb = leb128(num_funcs);

    let func_section_size = num_funcs_leb.len() + num_funcs as usize;
    let func_section_size_leb = leb128(func_section_size as u32);

    // Each function body: size=6, 1 local decl, 32767 (0xFF 0xFF 0x01), i64, end
    let body: &[u8] = &[0x06, 0x01, 0xff, 0xff, 0x01, 0x7e, 0x0b];
    let code_section_size = num_funcs_leb.len() + (num_funcs as usize * body.len());
    let code_section_size_leb = leb128(code_section_size as u32);

    let mut wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // magic + version
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type section: () -> ()
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
fn test_max_number_of_locals() {
    let wasm_input_binary = build_max_locals_module(20);
    let (rwasm_module, _) = RwasmModule::compile(
        CompilationConfig::default().with_entrypoint_name("main".into()),
        &wasm_input_binary,
    )
    .unwrap();
    println!("module = {}", rwasm_module);
    let rwasm_module_bytes = rwasm_module.serialize();
    println!("module_size = {}", rwasm_module_bytes.len());
    // old locals: 15'728'970 bytes
    // new local: 1'130 bytes
}
