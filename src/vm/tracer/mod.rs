use super::ValueStackPtr;
use crate::{
    event::FatOpEvent,
    mem_index::{TypedAddress, UNIT},
    types::Opcode,
    vm::tracer::{
        mem::{
            MemoryAccessRecord, MemoryLocalEvent, MemoryReadRecord, MemoryRecord, MemoryRecordEnum,
            MemoryWriteRecord,
        },
        state::VMState,
    },
    SysFuncIdx, UntypedValue,
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
#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub enum CallType {
    Call,
    CallInternal,
    CallIndirect,
    Return,
}

#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Default)]
pub struct SysCallData {
    pub sys_call_id: SysFuncIdx,
    pub params: Vec<u32>,
    pub result: Vec<u32>,
    pub memory_read_access: Vec<MemoryReadRecord>,
    pub memory_write_access: Vec<MemoryWriteRecord>,
    pub local_mem_access: Vec<MemoryLocalEvent>,
    pub local_mem_access_addr: Vec<u32>,
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
    // pub arg1: u32,

    // pub arg2: u32,
    pub res: u32,
    // pub res_hi: u32,
    // pub call_state: Option<TraceCallData>,
    pub fat_op: Option<FatOpEvent>,
    pub extension: Option<InstrStateExtension>,
}

#[derive(Debug, Clone)]
pub enum InstrStateExtension {
    Local(LocalStateExtension),
    SignatureCheck(SignatureCheckStateExtension),
    Call(CallStateExtension),
    TableInit(TableInitStateExtension),
    TableGrow(TableGrowStateExtension),
    I64Alu(I64AluStateExtension),
    Memory(MemExtension),
}

#[derive(Debug, Copy, Clone)]
pub struct MemExtension {
    pub low_record: MemoryRecordEnum,
    pub upper_record: Option<MemoryRecordEnum>,
}

#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Copy, Clone)]
pub struct CallStateExtension {
    pub table_idx: u32,
    pub table_size_read: Option<MemoryReadRecord>,
    pub table_read: Option<MemoryReadRecord>,
    pub call_stack_access: MemoryRecordEnum,
    pub call_stack_address: u32,
}

#[derive(Debug, Clone)]
pub struct I64AluStateExtension {
    pub res_lo_write: MemoryWriteRecord,
}

#[derive(Debug, Clone)]
pub struct LocalStateExtension {
    pub local_depth_access: MemoryRecordEnum,
}

#[derive(Debug, Clone)]
pub struct SignatureCheckStateExtension {
    pub last_signature_check_read: MemoryRecordEnum,
}

#[derive(Debug, Clone)]
pub struct TableInitStateExtension {
    pub dst_index_record: MemoryReadRecord,
    pub table_idx: u32,
    pub element_segment_idx: u32,
    pub src_read_records: Vec<MemoryReadRecord>,
    pub dst_write_records: Vec<MemoryWriteRecord>,
}

#[derive(Debug, Clone)]
pub struct TableGrowStateExtension {
    pub dst_write_records: Vec<MemoryWriteRecord>,
    pub table_size_read_record: MemoryReadRecord,
    pub table_size_write_record: Option<MemoryWriteRecord>,
}

#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Default, Clone, Debug)]
pub struct DataOpEvent {
    pub code: u32,
    pub aux_value: u32,
    pub pc: u32,
    pub clk: u32,
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
    pub data_op_logs: Vec<DataOpEvent>,
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
            call_id: 0,
            memory_access: MemoryAccessRecord::default(),
            sp,
            next_sp: 0,
            res: 0,
            fat_op: None,
            extension: None,
        };
        opcode_state.memory_access = self.record_mr(opcode, sp);

        if matches!(opcode, Opcode::ConsumeFuel(_) | Opcode::ConsumeFuelStack) {
            let consumed_fuel_record_low = self.mr(TypedAddress::from_reserved_addr(
                ReservedAddrEnum::ConsumedFuelLow,
            )
            .to_virtual_addr());
            opcode_state.memory_access.arg1_record =
                Some(MemoryRecordEnum::Read(consumed_fuel_record_low));
            let consumed_fuel_record_hi = self.mr(TypedAddress::from_reserved_addr(
                ReservedAddrEnum::ConsumedFuelHi,
            )
            .to_virtual_addr());
            opcode_state.memory_access.arg1_hi_record =
                Some(MemoryRecordEnum::Read(consumed_fuel_record_hi));
            if opcode == Opcode::ConsumeFuelStack {
                let stack_fuel_record = self.mr(sp);
                opcode_state.memory_access.arg2_record =
                    Some(MemoryRecordEnum::Read(stack_fuel_record));
                opcode_state.memory_access.arg2_addr = Some(TypedAddress::from_stack_vaddr(sp));
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
        // Ensure clock alignment: memory writes must occur on odd cycles.
        if self.state.clk % 2 == 0 {
            self.state.next_cycle();
        }

        self.record_sw(opcode, new_sp, stack);

        self.state.sp = new_sp;

        let main_op_event = self.logs.last().unwrap();
        if let Some(data_op_event) = self.make_sub_op_event(main_op_event.clone()) {
            self.data_op_logs.push(data_op_event);

            // Increment by DEFAULT_CLK_INC since sub_op is also an instruction.
            self.state.next_cycle();
            self.state.next_cycle();
        }

        // Advance the clock to prepare for the next memory read phase.
        self.state.next_cycle();
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

    pub fn make_sub_op_event(&self, main_op_log: TracerInstrState) -> Option<DataOpEvent> {
        let sub_op: Option<_> = {
            match main_op_log.opcode {
                Opcode::TableInit(_) => {
                    if let Some(InstrStateExtension::TableInit(extension)) = main_op_log.extension {
                        Some(Opcode::TableGet(extension.table_idx as u16))
                    } else {
                        None
                    }
                }
                Opcode::CallIndirect(_) => {
                    if let Some(InstrStateExtension::Call(extension)) = main_op_log.extension {
                        Some(Opcode::TableGet(extension.table_idx as u16))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        };
        sub_op.map(|sub_op| {
            let mut dataop_event = DataOpEvent::default();
            dataop_event.clk = main_op_log.clk;
            dataop_event.pc = main_op_log.pc + 1;
            dataop_event.code = sub_op.code();
            dataop_event.aux_value = sub_op.aux_value();
            dataop_event
        })
    }

    pub fn record_mr(&mut self, ins: Opcode, sp: u32) -> MemoryAccessRecord {
        let length = ins.opcode_stack_read();
        let mut memory_access = MemoryAccessRecord::default();

        // Handle the top of the stack (idx 0).
        // If length is > 0, we always read from `sp`.
        if length > 0 {
            let addr = sp;
            let rec = MemoryRecordEnum::Read(self.mr(addr));

            if length == 1 {
                // If there is only 1 argument, the top of the stack is Arg 1.
                memory_access.arg1_record = Some(rec);
            } else {
                // If there are 2 arguments, the top of the stack is usually Arg 2 (RHS),
                // while the deeper element is Arg 1 (LHS).
                memory_access.arg2_record = Some(rec);
            }
        }

        // Handle the second element on the stack (idx 1).
        // This is read only if the instruction consumes 2 (or more) arguments.
        if length >= 2 {
            let addr = sp + UNIT; // Equivalent to sp + 1 * UNIT
            let rec = MemoryRecordEnum::Read(self.mr(addr));

            memory_access.arg1_record = Some(rec);
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

    pub fn mr_with_local_access(
        &mut self,
        addr: u32,
        local_memory_access: Option<&mut HashMap<u32, MemoryLocalEvent>>,
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
        local_memory_access: Option<&mut HashMap<u32, MemoryLocalEvent>>,
    ) -> MemoryWriteRecord {
        let record = self.memory_records.entry(addr).or_default();
        // println!("memory write addr: {}record:{:?} clk:{}", addr, record, self.state.clk);
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
    match opcode {
        Opcode::I32Store(_) => addr % 4 != 0,
        Opcode::I32Store16(_) => addr % 4 == 3,
        Opcode::I32Load(_) => addr % 4 != 0,
        Opcode::I32Load16S(_) => addr % 4 == 3,
        Opcode::I32Load16U(_) => addr % 4 == 3,

        _ => false,
    }
}
