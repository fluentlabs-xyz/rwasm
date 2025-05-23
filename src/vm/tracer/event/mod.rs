use crate::{vm::opcodes, Opcode};

pub mod memory;

pub fn opcode_stack_read(ins: Opcode) -> u32 {
    if ins.is_binary_instruction() {
        return 2;
    } else if ins.is_unary_instruction() {
        return 1;
    } else if ins.is_nullary() {
        return 0;
    } else if ins.is_memory_load_instruction() {
        return 1;
    } else if ins.is_memory_store_instruction() {
        return 2;
    } else if ins.is_branch_instruction() {
    }
    return 0;
}

pub fn opcode_stack_write(op: Opcode) -> bool {
    if op.is_binary_instruction() || op.is_unary_instruction() {
        return true;
    }
    if op.is_binary_instruction() {
        return false;
    }
    if op.is_memory_instruction() {
        if op.is_memory_load_instruction() {
            return true;
        } else {
            return false;
        }
    }
    return false;
}
