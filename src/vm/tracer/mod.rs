use crate::{
    types::{Opcode, OpcodeMeta, TableIdx},
    OpcodeData,
    UntypedValue,
};
use alloc::{string::String, vec::Vec};
use core::{
    fmt::{Debug, Formatter},
    mem::take,
};
use downcast_rs::Downcast;
use event::{
    memory::{MemoryRecord, MemoryRecordEnum},
    opcode_stack_read,
    opcode_stack_write,
};
use hashbrown::{hash_map::Entry, HashMap};

pub mod event;
use super::{ValueStack, ValueStackPtr};
use event::memory::*;

#[derive(Debug, Clone)]
pub struct TracerMemoryState {
    pub offset: u32,
    pub len: u32,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TraceTableState {
    pub table_idx: u32,
    pub elem_idx: u32,
    pub func_ref: UntypedValue,
}

#[derive(Debug, Clone)]
pub struct TraceTableSizeState {
    pub table_idx: u32,
    pub init: u32,
    pub delta: u32,
}

#[derive(Debug, Clone)]
pub struct TracerInstrState {
    pub program_counter: u32,
    pub opcode: Opcode,
    pub value: OpcodeData,
    pub memory_changes: Vec<TracerMemoryState>,
    pub table_changes: Vec<TraceTableState>,
    pub table_size_changes: Vec<TraceTableSizeState>,
    pub next_table_idx: Option<TableIdx>,
    pub call_id: u32,
}

#[derive(Default, Debug, Clone)]
pub struct TracerFunctionMeta {
    pub fn_index: u32,
    pub max_stack_height: u32,
    pub num_locals: u32,
    pub fn_name: String,
}

#[derive(Default, Clone)]
pub struct TracerGlobalVariable {
    pub index: u32,
    pub value: u64,
}

#[derive(Default, Clone)]
pub struct Tracer {
    pub global_memory: Vec<TracerMemoryState>,
    pub logs: Vec<TracerInstrState>,
    pub memory_changes: Vec<TracerMemoryState>,
    pub table_changes: Vec<TraceTableState>,
    pub table_size_changes: Vec<TraceTableSizeState>,
    pub fns_meta: Vec<TracerFunctionMeta>,
    pub global_variables: Vec<TracerGlobalVariable>,
    pub extern_names: HashMap<u32, String>,
    pub nested_calls: u32,
    pub memory_records: HashMap<u32, MemoryRecord>,
    pub local_memory_event: HashMap<u32, MemoryLocalEvent>,
    pub memory_accesseveny: Vec<MemoryAccessRecord>,
    pub cycle_memory_access: Vec<MemoryAccessRecord>,
}

impl Debug for Tracer {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "global_memory: {:?}; logs: {:?}; memory_changes: {:?}; fns_meta: {:?}",
            self.global_memory, self.logs, self.memory_changes, self.fns_meta
        )
    }
}

impl Tracer {
    pub fn merge_nested_call(&mut self, tracer: &Tracer) {
        self.nested_calls += 1;
        for mut log in tracer.logs.iter().cloned() {
            log.call_id = self.nested_calls;
            self.logs.push(log);
        }
    }

    pub fn global_memory(&mut self, offset: u32, len: u32, memory: &[u8]) {
        self.global_memory.push(TracerMemoryState {
            offset,
            len,
            data: Vec::from(memory),
        });
    }

    pub fn pre_opcode_state(
        &mut self,
        program_counter: u32,
        sp: ValueStackPtr,
        shard: u32,
        clk: u32,
        opcode: Opcode,
        value: OpcodeData,
    ) {
        let memory_changes = take(&mut self.memory_changes);
        let table_changes = take(&mut self.table_changes);
        let table_size_changes = take(&mut self.table_size_changes);
        self.record_mr(opcode, sp.to_position(), shard, clk);
        let opcode_state = TracerInstrState {
            program_counter,
            opcode,
            value,
            memory_changes,
            table_changes,
            table_size_changes,
            next_table_idx: None,
            call_id: 0,
        };
        self.logs.push(opcode_state.clone());
    }

    pub fn post_opcode_state(
        &mut self,
        program_counter: u32,
        opcode: Opcode,
        sp: u32,
        shard: u32,
        clk: u32,
        stack: Vec<UntypedValue>,
    ) {
        self.record_mw(opcode, sp, shard, clk, stack);
    }

    pub fn remember_next_table(&mut self, table_idx: TableIdx) {
        self.logs.last_mut().map(|v| {
            v.next_table_idx = Some(table_idx);
        });
    }

    pub fn function_call(
        &mut self,
        fn_index: u32,
        max_stack_height: usize,
        num_locals: usize,
        fn_name: String,
    ) {
        let resolved_name = self.extern_names.get(&fn_index).unwrap_or(&fn_name);
        self.fns_meta.push(TracerFunctionMeta {
            fn_index,
            max_stack_height: max_stack_height as u32,
            num_locals: num_locals as u32,
            fn_name: resolved_name.clone(),
        })
    }

    pub fn global_variable(&mut self, value: UntypedValue, index: u32) {
        self.global_variables.push(TracerGlobalVariable {
            value: value.to_bits(),
            index,
        })
    }

    pub fn memory_change(&mut self, offset: u32, len: u32, memory: &[u8]) {
        self.memory_changes.push(TracerMemoryState {
            offset,
            len,
            data: Vec::from(memory),
        });
    }

    pub fn table_change(&mut self, table_idx: u32, elem_idx: u32, func_ref: UntypedValue) {
        self.table_changes.push(TraceTableState {
            table_idx,
            elem_idx,
            func_ref,
        });
    }

    pub fn table_size_change(&mut self, table_idx: u32, init: u32, delta: u32) {
        self.table_size_changes.push(TraceTableSizeState {
            table_idx,
            init,
            delta,
        });
    }

    pub fn record_mr(&mut self, ins: Opcode, sp: u32, shard: u32, clk: u32) {
        let length = opcode_stack_read(ins);

        for idx in length..0 {
            let addr = sp - length + 1;
            let record = self
                .memory_records
                .entry(sp - length)
                .or_insert(MemoryRecord {
                    value: 0,
                    shard: 0,
                    timestamp: 0,
                });

            let prev_record = *record;
            record.shard = shard;
            record.timestamp = clk;
            let local_memory_access = &mut self.local_memory_event;
            local_memory_access
                .entry(addr)
                .and_modify(|e| {
                    e.final_mem_access = *record;
                })
                .or_insert(MemoryLocalEvent {
                    addr,
                    initial_mem_access: prev_record,
                    final_mem_access: *record,
                });
            // Construct the memory read record.
            let mut memory_access = MemoryAccessRecord::default();
            let read_record = MemoryReadRecord::new(
                record.value,
                record.shard,
                record.timestamp,
                prev_record.shard,
                prev_record.timestamp,
            );
            match idx {
                1 => {
                    memory_access.b = Some(MemoryRecordEnum::Read(read_record));
                }
                0 => {
                    memory_access.a = Some(MemoryRecordEnum::Read(read_record));
                }
                _ => unreachable!(),
            }
            self.cycle_memory_access.push(memory_access);
        }
    }

    pub fn record_mw(
        &mut self,
        ins: Opcode,
        sp: u32,
        shard: u32,
        clk: u32,
        stack: Vec<UntypedValue>,
    ) {
        if opcode_stack_write(ins) {
            let record = self.memory_records.entry(sp).or_default();
            let prev_record = *record;
            record.shard = shard;
            record.timestamp = clk;
            record.value = stack.last().unwrap().as_u32();
            let local_memory_access = &mut self.local_memory_event;
            local_memory_access
                .entry(sp)
                .and_modify(|e| {
                    e.final_mem_access = *record;
                })
                .or_insert(MemoryLocalEvent {
                    addr: sp,
                    initial_mem_access: prev_record,
                    final_mem_access: *record,
                });
            // Construct the memory write record.

            let write_record = MemoryWriteRecord::new(
                record.value,
                record.shard,
                record.timestamp,
                prev_record.value,
                prev_record.shard,
                prev_record.timestamp,
            );

            let memory_access = self.cycle_memory_access.last_mut();
            memory_access.unwrap().c = Some(MemoryRecordEnum::Write(write_record));
        }
    }
}
