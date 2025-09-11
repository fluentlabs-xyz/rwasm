use crate::{
    mem::{MemoryLocalEvent, MemoryReadRecord, MemoryWriteRecord},
    Opcode,
};
use hashbrown::HashMap;
impl Opcode {
    pub fn opcode_stack_read(self) -> u32 {
        if self.is_binary_instruction() {
            return 2;
        } else if self.is_unary_instruction() {
            return 1;
        } else if self.is_nullary() {
            return 0;
        } else if self.is_memory_load_instruction() {
            return 1;
        } else if self.is_memory_store_instruction() {
            return 2;
        } else if let Opcode::LocalTee(_) = self {
            return 1;
        } else if let Opcode::LocalSet(_) = self {
            return 1;
        } else if self.is_branch_instruction() {
            if let Opcode::BrIfEqz(_) = self {
                return 1;
            } else if let Opcode::BrIfNez(_) = self {
                return 1;
            } else if let Opcode::BrTable(_) = self {
                return 1;
            }
        } else if self.is_table_instruction() {
            if let Opcode::TableGrow(_) = self {
                return 2;
            }
        }
        0
    }
    pub fn opcode_stack_write(self) -> bool {
        if self.is_binary_instruction() || self.is_unary_instruction() | self.is_const_instruction()
        {
            return true;
        }
        if self.is_binary_instruction() {
            return false;
        }
        if self.is_memory_instruction() {
            if self.is_memory_load_instruction() {
                true
            } else {
                false
            }
        } else if let Opcode::LocalGet(_) = self {
            true
        } else if let Opcode::TableGrow(_) = self {
            true
        } else {
            false
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
pub enum FatOpEvent {
    TableInit(TableInitEvent),
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
pub struct TableInitEvent {
    pub clk: u32,
    pub shard: u32,
    pub sp: u32,
    pub next_sp: u32,
    pub d: u32,
    pub s: u32,
    pub n: u32,
    pub table_idx: u32,
    pub stack_access: [MemoryReadRecord; 3],
    pub memory_read_access: Vec<MemoryReadRecord>,
    pub memory_write_acess: Vec<MemoryWriteRecord>,
    //If a memory addr is nenver touched by cpu it will ended up here.
    pub local_mem_access: Vec<MemoryLocalEvent>,
}
