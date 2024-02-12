use crate::{
    compiler::{
        config::CompilerConfig,
        types::{
            CompilerError,
            FuncOrExport,
            Injection,
            N_MAX_RECURSION_DEPTH,
            N_MAX_STACK_HEIGHT,
        },
    },
    ImportLinker,
    InstructionSet,
};
use alloc::{collections::BTreeMap, vec::Vec};
use rwasm::{
    common::{UntypedValue, ValueType},
    module::{ConstExpr, ElementSegmentKind},
    Config,
    Engine,
    Module,
    StackLimits,
};

pub struct Compiler2<'linker> {
    // input params
    pub(crate) import_linker: Option<&'linker ImportLinker>,
    pub(crate) config: CompilerConfig,
    // parsed wasmi state
    engine: Engine,
    module: Module,
    // translation state
    pub(crate) code_section: InstructionSet,
    function_beginning: BTreeMap<u32, u32>,
    injection_segments: Vec<Injection>,
}

impl<'linker> Compiler2<'linker> {
    pub fn new(wasm_binary: &[u8], config: CompilerConfig) -> Result<Self, CompilerError> {
        Self::new_with_linker(wasm_binary, config, None)
    }

    pub fn new_with_linker(
        wasm_binary: &[u8],
        config: CompilerConfig,
        import_linker: Option<&'linker ImportLinker>,
    ) -> Result<Self, CompilerError> {
        let mut engine_config = Config::default();
        engine_config.set_stack_limits(
            StackLimits::new(
                N_MAX_STACK_HEIGHT,
                N_MAX_STACK_HEIGHT,
                N_MAX_RECURSION_DEPTH,
            )
            .unwrap(),
        );
        engine_config.wasm_bulk_memory(true);
        engine_config.wasm_tail_call(true);
        engine_config.wasm_extended_const(config.extended_const);
        engine_config.consume_fuel(config.fuel_consume);
        engine_config.wasm_tail_call(config.tail_call);
        let engine = Engine::new(&engine_config);
        let module =
            Module::new(&engine, wasm_binary).map_err(|e| CompilerError::ModuleError(e))?;
        Ok(Compiler2 {
            engine,
            module,
            code_section: InstructionSet::new(),
            function_beginning: BTreeMap::new(),
            import_linker,
            injection_segments: vec![],
            config,
        })
    }

    pub fn config(&self) -> &CompilerConfig {
        &self.config
    }

    pub fn translate(&mut self, main_index: FuncOrExport) -> Result<InstructionSet, CompilerError> {
        // let's reserve 0 index for the magic prefix (?)
        if self.code_section.is_empty() && self.config.with_magic_prefix {
            self.code_section.op_magic_prefix([0x00; 8]);
        }

        todo!("")
    }

    pub fn translate_global(&mut self, global_index: u32) -> Result<(), CompilerError> {
        let globals = &self.module.globals;
        assert!(global_index < globals.len() as u32);

        // if global index less than global num then its imported global, and we have special call
        // index to translate such calls
        let len_globals = self.module.imports.len_globals;
        if global_index < len_globals as u32 {
            let global_start_index = self
                .config
                .global_start_index
                .ok_or(CompilerError::ExportedGlobalsAreDisabled)?;
            self.code_section.op_call(global_start_index + global_index);
            self.code_section.op_global_set(global_index);
            return Ok(());
        }

        // extract global init code to embed it into codebase
        let global_inits = &self.module.globals_init;
        assert!(global_index as usize - len_globals < global_inits.len());

        let global_expr = &global_inits[global_index as usize - len_globals];
        if let Some(value) = global_expr.eval_const() {
            self.code_section.op_i64_const(value);
        } else if let Some(value) = global_expr.funcref() {
            self.code_section.op_ref_func(value.into_u32());
        } else if let Some(index) = global_expr.global() {
            self.code_section.op_global_get(index.into_u32());
        } else {
            self.code_section
                .op_i64_const(self.translate_const_expr(global_expr)?.to_bits());
        }

        self.code_section.op_global_set(global_index);
        Ok(())
    }

    pub fn translate_const_expr(
        &self,
        const_expr: &ConstExpr,
    ) -> Result<UntypedValue, CompilerError> {
        return if cfg!(feature = "e2e") {
            let init_value = const_expr
                .eval_with_context(|_| rwasm::Value::I32(666), |_| rwasm::FuncRef::default())
                .ok_or(CompilerError::NotSupportedGlobalExpr)?;
            Ok(init_value)
        } else {
            let init_value = const_expr
                .eval_const()
                .ok_or(CompilerError::NotSupportedGlobalExpr)?;
            Ok(init_value)
        };
    }

    pub fn translate_tables(&mut self) -> Result<(), CompilerError> {
        for (table_index, table) in self.module.tables.iter().enumerate() {
            // don't use ref_func here due to the entrypoint section
            self.code_section.op_i32_const(0);
            if table_index < self.module.imports.len_tables {
                self.code_section.op_i64_const(table.minimum() as usize);
            } else {
                self.code_section.op_i64_const(table.minimum() as usize);
            }
            self.code_section.op_table_grow(table_index as u32);
            self.code_section.op_drop();
        }
        Ok(())
    }

    pub fn translate_elements(&mut self) -> Result<(), CompilerError> {
        for (i, e) in self.module.element_segments.iter().enumerate() {
            if e.ty() != ValueType::FuncRef {
                return Err(CompilerError::OnlyFuncRefAllowed);
            }
            match &e.kind() {
                ElementSegmentKind::Passive => {
                    for (_, item) in e.items_cloned().items().iter().enumerate() {
                        if let Some(value) = item.funcref() {
                            self.code_section.op_ref_func(value.into_u32());
                            self.code_section.op_elem_store(i as u32);
                        }
                    }
                }
                ElementSegmentKind::Active(aes) => {
                    let dest_offset = self.translate_const_expr(aes.offset())?;
                    for (index, item) in e.items_cloned().items().iter().enumerate() {
                        self.code_section
                            .op_i32_const(dest_offset.as_u32() + index as u32);
                        if let Some(value) = item.eval_const() {
                            self.code_section.op_i64_const(value);
                        } else if let Some(value) = item.funcref() {
                            self.code_section.op_ref_func(value.into_u32());
                        }
                        self.code_section.op_table_set(aes.table_index().into_u32());
                    }
                    if cfg!(feature = "e2e") {
                        self.code_section.op_i64_const(dest_offset);
                        self.code_section.op_i64_const(0);
                        self.code_section.op_i64_const(0);
                        self.code_section
                            .op_table_init(aes.table_index().into_u32(), i as u32);
                    }
                }
                ElementSegmentKind::Declared => return Ok(()),
            };
        }
        Ok(())
    }
}
