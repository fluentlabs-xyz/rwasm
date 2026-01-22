use crate::{
    mem::{MemoryLocalEvent, MemoryReadRecord, MemoryWriteRecord},
    Opcode,
};

impl Opcode {
    pub fn opcode_stack_read(self) -> u32 {
        if self.is_with_zero_params() {
            0
        } else if self.is_with_one_param() {
            1
        } else if self.is_with_two_params() {
            2
        } else {
            // In the case of three parameters, we read two in the general read and the third separately.
            2
        }
    }

    pub fn opcode_stack_write(self) -> bool {
        self.has_result()
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
