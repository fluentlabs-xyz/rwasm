use walrus::ir::{BinaryOp, Binop, Const, Instr, InstrSeq, UnaryOp, Unop, Value};

fn extract_wasm_snippet(wasm_binary: &[u8]) {
    let module = walrus::Module::from_buffer(wasm_binary).unwrap();
    for func in module.funcs.iter() {
        let Some(func_name) = func.name.clone() else {
            continue;
        };
        let kind = func.kind.unwrap_local();
        assert_eq!(kind.args.len(), 4);
        let block = kind.block(kind.entry_block());
        println!("func: {}", func_name);
        test_ending_opcodes(&block);
        println!()
    }
}

#[rustfmt::skip]
fn test_ending_opcodes(seq: &InstrSeq) {
    let fake_block = seq
        .iter()
        .rev()
        .take(6)
        .map(|v| v.0.clone())
        .rev()
        .collect::<Vec<_>>();
    for instr in &fake_block {
        println!(" - {:?}", instr);
    }
    assert_eq!(fake_block.len(), 6);
    // make sure the fake block we injected matches a>>32|b
    assert!(matches!(fake_block[0],Instr::Unop(Unop {op: UnaryOp::I64ExtendUI32})));
    assert!(matches!(fake_block[1], Instr::Const(Const{value: Value::I64(32)})));
    assert!(matches!(fake_block[2],Instr::Binop(Binop {op: BinaryOp::I64Shl})));
    assert!(matches!(fake_block[3],Instr::LocalGet(..)));
    assert!(matches!(fake_block[4],Instr::Unop(Unop {op: UnaryOp::I64ExtendUI32})));
    assert!(matches!(fake_block[5],Instr::Binop(Binop {op: BinaryOp::I64Or})));
}

#[test]
fn test_extract_i64_add() {
    let wasm_binary = include_bytes!("./lib.wasm");
    extract_wasm_snippet(wasm_binary);
}
