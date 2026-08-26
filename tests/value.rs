use rwasm::{ExternRef, FuncRef, I64ValueSplit, TransmuteInto, UntypedValue, Value, F32, F64};

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

    assert_eq!(Value::I64(1).i32(), None);
    assert_eq!(Value::I32(1).i64(), None);
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
