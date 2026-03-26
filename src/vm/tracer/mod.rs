use super::ValueStackPtr;
use crate::{
    types::{Opcode, TableIdx},
    vm::tracer::{
        mem::{
            MemoryAccessRecord, MemoryLocalEvent, MemoryReadRecord, MemoryRecord, MemoryRecordEnum,
            MemoryWriteRecord,
        },
        state::VMState,
    },
    UntypedValue,
};
use alloc::{string::String, vec::Vec};
use core::{
    fmt::{Debug, Formatter},
    mem::take,
};
use event::{opcode_stack_read, opcode_stack_write};
use hashbrown::HashMap;

pub mod event;
pub mod mem;
pub mod mem_index;
pub mod state;

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
    pub value: u32,
    pub memory_changes: Vec<TracerMemoryState>,
    pub table_changes: Vec<TraceTableState>,
    pub table_size_changes: Vec<TraceTableSizeState>,
    pub next_table_idx: Option<TableIdx>,
    pub call_id: u32,
    pub memory_access: MemoryAccessRecord,
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
    pub nested_calls: u32,
    pub memory_records: HashMap<u32, MemoryRecord>,
    pub local_memory_event: HashMap<u32, MemoryLocalEvent>,
    pub state: VMState,
    pub ip_max: u64,
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

    pub fn pre_opcode_state(&mut self, program_counter: u32, sp: ValueStackPtr, opcode: Opcode) {
        // TODO(wangyao): "determine clk and shard here using counters,will do it in post opcode"
        let memory_changes = take(&mut self.memory_changes);
        let table_changes = take(&mut self.table_changes);
        let table_size_changes = take(&mut self.table_size_changes);
        let memory_access = self.record_mr(opcode, sp.to_relative_address());
        let opcode_state = TracerInstrState {
            program_counter,
            opcode,
            value: opcode.aux_value(),
            memory_changes,
            table_changes,
            table_size_changes,
            next_table_idx: None,
            call_id: 0,
            memory_access,
        };
        println!("opcode _state{:?},", opcode_state);
        self.logs.push(opcode_state);
    }

    pub fn post_opcode_state(
        &mut self,
        next_program_counter: u32,
        new_sp: u32,
        stack: Vec<UntypedValue>,
    ) {
        let op_state = self.logs.last().unwrap();
        let opcode = op_state.opcode;

        self.record_mw(opcode, new_sp, stack);
        self.state.sp = new_sp;
    }

    pub fn remember_next_table(&mut self, table_idx: TableIdx) {
        self.logs.last_mut().map(|v| {
            v.next_table_idx = Some(table_idx);
        });
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

    pub fn record_mr(&mut self, ins: Opcode, sp: u32) -> MemoryAccessRecord {
        let length = opcode_stack_read(ins);
        let mut memory_access = MemoryAccessRecord::default();
        println!(
            "op:{},length:{},memory_record{:?}",
            ins, length, self.memory_records
        );

        for idx in length..0 {
            let addr = sp - idx;
            println!("length in loop{},addr:{}", length, addr);
            let record = self.memory_records.entry(addr).or_insert(MemoryRecord {
                value: 0,
                shard: 0,
                timestamp: 0,
            });

            let prev_record = *record;
            record.shard = self.state.shard;
            record.timestamp = self.state.clk;
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
        }
        memory_access
    }

    pub fn record_mw(&mut self, ins: Opcode, sp: u32, stack: Vec<UntypedValue>) {
        if !opcode_stack_write(ins) {
            return;
        }
        let record = self.memory_records.entry(sp).or_default();
        let prev_record = *record;
        record.shard = self.state.shard;
        record.timestamp = self.state.clk;
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
        let op_state = self.logs.last_mut().unwrap();

        op_state.memory_access.c = Some(MemoryRecordEnum::Write(write_record));
        println!("op_state:memoeryaccess:{:?}", op_state.memory_access);
    }
}
