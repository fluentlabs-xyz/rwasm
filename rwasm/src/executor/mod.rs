use crate::{
    core::{Pages, TrapCode, UntypedValue},
    engine::{
        bytecode::Instruction,
        code_map::{FuncHeader, InstructionPtr, InstructionsRef},
        stack::{ValueStack, ValueStackPtr},
    },
    memory::MemoryEntity,
    rwasm::{BinaryFormatError, RwasmModule},
    store::ResourceLimiterRef,
    MemoryType,
};
use hashbrown::HashMap;

#[derive(Debug, Copy, Clone)]
pub enum RwasmError {
    BinaryFormatError(BinaryFormatError),
    TrapCode(TrapCode),
    UnknownExternalFunction(u32),
    ExecutionHalted(i32),
}

impl From<BinaryFormatError> for RwasmError {
    fn from(value: BinaryFormatError) -> Self {
        Self::BinaryFormatError(value)
    }
}
impl From<TrapCode> for RwasmError {
    fn from(value: TrapCode) -> Self {
        Self::TrapCode(value)
    }
}

pub trait ExternalCallHandler {
    fn call_function(
        &mut self,
        func_idx: u32,
        sp: &mut ValueStackPtr,
        global_memory: &mut MemoryEntity,
    ) -> Result<(), RwasmError>;
}

#[derive(Default)]
pub struct DefaultExternalCallHandler;

impl ExternalCallHandler for DefaultExternalCallHandler {
    fn call_function(
        &mut self,
        func_idx: u32,
        _sp: &mut ValueStackPtr,
        _global_memory: &mut MemoryEntity,
    ) -> Result<(), RwasmError> {
        Err(RwasmError::UnknownExternalFunction(func_idx))
    }
}

pub fn execute_rwasm_bytecode<E: ExternalCallHandler>(
    rwasm_bytecode: &[u8],
    external_call_handler: Option<&mut E>,
) -> Result<i32, RwasmError> {
    let rwasm_module = RwasmModule::new(rwasm_bytecode)?;
    execute_rwasm_module(rwasm_module, external_call_handler)
}

pub fn execute_rwasm_module<E: ExternalCallHandler>(
    rwasm_module: RwasmModule,
    mut external_call_handler: Option<&mut E>,
) -> Result<i32, RwasmError> {
    let mut func_segments = vec![0u32];
    let mut total_func_len = 0u32;
    for func_len in rwasm_module
        .func_section
        .iter()
        .take(rwasm_module.func_section.len() - 1)
    {
        total_func_len += *func_len;
        func_segments.push(total_func_len);
    }
    let source_pc = func_segments
        .last()
        .copied()
        .expect("rwasm: empty function section");

    let mut value_stack = ValueStack::default();

    let mut resource_limiter_ref = ResourceLimiterRef::default();
    let mut global_memory = MemoryEntity::new(
        MemoryType::new(0, Some(1024)).expect("rwasm: bad initial memory"),
        &mut resource_limiter_ref,
    )
    .expect("rwasm: bad initial memory");
    let mut global_variables = HashMap::new();

    let mut sp = value_stack.stack_ptr();
    let mut ip = InstructionPtr::new(
        rwasm_module.code_section.instr.as_ptr(),
        rwasm_module.code_section.metas.as_ptr(),
    );
    ip.add(source_pc as usize);
    let mut call_stack = Vec::new();

    loop {
        let instr = *ip.get();
        println!("{:02}: {:?}", ip.pc(), instr);
        match instr {
            Instruction::LocalGet(local_depth) => {
                let value = sp.nth_back(local_depth.to_usize());
                sp.push(value);
                ip.add(1);
            }
            Instruction::LocalSet(local_depth) => {
                let new_value = sp.pop();
                sp.set_nth_back(local_depth.to_usize(), new_value);
                ip.add(1);
            }
            Instruction::LocalTee(local_depth) => {
                let new_value = sp.last();
                sp.set_nth_back(local_depth.to_usize(), new_value);
                ip.add(1);
            }
            Instruction::Br(offset) => ip.offset(offset.to_i32() as isize),
            Instruction::BrIfEqz(offset) => {
                let condition = sp.pop_as();
                if condition {
                    ip.add(1);
                } else {
                    ip.offset(offset.to_i32() as isize);
                }
            }
            Instruction::BrIfNez(offset) => {
                let condition = sp.pop_as();
                if condition {
                    ip.offset(offset.to_i32() as isize);
                } else {
                    ip.add(1);
                }
            }
            Instruction::BrAdjust(offset) => {}
            // TODO(dmitry123): "add more opcodes"
            Instruction::MemoryGrow => {
                let delta: u32 = sp.pop_as();
                if delta > Pages::max().into_inner() {
                    sp.push_as(u32::MAX);
                    ip.add(1);
                    continue;
                }
                let new_pages = global_memory
                    .grow(Pages::new(delta).unwrap(), &mut resource_limiter_ref)
                    .map(u32::from)
                    .unwrap_or(u32::MAX);
                sp.push_as(new_pages);
                ip.add(1);
            }
            Instruction::MemoryInit(data_segment_idx) => {
                // TODO(dmitry123): "add emptiness check"
                assert_eq!(
                    data_segment_idx.to_u32(),
                    0,
                    "rwasm: non-zero data segment index"
                );
                let (d, s, n) = sp.pop3();
                let n = i32::from(n) as usize;
                let src_offset = i32::from(s) as usize;
                let dst_offset = i32::from(d) as usize;
                let memory = global_memory
                    .data_mut()
                    .get_mut(dst_offset..)
                    .and_then(|memory| memory.get_mut(..n))
                    .ok_or(TrapCode::MemoryOutOfBounds)?;
                let data = rwasm_module
                    .memory_section
                    .get(src_offset..)
                    .and_then(|data| data.get(..n))
                    .ok_or(TrapCode::MemoryOutOfBounds)?;
                memory.copy_from_slice(data);
                // if let Some(tracer) = this.tracer.as_mut() {
                //     tracer.global_memory(dst_offset as u32, n as u32, memory);
                // }
                ip.add(1);
            }
            Instruction::DataDrop(_data_segment_idx) => ip.add(1),
            Instruction::Drop => {
                sp.drop();
                ip.add(1);
            }
            Instruction::I32Const(value) => {
                sp.push(value);
                ip.add(1);
            }
            Instruction::I32Eq => {
                sp.eval_top2(UntypedValue::i32_eq);
                ip.add(1);
            }
            Instruction::I32Ne => {
                sp.eval_top2(UntypedValue::i32_ne);
                ip.add(1);
            }
            Instruction::I64Const(value) => {
                sp.push(value);
                ip.add(1);
            }
            Instruction::CallInternal(func_idx) => {
                ip.add(1);
                value_stack.sync_stack_ptr(sp);
                // TODO(dmitry123): "add recursion limit check"
                call_stack.push(ip);
                let instr_ref = func_segments
                    .get(func_idx.to_u32() as usize)
                    .copied()
                    .expect("rwasm: unknown internal function");
                let header = FuncHeader::new(InstructionsRef::uninit(), 0, 0);
                value_stack.prepare_wasm_call(&header)?;
                sp = value_stack.stack_ptr();
                ip = InstructionPtr::new(
                    rwasm_module.code_section.instr.as_ptr(),
                    rwasm_module.code_section.metas.as_ptr(),
                );
                ip.add(instr_ref as usize);
            }
            Instruction::Call(func_idx) => {
                value_stack.sync_stack_ptr(sp);
                let result = external_call_handler
                    .as_mut()
                    .ok_or(RwasmError::UnknownExternalFunction(func_idx.to_u32()))?
                    .call_function(func_idx.to_u32(), &mut sp, &mut global_memory);
                if let Err(err) = result {
                    return match err {
                        RwasmError::ExecutionHalted(exit_code) => Ok(exit_code),
                        _ => Err(err),
                    };
                }
                ip.add(1);
            }
            Instruction::SignatureCheck(_) => {
                ip.add(1);
            }
            Instruction::Unreachable => {
                return Err(RwasmError::TrapCode(TrapCode::UnreachableCodeReached));
            }
            Instruction::GlobalGet(global_idx) => {
                let global_value = global_variables
                    .get(&global_idx)
                    .copied()
                    .unwrap_or_default();
                sp.push(global_value);
                ip.add(1);
            }
            Instruction::GlobalSet(global_idx) => {
                let new_value = sp.pop();
                global_variables.insert(global_idx, new_value);
                ip.add(1);
            }
            Instruction::Return(drop_keep) => {
                sp.drop_keep(drop_keep);
                value_stack.sync_stack_ptr(sp);
                match call_stack.pop() {
                    Some(caller) => {
                        ip = caller;
                    }
                    None => return Ok(0),
                }
            }
            Instruction::ConsumeFuel(_block_fuel) => ip.add(1),
            _ => unreachable!("rwasm: unsupported instruction ({})", instr),
        }
    }
}

#[derive(Default)]
pub struct SimpleCallHandler {
    pub input: Vec<u8>,
    pub state: u32,
    pub output: Vec<u8>,
}

impl SimpleCallHandler {
    fn fn_proc_exit(
        &self,
        sp: &mut ValueStackPtr,
        _global_memory: &mut MemoryEntity,
    ) -> Result<(), RwasmError> {
        let exit_code = sp.pop();
        Err(RwasmError::ExecutionHalted(exit_code.as_i32()))
    }

    fn fn_state(
        &self,
        sp: &mut ValueStackPtr,
        _global_memory: &mut MemoryEntity,
    ) -> Result<(), RwasmError> {
        sp.push(UntypedValue::from(self.state));
        Ok(())
    }

    fn fn_write_output(
        &mut self,
        sp: &mut ValueStackPtr,
        global_memory: &mut MemoryEntity,
    ) -> Result<(), RwasmError> {
        let (offset, length) = sp.pop2();
        let buffer = global_memory
            .data()
            .get(offset.as_usize()..(offset.as_usize() + length.as_usize()))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        self.output.extend_from_slice(buffer);
        Ok(())
    }
}

impl ExternalCallHandler for SimpleCallHandler {
    fn call_function(
        &mut self,
        func_idx: u32,
        sp: &mut ValueStackPtr,
        global_memory: &mut MemoryEntity,
    ) -> Result<(), RwasmError> {
        match func_idx {
            0x01 => self.fn_proc_exit(sp, global_memory),
            0x02 => self.fn_state(sp, global_memory),
            0x05 => self.fn_write_output(sp, global_memory),
            _ => unreachable!("rwasm: unknown function ({})", func_idx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::from_utf8;

    #[allow(unused)]
    fn trace_rwasm(rwasm_bytecode: &[u8]) {
        let rwasm_module = RwasmModule::new(rwasm_bytecode).unwrap();
        let mut func_length = 0usize;
        let mut expected_func_length = rwasm_module
            .func_section
            .first()
            .copied()
            .unwrap_or(u32::MAX) as usize;
        let mut func_index = 0usize;
        println!("\n -- function #{} -- ", func_index);
        for (i, instr) in rwasm_module.code_section.instr.iter().enumerate() {
            println!("{:02}: {:?}", i, instr);
            func_length += 1;
            if func_length == expected_func_length {
                func_index += 1;
                expected_func_length = rwasm_module
                    .func_section
                    .get(func_index)
                    .copied()
                    .unwrap_or(u32::MAX) as usize;
                if expected_func_length != u32::MAX as usize {
                    println!("\n -- function #{} -- ", func_index);
                }
                func_length = 0;
            }
        }
        println!("\n")
    }

    #[test]
    fn test_execute_rwasm_bytecode() {
        let greeting_rwasm = include_bytes!("../../../tests/greeting.rwasm");
        // trace_rwasm(greeting_rwasm);
        let mut call_handler = SimpleCallHandler::default();
        let exit_code = execute_rwasm_bytecode(greeting_rwasm, Some(&mut call_handler)).unwrap();
        assert_eq!(exit_code, 0);
        let utf8_output = from_utf8(&call_handler.output).unwrap();
        assert_eq!(utf8_output, "Hello, World");
    }
}
