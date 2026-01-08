use crate::{
    mem::{MemoryLocalEvent, MemoryReadRecord, MemoryWriteRecord},
    Opcode,
};

impl Opcode {
    pub fn opcode_stack_read(self) -> u32 {
        if self.is_nullary()
            || matches!(
                self,
                Opcode::LocalGet(_)
                    | Opcode::GlobalGet(_)
                    | Opcode::MemorySize
                    | Opcode::TableSize(_)
            )
        {
            return 0;
        }

        if self.is_unary_instruction()
            || self.is_memory_load_instruction()
            || matches!(
                self,
                Opcode::LocalSet(_)
                    | Opcode::LocalTee(_)
                    | Opcode::GlobalSet(_)
                    | Opcode::Drop
                    | Opcode::CallIndirect(_)
                    | Opcode::ReturnCallIndirect(_)
                    | Opcode::ConsumeFuelStack
                    | Opcode::StackCheck(_)
                    | Opcode::MemoryGrow
                    | Opcode::TableGet(_)
                    | Opcode::BrIfEqz(_)
                    | Opcode::BrIfNez(_)
                    | Opcode::BrTable(_)
            )
        {
            return 1;
        }

        if self.is_binary_instruction()
            || self.is_memory_store_instruction()
            || self.is_64b_op()
            || matches!(self, Opcode::TableSet(_) | Opcode::TableGrow(_))
        {
            return 2;
        }

        if matches!(
            self,
            Opcode::Select
                | Opcode::MemoryFill
                | Opcode::MemoryCopy
                | Opcode::MemoryInit(_)
                | Opcode::TableFill(_)
                | Opcode::TableCopy(_, _)
                | Opcode::TableInit(_)
        ) {
            return 3;
        }

        0
    }

    pub fn opcode_stack_write(self) -> bool {
        self.is_binary_instruction()
            || self.is_unary_instruction()
            || self.is_const_instruction()
            || self.is_memory_load_instruction()
            || self.is_64b_op()
            || matches!(
                self,
                Opcode::LocalGet(_)
                    | Opcode::LocalTee(_)
                    | Opcode::GlobalGet(_)
                    | Opcode::Select
                    | Opcode::MemorySize
                    | Opcode::MemoryGrow
                    | Opcode::TableSize(_)
                    | Opcode::TableGet(_)
                    | Opcode::TableGrow(_)
            )
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
pub enum FatOpEvent {
    TableInit(TableInitEvent),
    TableGrow(TableGrowEvent),
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
    pub local_mem_access_addr: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
pub struct TableGrowEvent {
    pub clk: u32,
    pub shard: u32,
    pub sp: u32,
    pub next_sp: u32,
    pub delta: u32,
    pub init: u32,
    pub table_idx: u32,
    pub stack_access: [MemoryReadRecord; 2],
    pub memory_write_acess: Vec<MemoryWriteRecord>,
    pub table_size_read_acess: MemoryReadRecord,
    pub table_size_write_acess: MemoryWriteRecord,
    pub result_write_access: MemoryWriteRecord,
    //If a memory addr is nenver touched by cpu it will ended up here.
    pub local_mem_access: Vec<MemoryLocalEvent>,
    pub local_mem_access_addr: Vec<u32>,
}
