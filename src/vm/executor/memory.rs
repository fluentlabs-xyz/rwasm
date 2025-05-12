use crate::{Instruction, Opcode, RwasmExecutor, TrapCode, UntypedValue};

#[inline(always)]
pub(crate) fn exec_memory_load_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    instr: Instruction,
) -> Result<(), TrapCode> {
    use Opcode::*;
    let (opcode, offset) = match instr {
        Instruction::AddressOffset(opcode, value) => (opcode, value),
        _ => unreachable!("rwasm: missing instr data"),
    };
    let load_extend = match opcode {
        I32Load => UntypedValue::i32_load,
        // I64Load => UntypedValue::i64_load,
        I32Load8S => UntypedValue::i32_load8_s,
        I32Load8U => UntypedValue::i32_load8_u,
        I32Load16S => UntypedValue::i32_load16_s,
        I32Load16U => UntypedValue::i32_load16_u,
        // I64Load8S => UntypedValue::i64_load8_s,
        // I64Load8U => UntypedValue::i64_load8_u,
        // I64Load16S => UntypedValue::i64_load16_s,
        // I64Load16U => UntypedValue::i64_load16_u,
        // I64Load32S => UntypedValue::i64_load32_s,
        // I64Load32U => UntypedValue::i64_load32_u,
        _ => unreachable!(),
    };
    vm.sp.try_eval_top(|address| {
        let memory = vm.global_memory.data();
        let value = load_extend(memory, address, offset.into_inner())?;
        Ok(value)
    })?;
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn exec_memory_store_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    instr: Instruction,
) -> Result<(), TrapCode> {
    use Opcode::*;
    let (opcode, offset) = match instr {
        Instruction::AddressOffset(opcode, value) => (opcode, value),
        _ => unreachable!("rwasm: missing instr data"),
    };
    let (address, value) = vm.sp.pop2();
    let memory = vm.global_memory.data_mut();
    let store_wrap = match opcode {
        I32Store => UntypedValue::i32_store,
        // I64Store => UntypedValue::i64_store,
        I32Store8 => UntypedValue::i32_store8,
        I32Store16 => UntypedValue::i32_store16,
        // I64Store8 => UntypedValue::i64_store8,
        // I64Store16 => UntypedValue::i64_store16,
        // I64Store32 => UntypedValue::i64_store32,
        _ => unreachable!(),
    };
    store_wrap(memory, address, offset.into_inner(), value)?;
    #[cfg(feature = "tracing")]
    if let Some(tracer) = vm.tracer.as_mut() {
        let address = u32::from(address);
        let base_address = offset.into_inner() + address;
        let len = match opcode {
            I32Store => 4,
            I32Store16 => 2,
            I32Store8 => 1,
            // I32Store | I64Store32 => 4,
            // I64Store => 8,
            // I32Store16 | I64Store16 => 2,
            // I32Store8 | I64Store8 => 1,
            _ => unreachable!(),
        };
        tracer.memory_change(
            base_address,
            len,
            &memory[base_address as usize..(base_address + len) as usize],
        );
    }
    vm.ip.add(1);
    Ok(())
}
