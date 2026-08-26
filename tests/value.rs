use rwasm::{
    CompilationConfig, ExternRef, FuncRef, I64ValueSplit, StrategyDefinition, TransmuteInto,
    UntypedValue, Value, F32, F64,
};

/// Covers matching and mismatching typed accessors and conversion to VM words.
#[test]
fn value_conversions_and_accessors() {
    let values = [
        (Value::I32(-1), u32::MAX),
        (Value::I64(-2), u32::MAX - 1),
        (Value::F32(F32::from(1.25)), 1.25_f32.to_bits()),
        (Value::F64(F64::from(2.5)), 2.5_f64.to_bits() as u32),
        (Value::FuncRef(FuncRef::new(7)), 7),
        (Value::ExternRef(ExternRef::new(8)), 8),
    ];
    for (value, expected_bits) in values {
        assert_eq!(UntypedValue::from(value).to_bits(), expected_bits);
    }

    assert_eq!(Value::I32(-1).i32(), Some(-1));
    assert_eq!(Value::I64(1).i32(), None);
    assert_eq!(Value::I32(1).i64(), None);
    assert_eq!(Value::I64(-2).i64(), Some(-2));
    assert_eq!(Value::F32(F32::from(1.0)).f32(), Some(F32::from(1.0)));
    assert_eq!(Value::I32(1).f32(), None);
    assert_eq!(Value::F64(F64::from(1.0)).f64(), Some(F64::from(1.0)));
    assert_eq!(Value::I32(1).f64(), None);

    let func_ref = Value::FuncRef(FuncRef::new(7));
    assert_eq!(func_ref.funcref(), Some(&FuncRef::new(7)));
    assert_eq!(Value::I32(1).funcref(), None);
    let extern_ref = Value::ExternRef(ExternRef::new(8));
    assert_eq!(extern_ref.externref(), Some(&ExternRef::new(8)));
    assert_eq!(Value::I32(1).externref(), None);
}

/// Covers bit-preserving transmutations between primitive and NaN-preserving floats.
#[test]
fn value_transmute_conversions_preserve_bits() {
    assert_eq!(TransmuteInto::<i32>::transmute_into(1_i32), 1);
    assert_eq!(TransmuteInto::<u32>::transmute_into(-1_i32), u32::MAX);
    assert_eq!(TransmuteInto::<f32>::transmute_into(F32::from(1.0)), 1.0);
    assert_eq!(
        TransmuteInto::<F32>::transmute_into(1.0_f32),
        F32::from(1.0)
    );
    assert_eq!(
        TransmuteInto::<i32>::transmute_into(1.0_f32) as u32,
        1.0_f32.to_bits()
    );
    assert_eq!(
        TransmuteInto::<u32>::transmute_into(F32::from(1.0)),
        1.0_f32.to_bits()
    );
    assert_eq!(
        TransmuteInto::<F32>::transmute_into(1_u32),
        F32::from_bits(1)
    );

    assert_eq!(TransmuteInto::<f64>::transmute_into(F64::from(1.0)), 1.0);
    assert_eq!(
        TransmuteInto::<F64>::transmute_into(1.0_f64),
        F64::from(1.0)
    );
    assert_eq!(
        TransmuteInto::<i64>::transmute_into(1.0_f64) as u64,
        1.0_f64.to_bits()
    );
    assert_eq!(
        TransmuteInto::<u64>::transmute_into(F64::from(1.0)),
        1.0_f64.to_bits()
    );
    assert_eq!(
        TransmuteInto::<F64>::transmute_into(1_u64),
        F64::from_bits(1)
    );
}

/// Covers the two-word little-endian representation used for 64-bit VM values.
#[test]
fn i64_values_split_into_little_endian_words() {
    let value = 0x1122_3344_5566_7788_i64;
    let expected = (0x5566_7788, 0x1122_3344);
    assert_eq!(value.split_into_i32_tuple(), expected);
    assert_eq!(value.split_into_i32_array(), [expected.0, expected.1]);

    let value = value as u64;
    assert_eq!(value.split_into_i32_tuple(), expected);
    assert_eq!(value.split_into_i32_array(), [expected.0, expected.1]);
}

/// Covers memory helpers whose complete result fits in one 32-bit VM word.
#[test]
fn untyped_value_memory_operations_preserve_wasm_layout() {
    let mut memory = [0_u8; 32];
    let address = UntypedValue::from(1_u32);

    UntypedValue::i64_store(&mut memory, address, 1, UntypedValue::from(0x1122_3344_u32)).unwrap();
    assert_eq!(
        UntypedValue::i64_load(&memory, address, 1)
            .unwrap()
            .as_u64(),
        0x1122_3344
    );

    UntypedValue::f64_store(
        &mut memory,
        address,
        9,
        UntypedValue::from(F64::from_bits(0x5566_7788)),
    )
    .unwrap();
    assert_eq!(
        UntypedValue::f64_load(&memory, address, 9)
            .unwrap()
            .as_f64()
            .to_bits(),
        0x5566_7788
    );

    memory[0..4].copy_from_slice(&[0x80, 0x81, 0x02, 0x80]);
    let address = UntypedValue::default();
    assert_eq!(
        UntypedValue::i64_load8_u(&memory, address, 0)
            .unwrap()
            .as_u64(),
        0x80
    );
    assert_eq!(
        UntypedValue::i64_load16_u(&memory, address, 0)
            .unwrap()
            .as_u64(),
        0x8180
    );
    assert_eq!(
        UntypedValue::i64_load32_u(&memory, address, 0)
            .unwrap()
            .as_u64(),
        0x8002_8180
    );

    let value = UntypedValue::from(0x1122_3344_u32);
    UntypedValue::i64_store8(&mut memory, address, 8, value).unwrap();
    UntypedValue::i64_store16(&mut memory, address, 9, value).unwrap();
    UntypedValue::i64_store32(&mut memory, address, 11, value).unwrap();
    assert_eq!(&memory[8..15], &[0x44, 0x44, 0x33, 0x44, 0x33, 0x22, 0x11]);
}

/// Covers the public numeric helpers that operate on one VM word.
#[test]
fn untyped_value_numeric_operations_cover_public_wasm_helpers() {
    let one = UntypedValue::from(1_i32);
    let two = UntypedValue::from(2_i32);
    assert_eq!(one.i64_add(two).as_i64(), 3);
    assert_eq!(two.i64_sub(one).as_i64(), 1);

    let f32_value = UntypedValue::from(F32::from(-1.5));
    assert_eq!(f32_value.f32_abs().as_f32(), F32::from(1.5));
    assert_eq!(f32_value.f32_neg().as_f32(), F32::from(1.5));
    assert_eq!(f32_value.f32_ceil().as_f32(), F32::from(-1.0));
    assert_eq!(f32_value.f32_floor().as_f32(), F32::from(-2.0));
    assert_eq!(f32_value.f32_trunc().as_f32(), F32::from(-1.0));
    assert_eq!(f32_value.f32_nearest().as_f32(), F32::from(-2.0));
    assert_eq!(
        UntypedValue::from(F32::from(4.0)).f32_sqrt().as_f32(),
        F32::from(2.0)
    );

    let smallest = UntypedValue::from(F64::from_bits(1));
    let next = UntypedValue::from(F64::from_bits(2));
    assert_eq!(smallest.f64_eq(smallest).as_u32(), 1);
    assert_eq!(smallest.f64_ne(next).as_u32(), 1);
    assert_eq!(smallest.f64_lt(next).as_u32(), 1);
    assert_eq!(smallest.f64_le(next).as_u32(), 1);
    assert_eq!(next.f64_gt(smallest).as_u32(), 1);
    assert_eq!(next.f64_ge(smallest).as_u32(), 1);

    assert_eq!(
        UntypedValue::from(u32::MAX).i32_wrap_i64().as_u32(),
        u32::MAX
    );
    assert_eq!(
        UntypedValue::from(F32::from(12.75))
            .i32_trunc_f32_s()
            .unwrap()
            .as_i32(),
        12
    );
    assert_eq!(
        UntypedValue::from(F32::from(12.75))
            .i32_trunc_f32_u()
            .unwrap()
            .as_u32(),
        12
    );
    assert_eq!(
        UntypedValue::from(-2_i32).f32_convert_i32_s().as_f32(),
        F32::from(-2.0)
    );
    assert_eq!(
        UntypedValue::from(2_u32).f32_convert_i32_u().as_f32(),
        F32::from(2.0)
    );
    assert_eq!(
        UntypedValue::from(F32::from(-2.0))
            .i32_trunc_sat_f32_s()
            .as_i32(),
        -2
    );
    assert_eq!(
        UntypedValue::from(F32::from(-2.0))
            .i32_trunc_sat_f32_u()
            .as_u32(),
        0
    );

    let value = UntypedValue::from(7_u64);
    assert_eq!(value.as_u16(), 7);
    assert_eq!(value.as_usize(), 7);
    assert_eq!(value.as_f64().to_bits(), 7);
    assert_eq!(value.to_string(), "7");
}

/// Verifies full-width signed loads and floating-point negation without word truncation.
#[test]
fn runtime_preserves_full_width_i64_loads_and_f64_neg() {
    let wasm = wat::parse_str(
        r#"
            (module
                (memory 1)
                (data (i32.const 0) "\80\81\02\80")
                (func (export "load8") (result i64)
                    i32.const 0
                    i64.load8_s)
                (func (export "load16") (result i64)
                    i32.const 0
                    i64.load16_s)
                (func (export "load32") (result i64)
                    i32.const 0
                    i64.load32_s)
            )
        "#,
    )
    .unwrap();

    for (entrypoint, expected) in [
        ("load8", -128_i64),
        ("load16", 0xffff_ffff_ffff_8180_u64 as i64),
        ("load32", 0xffff_ffff_8002_8180_u64 as i64),
    ] {
        let config = CompilationConfig::default()
            .with_entrypoint_name(entrypoint.into())
            .with_allow_malformed_entrypoint_func_type(true);
        let strategy = StrategyDefinition::new_as_rwasm(config, &wasm).unwrap();
        let mut result = [Value::I64(0)];
        strategy
            .default_executor()
            .unwrap()
            .execute(entrypoint, &[], &mut result)
            .unwrap();
        assert_eq!(result[0].i64(), Some(expected));
    }

    assert_eq!((-F64::from(0.0)).to_bits(), 0x8000_0000_0000_0000);
}
