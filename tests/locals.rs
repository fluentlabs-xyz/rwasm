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
    wasm.extend(core::iter::repeat_n(0x00, num_funcs as usize));

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

/// Runs `main` of `wat` on the rwasm VM and returns its single `i64` result.
fn run_main_i64(wat: &str) -> i64 {
    use rwasm::{ExecutionEngine, ImportLinker, RwasmStore, Value};
    let wasm = wat::parse_str(wat).expect("valid WAT");
    let config = CompilationConfig::default()
        .with_entrypoint_name("main".into())
        .with_allow_malformed_entrypoint_func_type(true);
    let (module, _) = RwasmModule::compile(config, &wasm).expect("module compiles");
    let mut store = RwasmStore::<()>::default();
    let instance = ImportLinker::default()
        .instantiate(&mut store, ExecutionEngine::new(), module)
        .unwrap();
    let mut result = [Value::I64(0)];
    instance.execute(&mut store, &[], &mut result).unwrap();
    result[0].i64().unwrap()
}

/// Local accesses are lowered to value-stack slot depths, and `i64` locals take two slots. Mixed
/// local widths, deep operand stacks and every `local.*` opcode must resolve to the right slots;
/// the compiler computes those depths incrementally rather than by rescanning the type stack.
#[test]
fn test_mixed_width_locals_resolve_to_the_right_slots() {
    const LOCALS: u32 = 300;
    let mut wat = String::from("(module (func (export \"main\") (result i64)");
    for i in 0..LOCALS {
        wat.push_str(if i % 2 == 0 {
            " (local i32)"
        } else {
            " (local i64)"
        });
    }
    // local i := i, going through `local.tee` on odd indices
    for i in 0..LOCALS {
        if i % 2 == 0 {
            wat.push_str(&format!(" i32.const {i} local.set {i}"));
        } else {
            wat.push_str(&format!(" i64.const {i} local.tee {i} drop"));
        }
    }
    // sum every local with a deep operand stack: push all, then fold with `i64.add`
    wat.push_str(" i64.const 0");
    for i in 0..LOCALS {
        if i % 2 == 0 {
            wat.push_str(&format!(" local.get {i} i64.extend_i32_u"));
        } else {
            wat.push_str(&format!(" local.get {i}"));
        }
    }
    for _ in 0..LOCALS {
        wat.push_str(" i64.add");
    }
    wat.push_str("))");
    let expected = (0..LOCALS as i64).sum::<i64>();
    assert_eq!(run_main_i64(&wat), expected);
}

/// A body that declares many locals and touches one of them many times compiles in linear time.
/// The local count stays below the VM's value-stack limit so the module also runs.
#[test]
fn test_many_locals_with_many_accesses_compile() {
    const LOCALS: u32 = 4_000;
    const ACCESSES: u32 = 40_000;
    let mut wat = String::from("(module (func (export \"main\") (result i64) (local i64)");
    for _ in 0..LOCALS - 2 {
        wat.push_str(" (local i32)");
    }
    wat.push_str(" (local i64)");
    wat.push_str(" i64.const 7 local.set 0");
    for _ in 0..ACCESSES {
        wat.push_str(&format!(" local.get 0 local.set {}", LOCALS - 1));
    }
    wat.push_str(&format!(" local.get {}))", LOCALS - 1));
    assert_eq!(run_main_i64(&wat), 7);
}
