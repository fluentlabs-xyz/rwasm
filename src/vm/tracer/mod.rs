use super::ValueStackPtr;
use crate::{
    event::{FatOpEvent, TableInitEvent},
    mem_index::{TypedAddress, GLOBAL_MEM_START, UNIT},
    types::Opcode,
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
    cmp,
    fmt::{Debug, Formatter},
    mem::take,
};
use fnv::FnvBuildHasher;
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
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub enum CallType {
    Call,
    CallInternal,
    CallIndirect,
    Return,
}
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub struct TraceCallData {
    pub calltype: CallType,
    pub table_id: u32,
    pub table_idx: u32,
    pub func_ref: u32,
    pub signature_id: u32,
}

#[derive(Debug, Clone)]
pub struct TracerInstrState {
    pub clk: u32,
    pub pc: u32,
    pub next_pc: u32,
    pub opcode: Opcode,
    pub sp: u32,
    pub next_sp: u32,
    pub call_sp: u32,
    pub next_call_sp: u32,
    pub call_id: u32,
    pub memory_access: MemoryAccessRecord,
    pub arg1: u32,

    pub arg2: u32,
    pub res: u32,

    pub call_state: Option<TraceCallData>,
    pub fat_op: Option<FatOpEvent>,
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
    pub memory_records: HashMap<u32, MemoryRecord, fnv::FnvBuildHasher>,
    pub local_memory_event: HashMap<u32, MemoryLocalEvent, fnv::FnvBuildHasher>,
    pub state: VMState,
    pub ip_max: u64,
    // We need once generate all memory record for elements and data segements before execution.
    pub is_memory_inited: bool,
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
            call_sp: self.state.call_sp,
            next_call_sp: self.state.call_sp,
            call_id: 0,
            memory_access: MemoryAccessRecord::default(),
            sp,
            next_sp: 0,
            arg1: 0,
            arg2: 0,
            res: 0,
            call_state: None,
            fat_op: None,
        };
        let memory_access = self.record_mr(opcode, sp);

        if let Some(memory_read_record) = memory_access.arg1_record {
            opcode_state.arg1 = memory_read_record.value();
        }
        if let Some(memory_read_record) = memory_access.arg2_record {
            opcode_state.arg2 = memory_read_record.value();
        }

        if let Opcode::BrTable(_) = opcode {
            opcode_state.arg2 = opcode.aux_value();
        }

        if opcode.is_local_instruction() {
            opcode_state.arg2 = opcode.aux_value();
        }
        println!("op_code_state:{:?}", opcode_state);

        opcode_state.memory_access = memory_access;
        if opcode == Opcode::Return && self.state.call_sp != 0 {
            let call_state = TraceCallData {
                calltype: CallType::Return,
                table_id: 0,
                table_idx: 0,
                func_ref: 0,
                signature_id: 0,
            };
            opcode_state.call_state = Some(call_state);
            opcode_state.next_call_sp = self.state.call_sp - 1;
        }
        if opcode.is_table_instruction() {
            if let Opcode::TableInit(_) = opcode {
                let mut fat_op_event = TableInitEvent::default();
                let mut local_memory_access = HashMap::default();
                for idx in 0..3 {
                    let addr = sp + idx * UNIT;
                    println!("idx:{},addr:{}", idx, addr);
                    let read_record =
                        self.mr_with_local_access(addr, Some(&mut local_memory_access));

                    match idx {
                        2 => {
                            fat_op_event.stack_access[0] = read_record;
                            fat_op_event.d = read_record.value;
                        }
                        1 => {
                            println!("s: read_record:{:?}", read_record);
                            fat_op_event.stack_access[1] = read_record;
                            fat_op_event.s = read_record.value;
                        }
                        0 => {
                            fat_op_event.stack_access[2] = read_record;
                            fat_op_event.n = read_record.value;
                        }
                        _ => unreachable!(),
                    }
                }
                fat_op_event.clk = self.state.clk;
                fat_op_event.shard = self.state.shard;
                fat_op_event.sp = sp;
                fat_op_event.next_sp = sp + 3 * UNIT;
                fat_op_event.local_mem_access =
                    local_memory_access.iter().map(|(_, v)| (*v)).collect();
                fat_op_event.local_mem_access_addr =
                    local_memory_access.iter().map(|(k, v)| (*k)).collect();
                opcode_state.fat_op = Some(FatOpEvent::TableInit(fat_op_event));
            }
        }

        self.logs.push(opcode_state);
    }

    pub fn post_opcode_state(
        &mut self,
        next_program_counter: u32,
        new_sp: u32,
        opcode: Opcode,
        stack: Vec<UntypedValue>,
    ) {
        match opcode {
            Opcode::LocalSet(_) | Opcode::LocalTee(_) => {
                let v_addr = new_sp + opcode.aux_value() * UNIT - UNIT;
                let value = self
                    .logs
                    .last_mut()
                    .unwrap()
                    .memory_access
                    .arg1_record
                    .unwrap()
                    .value();
                println!("value:{},addr:{}", value, v_addr);
                let res_record = Some(MemoryRecordEnum::Write(self.mw(v_addr, value)));
                self.logs.last_mut().unwrap().memory_access.res_record = res_record;
                self.logs.last_mut().unwrap().res = res_record.unwrap().value();
            }
            //We are different from RISCV so that we have to send the branching offset with res
            // because we have no register to read.
            Opcode::Br(_) | Opcode::BrIfEqz(_) | Opcode::BrIfNez(_) => {
                self.logs.last_mut().unwrap().res = opcode.aux_value();
                // let fake_res_record = MemoryWriteRecord::new(opcode.aux_value(),0, 1,0,0,0);
                // self.logs.last_mut().unwrap().memory_access.
                // res_record=Some(MemoryRecordEnum::Write(fake_res_record));
            }
            Opcode::BrTable(_) => {
                let index = self.logs.last_mut().unwrap().arg1;
                let max_index = opcode.aux_value() - 1;
                let normalized_index = cmp::min(index, max_index);
                self.logs.last_mut().unwrap().res = 2 * normalized_index + 1;
                self.logs.last_mut().unwrap().arg2 = opcode.aux_value();
                //  let fake_arg2_record = MemoryReadRecord{ value: opcode.aux_value()-1, shard: 0,
                // timestamp: 0, prev_timestamp:1,prev_shard: 0 };  self.logs.
                // last_mut().unwrap().memory_access.arg2_record =
                // Some(MemoryRecordEnum::Read(fake_arg2_record));  self.logs.
                // last_mut().unwrap().arg2=opcode.aux_value()-1;
                //   let fake_res_record = MemoryWriteRecord::new(2*normalized_index+1,0, 1,0,0,0);
                //   self.logs.last_mut().unwrap().memory_access.
                // res_record=Some(MemoryRecordEnum::Write(fake_res_record));
            }
            Opcode::CallInternal(compiled_func) => {
                let old_pc = self.logs.last_mut().unwrap().pc + 1;
                let new_call_sp = self.state.call_sp + 1;
                let typed_addr = TypedAddress::FuncFrame(new_call_sp);
                let v_addr = typed_addr.to_virtual_addr();
                let write_record = self.mw(v_addr, old_pc);
                let res_record = Some(MemoryRecordEnum::Write(write_record));
                self.logs.last_mut().unwrap().memory_access.call_sp_access =
                    Some(MemoryRecordEnum::Write(write_record));
                self.logs.last_mut().unwrap().call_state = Some(TraceCallData {
                    calltype: CallType::CallInternal,
                    table_id: 0,
                    table_idx: 0,
                    func_ref: opcode.aux_value(),
                    signature_id: 0,
                });
                self.logs.last_mut().unwrap().next_call_sp = new_call_sp;
                self.state.call_sp = new_call_sp;
            }
            Opcode::Return => {
                if self.logs.last_mut().unwrap().call_sp != 0 {
                    self.state.call_sp = self.logs.last_mut().unwrap().call_sp - 1;
                    self.logs.last_mut().unwrap().next_call_sp = self.state.call_sp;
                }
            }
            _ => self.record_sw(opcode, new_sp, stack),
        }
        if opcode.is_fat_op() {
            let main_op_event = self.logs.last().unwrap();

            let sub_op_event = self.make_sub_op_event(main_op_event.clone());
        }
        self.state.sp = new_sp;

        self.state.next_cycle();
        if opcode.is_fat_op() {
            self.state.next_cycle();
            self.state.next_cycle();
        }
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

    pub fn make_sub_op_event(&mut self, main_op_log: TracerInstrState) -> TracerInstrState {
        let sub_op = {
            match main_op_log.opcode {
                Opcode::TableInit(_) => {
                    if let FatOpEvent::TableInit(table_init_event) = main_op_log.fat_op.unwrap() {
                        Opcode::TableGet(table_init_event.table_idx as u16)
                    } else {
                        unreachable!()
                    }
                }
                _ => unreachable!(),
            }
        };
        TracerInstrState {
            clk: main_op_log.clk + 2,
            pc: main_op_log.pc + 1,
            opcode: sub_op,
            sp: main_op_log.next_sp,
            next_sp: main_op_log.next_sp,
            call_sp: main_op_log.next_call_sp,
            next_call_sp: main_op_log.next_call_sp,
            call_id: 0,
            memory_access: MemoryAccessRecord::default(),
            arg1: 0,
            arg2: 0,
            res: 0,
            call_state: None,
            fat_op: None,
            next_pc: main_op_log.next_pc + 1,
        }
    }

    pub fn record_mr(&mut self, ins: Opcode, sp: u32) -> MemoryAccessRecord {
        let length = ins.opcode_stack_read();
        let mut memory_access = MemoryAccessRecord::default();
        println!("length:{:?},sp:{},op:{}", length, sp, ins);

        for idx in 0..length {
            let addr = sp + idx * UNIT;
            println!("idx:{},addr:{}", idx, addr);
            let read_record = self.mr(addr);

            match idx {
                1 => {
                    println!("arg1:read_record:{:?}", read_record);
                    memory_access.arg1_record = Some(MemoryRecordEnum::Read(read_record));
                    memory_access.arg1_addr=Some(TypedAddress::Stack(idx))
                }
                0 => match length {
                    2 => {
                        println!("arg2:load:read_record:{:?}", read_record);
                        memory_access.arg2_record = Some(MemoryRecordEnum::Read(read_record));
                         memory_access.arg2_addr=Some(TypedAddress::Stack(idx))
                    }
                    1 => {
                        println!("arg1:load:read_record:{:?}", read_record);
                        memory_access.arg1_record = Some(MemoryRecordEnum::Read(read_record));
                         memory_access.arg1_addr=Some(TypedAddress::Stack(idx))
                    }
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }
        }

        if ins.is_memory_load_instruction() {
            let offset = ins.aux_value();
            let raw_addr = memory_access.arg1_record.unwrap().value();
            println!("rawaddr load:{}", raw_addr);
            let aligned_addr = align(raw_addr.wrapping_add(offset));
            let typed_addr = TypedAddress::GlobalMemory(aligned_addr);
            let read_record = self.mr(typed_addr.to_virtual_addr());
            println!(
                "load:addr{},read_record:{:?}",
                typed_addr.to_virtual_addr(),
                read_record
            );
            memory_access.memory = Some(MemoryRecordEnum::Read(read_record));
            if is_multi_align(ins, raw_addr.wrapping_add(offset)) {
                let typed_addr_hi = TypedAddress::GlobalMemory(aligned_addr + UNIT);
                let read_record_hi = self.mr(typed_addr_hi.to_virtual_addr());
                memory_access.memory_hi = Some(MemoryRecordEnum::Read(read_record_hi));
            }
        }

        if let Opcode::LocalGet(_) = ins {
            println!("sp:{},aux_val:{}", sp, ins.aux_value());
            let v_addr = sp + ins.aux_value() * UNIT - UNIT;

            println!("localgetaddr:{}", v_addr);
            let read_record = self.mr(v_addr);
            memory_access.arg1_record = Some(MemoryRecordEnum::Read(read_record));
        }

        if let Opcode::Return = ins {
            if self.state.call_sp != 0 {
                let typed_addr = TypedAddress::FuncFrame(self.state.call_sp);
                let read_record = self.mr(typed_addr.to_virtual_addr());
                memory_access.call_sp_access = Some(MemoryRecordEnum::Read(read_record));
                self.state.call_sp -= 1;
            }
        }

        memory_access
    }

    pub fn record_sw(&mut self, ins: Opcode, sp: u32, stack: Vec<UntypedValue>) {
        println!("opcode:{},write to stack?{}", ins, ins.opcode_stack_write());
        if ins.opcode_stack_write() {
            let value = stack.last().unwrap().as_u32();
            let res_record = self.mw(sp, value);

            self.logs.last_mut().unwrap().memory_access.res_record =
                Some(MemoryRecordEnum::Write(res_record));
            self.logs.last_mut().unwrap().res = res_record.value;
        }
    }

    pub fn mr_with_local_access(
        &mut self,
        addr: u32,
        local_memory_access: Option<&mut HashMap<u32, MemoryLocalEvent, FnvBuildHasher>>,
    ) -> MemoryReadRecord {
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
        let local_memory_access = if let Some(local_memory_access) = local_memory_access {
            local_memory_access
        } else {
            &mut self.local_memory_event
        };
        let entry = local_memory_access
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

    pub fn mr(&mut self, addr: u32) -> MemoryReadRecord {
        self.mr_with_local_access(addr, None)
    }

    pub fn mw_with_local_access(
        &mut self,
        addr: u32,
        value: u32,
        local_memory_access: Option<&mut HashMap<u32, MemoryLocalEvent, FnvBuildHasher>>,
    ) -> MemoryWriteRecord {
        let record = self.memory_records.entry(addr).or_default();
        println!("addr: {}record:{:?}", addr, record);
        let prev_record = *record;
        record.shard = self.state.shard;
        record.timestamp = self.state.clk;
        record.value = value;
        let local_memory_access = if let Some(local_memory_access) = local_memory_access {
            local_memory_access
        } else {
            &mut self.local_memory_event
        };
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
    pub fn mw(&mut self, addr: u32, value: u32) -> MemoryWriteRecord {
        self.mw_with_local_access(addr, value, None)
    }
}

pub fn align(addr: u32) -> u32 {
    return addr - addr % 4;
}
// This checks whether the memory read or write need to touch multiple aligned  address.
pub fn is_multi_align(opcode: Opcode, addr: u32) -> bool {
    println!("opcode:{}addr:{}", opcode, addr);
    match opcode {
        Opcode::I32Store(_) => addr % 4 != 0,
        Opcode::I32Store16(_) => addr % 4 == 3,
        Opcode::I32Load(_) => addr % 4 != 0,
        Opcode::I32Load16S(_) => addr % 4 == 3,
        Opcode::I32Load16U(_) => addr % 4 == 3,

        _ => false,
    }
}
