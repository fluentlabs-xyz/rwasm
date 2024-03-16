#![allow(dead_code, deprecated)]

use crate::{
    compiler::{
        config::CompilerConfig,
        types::{CompilerError, FuncOrExport},
    },
    constants::{N_MAX_RECURSION_DEPTH, N_MAX_STACK_HEIGHT},
};
use alloc::vec::Vec;
use rwasm::{
    core::ImportLinker,
    engine::{bytecode::Instruction, code_map::InstructionPtr, RwasmConfig},
    Config,
    Engine,
    Module,
    StackLimits,
};

#[deprecated(note = "doesn't work, will be removed, use `RwasmModule` instead")]
pub struct Compiler {
    // input params
    pub config: CompilerConfig,
    // parsed wasmi state
    engine: Engine,
    module: Module,
    // translation state
    function_beginning: Vec<u32>,
}

impl Compiler {
    pub fn new(wasm_binary: &[u8], config: CompilerConfig) -> Result<Self, CompilerError> {
        Self::new_with_linker(wasm_binary, config, None)
    }

    pub fn new_with_linker(
        wasm_binary: &[u8],
        config: CompilerConfig,
        _import_linker: Option<&ImportLinker>,
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
        engine_config.wasm_tail_call(false);
        engine_config.wasm_extended_const(config.extended_const);
        engine_config.consume_fuel(config.fuel_consume);
        engine_config.rwasm_config(RwasmConfig::default());
        let engine = Engine::new(&engine_config);
        let module =
            Module::new(&engine, wasm_binary).map_err(|e| CompilerError::ModuleError(e))?;
        Ok(Compiler {
            engine,
            module,
            function_beginning: Vec::new(),
            config,
        })
    }

    pub fn finalize(self) -> (Engine, Module) {
        (self.engine, self.module)
    }

    pub fn config(&self) -> &CompilerConfig {
        &self.config
    }

    fn translate_opcode(
        &mut self,
        instr_ptr: &mut InstructionPtr,
        _return_ptr_offset: usize,
    ) -> Result<(), CompilerError> {
        use Instruction as WI;

        match *instr_ptr.get() {
            // WI::BrAdjust(branch_offset) => {
            //     opcode_count_origin += 1;
            //     Self::extract_drop_keep(instr_ptr).translate(&mut self.code_section)?;
            //     self.code_section.op_br(branch_offset);
            //     self.code_section.op_return();
            // }
            // WI::BrAdjustIfNez(branch_offset) => {
            //     opcode_count_origin += 1;
            //     let br_if_offset = self.code_section.len();
            //     self.code_section.op_br_if_eqz(0);
            //     Self::extract_drop_keep(instr_ptr).translate(&mut self.code_section)?;
            //     let drop_keep_len = self.code_section.len() - br_if_offset + 1;
            //     self.code_section
            //         .get_mut(br_if_offset as usize)
            //         .unwrap()
            //         .update_branch_offset(BranchOffset::from(1 + drop_keep_len as i32));
            //     let mut branch_offset = branch_offset.to_i32();
            //     if branch_offset < 0 {
            //         branch_offset -= 3;
            //     }
            //     self.code_section.op_br(branch_offset);
            //     self.code_section.op_return();
            // }
            WI::ReturnCallInternal(_) | WI::ReturnCall(_) | WI::ReturnCallIndirect(_) => {
                unreachable!("not supported tail call")
            }
            // WI::Return(drop_keep) => {
            //     drop_keep.translate(&mut self.code_section)?;
            //     self.code_section.op_return();
            // }
            // WI::ReturnIfNez(drop_keep) => {
            //     let br_if_offset = self.code_section.len();
            //     self.code_section.op_br_if_eqz(0);
            //     drop_keep.translate(&mut self.code_section)?;
            //     let drop_keep_len = self.code_section.len() - br_if_offset;
            //     self.code_section
            //         .get_mut(br_if_offset as usize)
            //         .unwrap()
            //         .update_branch_offset(BranchOffset::from(1 + drop_keep_len as i32));
            //     self.code_section.op_return();
            // }
            _ => {}
        };

        instr_ptr.add(1);
        Ok(())
    }

    pub fn resolve_func_index(&self, export: &FuncOrExport) -> Result<Option<u32>, CompilerError> {
        match export {
            FuncOrExport::Export(name) => Some(self.resolve_export_index(name)).transpose(),
            FuncOrExport::Func(index) => Ok(Some(*index)),
            _ => Ok(None),
        }
    }

    fn resolve_export_index(&self, name: &str) -> Result<u32, CompilerError> {
        let export_index = self
            .module
            .exports
            .get(name)
            .ok_or(CompilerError::MissingEntrypoint)?
            .into_func_idx()
            .ok_or(CompilerError::MissingEntrypoint)?
            .into_u32();
        Ok(export_index)
    }
}
