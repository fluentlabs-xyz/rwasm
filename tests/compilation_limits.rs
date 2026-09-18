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
    assert_ne!(strict.codegen_identity(), relaxed.codegen_identity());
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

#[cfg(feature = "wasmtime")]
#[test]
fn cached_strategy_enforces_a_stricter_function_type_limit() {
    let wasm = module_with_distinct_types(3);
    let relaxed = config()
        .with_consume_fuel(false)
        .with_max_allowed_function_types(3);
    let strict = relaxed.clone().with_max_allowed_function_types(2);
    let cache_key = Some([0x73; 32]);

    // Warm the cache under a policy that accepts this module.
    StrategyDefinition::new_as_wasmtime(relaxed.clone(), &wasm, cache_key).unwrap();
    for key in [None, cache_key] {
        let result = StrategyDefinition::new_as_wasmtime(strict.clone(), &wasm, key);
        assert!(
            matches!(
                result,
                Err(CompilationError::TooManyFunctionTypes { count: 3, limit: 2 })
            ),
            "the stricter limit must be enforced even when the module is cached"
        );
    }
    // Rejecting the stricter policy must not invalidate the relaxed policy's cached module.
    assert!(StrategyDefinition::new_as_wasmtime(relaxed, &wasm, cache_key).is_ok());
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

mod code_size_bound {
    //! HIGH-4: the translator had no bound on emitted code, so a small input compiled to an
    //! enormous module.
    //!
    //! Both tests assert that the emitted code stays within a sane instruction budget for the input,
    //! either by rejecting the module with a compile-time size limit or by emitting a bounded amount.
    //! They failed at the audited revision: a `br_table` emitted ~2001 instructions (16 KB) per
    //! one-byte target and a `br_if` with a 1000-value `DropKeep` the same per occurrence, with no
    //! limit anywhere in the compiler.
    //!
    //! The bound used here is 2,000,000 instructions, the order of magnitude the host's
    //! `RWASM_MAX_CODE_SIZE` (12 MiB) implies.

    use rwasm::{CompilationConfig, CompilationError, RwasmModule};

    const MAX_INSTRUCTIONS: usize = 2_000_000;

    fn config() -> CompilationConfig {
        CompilationConfig::default_strategy_compatible()
            .with_entrypoint_name("main".into())
            .with_allow_malformed_entrypoint_func_type(true)
    }

    /// A `br_table` whose targets all name a 1000-result block, so every target needs a
    /// `DropKeep`-style trampoline (`2 * keep + 1` instructions) and none of them is shared.
    fn br_table_module(targets: usize) -> String {
        let results = "(result i32)".repeat(1000);
        let consts = "(i32.const 0)".repeat(1000);
        let target_list = "$b ".repeat(targets);
        format!(
            r#"(module
                 (func (export "main") {results}
                   (block $b {results}
                     (i32.const 7)
                     {consts}
                     (i32.const 0)
                     (br_table {target_list}$b))))"#
        )
    }

    /// One `br_if` per occurrence, each with a 1000-value `DropKeep` branch to the same block.
    fn br_if_module(branches: usize) -> String {
        let results = "(result i32)".repeat(1000);
        let consts = "(i32.const 0)".repeat(1000);
        let branch = "(br_if $b (i32.const 0))".repeat(branches);
        format!(
            r#"(module
                 (func (export "main") {results}
                   (block $b {results}
                     (i32.const 7)
                     {consts}
                     {branch}
                     (br $b))))"#
        )
    }

    fn assert_bounded(label: &str, wasm: &[u8]) {
        let instructions = match RwasmModule::compile(config(), wasm) {
            Ok((module, _)) => module.code_section.len(),
            // The fix rejects an oversized expansion with a dedicated error; any other error means
            // the fixture is broken or an unrelated regression, not a bounded compiler.
            Err(CompilationError::CodeSizeExceeded { .. }) => return,
            Err(err) => panic!("{label}: expected CodeSizeExceeded, got {err:?}"),
        };
        assert!(
            instructions <= MAX_INSTRUCTIONS,
            "{label}: {} B of Wasm compiled to {instructions} rwasm instructions ({:.1} MiB), which \
             grows without bound in the target count",
            wasm.len(),
            (instructions * core::mem::size_of::<rwasm::Opcode>()) as f64 / (1024.0 * 1024.0),
        );
    }

    #[test]
    fn br_table_expansion_is_bounded() {
        let wasm = wat::parse_str(br_table_module(10_000)).unwrap();
        assert_bounded("br_table with 10,000 targets", &wasm);
    }

    #[test]
    fn br_if_expansion_is_bounded() {
        let wasm = wat::parse_str(br_if_module(10_000)).unwrap();
        assert_bounded("10,000 br_if with a 1000-value drop-keep", &wasm);
    }
}
