use rwasm::{
    instruction_set, CompilationConfig, CompilationError, HintType, Opcode, RwasmModule,
    RwasmModuleBuilder, SysFuncIdx, F32, F64,
};

/// Covers the public module builder, decoding entry points, hints, and display output.
#[test]
fn module_public_api_roundtrips_all_sections() {
    let module = RwasmModuleBuilder::new(instruction_set! {
        I32Const(7)
        Drop
    })
    .with_data_section(&[1, 2, 3])
    .with_elem_section(&[4, 5])
    .with_hint_section(b"\0asm\x01\0\0\0")
    .with_source_pc(1)
    .build();

    assert_eq!(module.data_section, [1, 2, 3]);
    assert_eq!(module.elem_section, [4, 5]);
    assert_eq!(module.hint_type(), HintType::WASM);
    assert!(format!("{module}").contains("0001: Drop  <- SOURCE"));

    let encoded = module.serialize();
    let (decoded, bytes_read) = RwasmModule::new_or_empty(&encoded);
    assert_eq!(decoded, module);
    assert_eq!(bytes_read, encoded.len());
    assert_eq!(RwasmModule::new_checked_exact(&encoded).unwrap(), module);

    let (empty, bytes_read) = RwasmModule::new_or_empty(&[]);
    assert_eq!(empty, RwasmModule::empty());
    assert_eq!(empty.hint_type(), HintType::EVM);
    assert_eq!(bytes_read, 0);

    let from_builder: RwasmModule = RwasmModuleBuilder::new(instruction_set! { Return }).into();
    assert_eq!(from_builder.code_section, instruction_set! { Return });
}

/// Covers the distinct malformed-header branches of the checked module decoder.
#[test]
fn checked_module_decoder_rejects_bad_magic_and_version() {
    let encoded = RwasmModule::empty().serialize();

    let mut bad_magic = encoded.clone();
    bad_magic[0] ^= 1;
    assert!(RwasmModule::new_checked(&bad_magic).is_err());

    let mut bad_version = encoded;
    bad_version[2] = bad_version[2].wrapping_add(1);
    assert!(RwasmModule::new_checked(&bad_version).is_err());
}

/// Covers every public opcode classification family with positive and negative cases.
#[test]
fn opcode_classification_covers_all_families() {
    for opcode in [
        Opcode::I32Load8S(0),
        Opcode::I32Load8U(0),
        Opcode::I32Load16S(0),
        Opcode::I32Load16U(0),
        Opcode::I32Load(0),
    ] {
        assert!(opcode.is_memory_instruction());
        assert!(opcode.is_memory_load_instruction());
        assert!(!opcode.is_memory_store_instruction());
    }
    for opcode in [
        Opcode::I32Store8(0),
        Opcode::I32Store16(0),
        Opcode::I32Store(0),
    ] {
        assert!(opcode.is_memory_instruction());
        assert!(!opcode.is_memory_load_instruction());
        assert!(opcode.is_memory_store_instruction());
    }
    assert!(!Opcode::I32Add.is_memory_instruction());

    assert!(Opcode::Call(SysFuncIdx::from(0u32)).is_ecall_instruction());
    assert!(Opcode::ReturnCall(SysFuncIdx::from(0u32)).is_ecall_instruction());
    assert!(!Opcode::Return.is_ecall_instruction());

    for opcode in [
        Opcode::Br(0i32.into()),
        Opcode::BrIfEqz(0i32.into()),
        Opcode::BrIfNez(0i32.into()),
    ] {
        assert!(opcode.is_branch_instruction());
    }
    assert!(!Opcode::BrTable(0).is_branch_instruction());
    assert!(!Opcode::Return.is_jump_instruction());
    assert!(!Opcode::Return.is_halt());

    for opcode in [
        Opcode::I32Clz,
        Opcode::I32Ctz,
        Opcode::I32Popcnt,
        Opcode::I32Eqz,
    ] {
        assert!(opcode.is_unary_instruction());
        assert!(!opcode.is_binary_instruction());
    }
    for opcode in [
        Opcode::I32Eq,
        Opcode::I32Ne,
        Opcode::I32LtS,
        Opcode::I32LtU,
        Opcode::I32GtS,
        Opcode::I32GtU,
        Opcode::I32LeS,
        Opcode::I32LeU,
        Opcode::I32GeS,
        Opcode::I32GeU,
        Opcode::I32Add,
        Opcode::I32Sub,
        Opcode::I32Mul,
        Opcode::I32DivS,
        Opcode::I32DivU,
        Opcode::I32RemS,
        Opcode::I32RemU,
        Opcode::I32And,
        Opcode::I32Or,
        Opcode::I32Xor,
        Opcode::I32Shl,
        Opcode::I32ShrS,
        Opcode::I32ShrU,
        Opcode::I32Rotl,
        Opcode::I32Rotr,
    ] {
        assert!(opcode.is_binary_instruction());
        assert!(!opcode.is_unary_instruction());
    }
}

/// Covers float helpers and conversions while preserving their exact bit representations.
#[test]
fn nan_preserving_float_helpers_keep_bits_and_format_values() {
    let f32_value = F32::from(-1.5);
    assert_eq!(f32_value.abs(), F32::from(1.5));
    assert_eq!(F32::from(1.5).fract(), F32::from(0.5));
    assert_eq!(format!("{f32_value:?}"), "-1.5");
    assert_eq!(format!("{f32_value}"), "-1.5");
    let f32_bits: u32 = F32::from_bits(0x7fc0_1234).into();
    assert_eq!(f32_bits, 0x7fc0_1234);

    let f64_value = F64::from(-1.5);
    assert_eq!(f64_value.abs(), F64::from(1.5));
    assert_eq!(F64::from(1.5).fract(), F64::from(0.5));
    assert_eq!(format!("{f64_value:?}"), "-1.5");
    assert_eq!(format!("{f64_value}"), "-1.5");
    let f64_bits: u64 = F64::from_bits(0x7ff8_0000_0000_1234).into();
    assert_eq!(f64_bits, 0x7ff8_0000_0000_1234);
}

/// Covers user-facing messages for every directly constructible compilation error.
#[test]
fn compilation_errors_have_stable_messages() {
    let cases = [
        (
            CompilationError::BranchOffsetOutOfBounds,
            "branch offset out of bounds",
        ),
        (
            CompilationError::BlockFuelOutOfBounds,
            "block fuel out of bounds",
        ),
        (
            CompilationError::NotSupportedExtension,
            "not supported extension",
        ),
        (
            CompilationError::DropKeepOutOfBounds,
            "drop keep out of bounds",
        ),
        (
            CompilationError::BranchTableTargetsOutOfBounds,
            "branch table targets are out of bounds",
        ),
        (
            CompilationError::NotSupportedImportType,
            "not supported an import type",
        ),
        (
            CompilationError::NotSupportedFuncType,
            "not supported func type",
        ),
        (
            CompilationError::UnresolvedImportFunction,
            "unresolved import function",
        ),
        (
            CompilationError::MalformedImportFunctionType,
            "malformed import function type",
        ),
        (
            CompilationError::NonDefaultMemoryIndex,
            "non default memory index",
        ),
        (
            CompilationError::ConstEvaluationFailed,
            "const evaluation failed",
        ),
        (
            CompilationError::NotSupportedLocalType,
            "not supported local type",
        ),
        (
            CompilationError::NotSupportedGlobalType,
            "not supported global type",
        ),
        (CompilationError::NotSupportedOpcode, "not supported opcode"),
        (
            CompilationError::MaxReadonlyDataReached,
            "memory segments overflow",
        ),
        (CompilationError::MissingEntrypoint, "missing entrypoint"),
        (CompilationError::MalformedFuncType, "malformed func type"),
        (
            CompilationError::MemoryOutOfBounds,
            "out of bounds memory access",
        ),
        (
            CompilationError::TableOutOfBounds,
            "out of bounds table access",
        ),
        (
            CompilationError::StartSectionsAreNotAllowed,
            "start sections are not allowed",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
    }

    let malformed = RwasmModule::compile(CompilationConfig::default(), &[0])
        .expect_err("a truncated Wasm header must be malformed");
    assert!(malformed.to_string().starts_with("malformed wasm binary ("));
}
