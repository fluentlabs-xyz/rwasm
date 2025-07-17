use super::ValueStackPtr;
use crate::{
    mem_index::{AddressType, UNIT},
    types::Opcode,
    vm::tracer::{
        mem::{
            MemoryAccessRecord,
            MemoryLocalEvent,
            MemoryReadRecord,
            MemoryRecord,
            MemoryRecordEnum,
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
    pub clk: u32,
    pub pc: u32,
    pub next_pc: u32,
    pub opcode: Opcode,
    pub sp: u32,
    pub next_sp: u32,
    pub call_id: u32,
    pub memory_access: MemoryAccessRecord,
    pub arg1: u32,
    pub arg2: u32,
    pub res: u32,
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
        let sp = sp.to_relative_address();
        let mut opcode_state = TracerInstrState {
            clk: self.state.clk,
            pc: program_counter,
            next_pc: 0,
            opcode,
            call_id: 0,
            memory_access: MemoryAccessRecord::default(),
            sp,
            next_sp: 0,
            arg1: 0,
            arg2: 0,
            res: 0,
        };
        let memory_access = self.record_mr(opcode, sp);

        if let Some(memory_read_record) = memory_access.arg1_record {
            opcode_state.arg1 = memory_read_record.value();
        }
        if let Some(memory_read_record) = memory_access.arg2_record {
            opcode_state.arg2 = memory_read_record.value();
        }
        if opcode.is_branch_instruction() {
            opcode_state.arg2 = opcode.aux_value();
        }
        println!("op_code_state:{:?}", opcode_state);

        opcode_state.memory_access = memory_access;

        self.logs.push(opcode_state);
    }

    pub fn post_opcode_state(
        &mut self,
        next_program_counter: u32,
        new_sp: u32,
        opcode: Opcode,
        stack: Vec<UntypedValue>,
    ) {
        if let Opcode::LocalSet(offset) = opcode {
            let v_addr = AddressType::Stack(opcode.aux_value()).to_virtual_addr();
            let value = self
                .logs
                .last_mut()
                .unwrap()
                .memory_access
                .arg1_record
                .unwrap()
                .value();
            let res_record = Some(MemoryRecordEnum::Write(self.mw(v_addr, value)));
            self.logs.last_mut().unwrap().memory_access.memory = res_record;
            self.logs.last_mut().unwrap().res = res_record.unwrap().value();
        } else {
            self.record_sw(opcode, new_sp, stack);
        }
        self.state.sp = new_sp;
    }

    // pub fn global_variable(&mut self, value: UntypedValue, index: u32) {
    //     self.global_variables.push(TracerGlobalVariable {
    //         value: value.to_bits(),
    //         index,
    //     })
    // }

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
        let length = ins.opcode_stack_read();
        let mut memory_access = MemoryAccessRecord::default();
        println!("length:{:?},", length);
        for idx in 0..length {
            let addr = sp + idx * UNIT;
            println!("idx:{},addr:{}", idx, addr);
            let read_record = self.mr(addr);

            match idx {
                1 => {
                    println!("arg1:read_record:{:?}", read_record);
                    memory_access.arg1_record = Some(MemoryRecordEnum::Read(read_record));
                }
                0 => {
                    match length{
                        2=>{
                            println!("arg2:load:read_record:{:?}", read_record);
                    memory_access.arg2_record = Some(MemoryRecordEnum::Read(read_record));
                        }
                        1=>{
                             println!("arg1:load:read_record:{:?}", read_record);
                    memory_access.arg1_record = Some(MemoryRecordEnum::Read(read_record));
                        }
                         _=>unreachable!()
                    }
                   
                    
                }
                _ => unreachable!(),
            }
        }

        if ins.is_memory_load_instruction() {
            let offset = ins.aux_value();
            let raw_addr = memory_access.arg1_record.unwrap().value();
            let aligned_addr = align(raw_addr + offset);
            let read_record = self.mr(aligned_addr);
            println!("load:read_record:{:?}", read_record);
            memory_access.memory = Some(MemoryRecordEnum::Read(read_record));
        }

        if let Opcode::LocalGet(_) = ins {
            let v_addr = AddressType::Stack(ins.aux_value()).to_virtual_addr();
            let read_record = self.mr(v_addr);
            memory_access.arg1_record = Some(MemoryRecordEnum::Read(read_record));
        }

        memory_access
    }

    pub fn record_sw(&mut self, ins: Opcode, sp: u32, stack: Vec<UntypedValue>) {
        if ins.opcode_stack_write() {
            let value = stack.last().unwrap().as_u32();
            let res_record = self.mw(sp, value);

            self.logs.last_mut().unwrap().memory_access.res_record =
                Some(MemoryRecordEnum::Write(res_record));
            self.logs.last_mut().unwrap().res = res_record.value;
        }
    }

    pub fn mr(&mut self, addr: u32) -> MemoryReadRecord {
        let clk = self.state.clk;
        let shard = self.state.shard;
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
        MemoryReadRecord::new(
            record.value,
            record.shard,
            record.timestamp,
            prev_record.shard,
            prev_record.timestamp,
        )
    }
    pub fn mw(&mut self, addr: u32, value: u32) -> MemoryWriteRecord {
        assert!(addr % 4 == 0);
        let record = self.memory_records.entry(addr).or_default();

        let prev_record = *record;
        record.shard = self.state.shard;
        record.timestamp = self.state.clk;
        record.value = value;
        let local_memory_access = &mut self.local_memory_event;
        local_memory_access
            .entry(addr)
            .and_modify(|e| {
                e.final_mem_access = *record;
            })
            .or_insert(MemoryLocalEvent {
                addr: addr,
                initial_mem_access: prev_record,
                final_mem_access: *record,
            });
        // Construct the memory write record.
        MemoryWriteRecord::new(
            record.value,
            record.shard,
            record.timestamp,
            prev_record.value,
            prev_record.shard,
            prev_record.timestamp,
        )
    }
}

pub fn align(addr: u32) -> u32 {
    return addr - addr % 4;
}
