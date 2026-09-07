use rwasm::{CompilationConfig, CompilationError, ModuleParser, RwasmModule, StrategyDefinition};

fn config() -> CompilationConfig {
    CompilationConfig::default().with_entrypoint_name("main".into())
}

fn module_with_distinct_types(count: u32) -> Vec<u8> {
    let mut source = String::from("(module (type (func))");
    for index in 1..count {
        source.push_str("(type (func (param");
        for bit in 0..13 {
            source.push_str(if index & (1 << bit) == 0 {
                " i32"
            } else {
                " i64"
            });
        }
        source.push_str(")))");
    }
    source.push_str("(func (export \"main\") (type 0)))");
    wat::parse_str(source).unwrap()
}

#[test]
fn default_type_limit_rejects_large_valid_type_section() {
    let wasm = module_with_distinct_types(4097);
    let result = RwasmModule::compile(config(), &wasm);
    assert!(matches!(
        result,
        Err(CompilationError::TooManyFunctionTypes {
            count: 4097,
            limit: 4096
        })
    ));
}

#[test]
fn type_limit_is_inclusive_and_does_not_change_accepted_bytecode() {
    let wasm = module_with_distinct_types(4096);
    let strict = config();
    let relaxed = config().with_max_allowed_function_types(8192);
    assert_eq!(strict.codegen_identity(), relaxed.codegen_identity());
    let (strict_module, _) = RwasmModule::compile(strict, &wasm).unwrap();
    let (relaxed_module, _) = RwasmModule::compile(relaxed, &wasm).unwrap();
    assert_eq!(strict_module.serialize(), relaxed_module.serialize());
}

#[test]
fn explicit_type_limit_controls_acceptance() {
    let wasm = module_with_distinct_types(3);
    assert!(RwasmModule::compile(config().with_max_allowed_function_types(3), &wasm).is_ok());
    assert!(matches!(
        RwasmModule::compile(config().with_max_allowed_function_types(2), &wasm),
        Err(CompilationError::TooManyFunctionTypes { count: 3, limit: 2 })
    ));
    // The large module from the regression is valid when the host explicitly permits it.
    let wasm = module_with_distinct_types(4097);
    assert!(RwasmModule::compile(config().with_max_allowed_function_types(4097), &wasm).is_ok());
}

#[test]
fn oversized_count_is_rejected_before_type_body_validation() {
    // A type section declaring u32::MAX entries with no body. The limit error must precede
    // wasmparser's malformed-body error and its count-based storage reservation.
    let wasm = b"\0asm\x01\0\0\0\x01\x05\xff\xff\xff\xff\x0f";
    let err = RwasmModule::compile(config(), wasm).unwrap_err();
    assert!(matches!(
        err,
        CompilationError::TooManyFunctionTypes {
            count: u32::MAX,
            limit: 4096
        }
    ));
    assert_eq!(
        err.to_string(),
        "function type count 4294967295 exceeds compilation limit 4096"
    );

    let truncated = b"\0asm\x01\0\0\0\x01\x01\x01";
    assert!(matches!(
        RwasmModule::compile(config(), truncated),
        Err(CompilationError::MalformedWasmBinary(_))
    ));
}

#[test]
fn export_parsing_and_strategy_construction_enforce_type_limit() {
    let wasm = module_with_distinct_types(3);
    let config = config()
        .with_max_allowed_function_types(2)
        .with_consume_fuel(false);
    assert!(matches!(
        ModuleParser::parse_function_exports(config.clone(), &wasm),
        Err(CompilationError::TooManyFunctionTypes { count: 3, limit: 2 })
    ));
    assert!(matches!(
        StrategyDefinition::new_as_rwasm(config, &wasm),
        Err(CompilationError::TooManyFunctionTypes { count: 3, limit: 2 })
    ));
    assert_eq!(
        CompilationConfig::default_strategy_compatible().max_allowed_function_types,
        4096
    );
}

#[test]
fn duplicate_signatures_still_count_toward_limit() {
    let wasm = wat::parse_str("(module (type (func)) (type (func)) (type (func)))").unwrap();
    assert!(matches!(
        ModuleParser::new(config().with_max_allowed_function_types(2)).parse(&wasm),
        Err(CompilationError::TooManyFunctionTypes { count: 3, limit: 2 })
    ));
}

#[test]
fn zero_type_limit_accepts_only_type_free_modules() {
    let config = config().with_max_allowed_function_types(0);
    let empty = wat::parse_str("(module)").unwrap();
    ModuleParser::new(config.clone()).parse(&empty).unwrap();
    let wasm = module_with_distinct_types(1);
    assert!(matches!(
        RwasmModule::compile(config, &wasm),
        Err(CompilationError::TooManyFunctionTypes { count: 1, limit: 0 })
    ));
}
