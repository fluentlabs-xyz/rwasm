use crate::{
    executor::{
        config::ExecutorConfig,
        data_entity::DataSegmentEntity,
        element_entity::ElementSegmentEntity,
        handler::{always_failing_syscall_handler, SyscallHandler},
        instr_ptr::InstructionPtr,
        memory::GlobalMemory,
        opcodes::run_the_loop,
        table_entity::TableEntity,
        tracer::Tracer,
        value_stack::{ValueStack, ValueStackPtr},
    },
    types::{
        AddressOffset,
        DataSegmentIdx,
        DropKeep,
        ElementSegmentIdx,
        FuelCosts,
        GlobalIdx,
        OpcodeData,
        Pages,
        RwasmError,
        RwasmModule,
        SignatureIdx,
        TableIdx,
        UntypedValue,
        FUNC_REF_NULL,
        FUNC_REF_OFFSET,
        N_DEFAULT_STACK_SIZE,
        N_MAX_RECURSION_DEPTH,
        N_MAX_STACK_SIZE,
    },
};
use alloc::sync::Arc;
use hashbrown::HashMap;

pub struct RwasmExecutor<T> {
    // function segments
    pub(crate) module: Arc<RwasmModule>,
    pub(crate) config: ExecutorConfig,
    // execution context information
    pub(crate) consumed_fuel: u64,
    pub(crate) refunded_fuel: i64,
    pub(crate) value_stack: ValueStack,
    pub(crate) sp: ValueStackPtr,
    pub(crate) global_memory: GlobalMemory,
    pub(crate) ip: InstructionPtr,
    pub(crate) context: T,
    pub(crate) tracer: Option<Tracer>,
    pub(crate) fuel_costs: FuelCosts,
    // rwasm modified segments
    pub(crate) global_variables: HashMap<GlobalIdx, UntypedValue>,
    pub(crate) tables: HashMap<TableIdx, TableEntity>,
    pub(crate) data_segments: HashMap<DataSegmentIdx, DataSegmentEntity>,
    pub(crate) elements: HashMap<ElementSegmentIdx, ElementSegmentEntity>,
    // list of nested calls return pointers
    pub(crate) call_stack: Vec<InstructionPtr>,
    // the last used signature (needed for indirect calls type checks)
    pub(crate) last_signature: Option<SignatureIdx>,
    pub(crate) next_result: Option<Result<i32, RwasmError>>,
    pub(crate) stop_exec: bool,
    pub(crate) syscall_handler: SyscallHandler<T>,
}

impl<T> RwasmExecutor<T> {
    pub fn parse(
        rwasm_bytecode: &[u8],
        config: ExecutorConfig,
        context: T,
    ) -> Result<Self, RwasmError> {
        Ok(Self::new(
            Arc::new(RwasmModule::new(rwasm_bytecode)),
            config,
            context,
        ))
    }

    pub fn new(module: Arc<RwasmModule>, config: ExecutorConfig, context: T) -> Self {
        // create a stack with sp
        let mut value_stack = ValueStack::new(N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE);
        let sp = value_stack.stack_ptr();

        // assign sp to the position inside a code section
        let mut ip = InstructionPtr::new(module.code_section.instr.as_ptr());
        ip.add(module.source_pc as usize);

        // create global memory
        let global_memory = GlobalMemory::new(Pages::default());

        // create the main element segment (index 0) from the module elements
        let mut element_segments = HashMap::new();
        element_segments.insert(
            ElementSegmentIdx::from(0u32),
            ElementSegmentEntity::new(
                module
                    .element_section
                    .iter()
                    .copied()
                    .map(|v| UntypedValue::from(v + FUNC_REF_OFFSET))
                    .collect(),
            ),
        );

        let tracer = if config.trace_enabled {
            Some(Tracer::default())
        } else {
            None
        };

        Self {
            module,
            config,
            consumed_fuel: 0,
            refunded_fuel: 0,
            value_stack,
            sp,
            global_memory,
            ip,
            context,
            tracer,
            fuel_costs: Default::default(),
            global_variables: Default::default(),
            tables: Default::default(),
            data_segments: Default::default(),
            elements: element_segments,
            call_stack: vec![],
            last_signature: None,
            next_result: None,
            stop_exec: false,
            syscall_handler: always_failing_syscall_handler,
        }
    }

    pub fn set_syscall_handler(&mut self, handler: SyscallHandler<T>) {
        self.syscall_handler = handler;
    }

    pub fn program_counter(&self) -> u32 {
        self.ip.pc()
    }

    pub fn reset(&mut self, pc: Option<usize>) {
        let mut ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        ip.add(pc.unwrap_or(self.module.source_pc as usize));
        self.ip = ip;
        self.consumed_fuel = 0;
        self.value_stack.drain();
        self.sp = self.value_stack.stack_ptr();
        self.call_stack.clear();
        self.last_signature = None;
    }

    pub fn reset_last_signature(&mut self) {
        self.last_signature = None;
    }

    pub fn try_consume_fuel(&mut self, fuel: u64) -> Result<(), RwasmError> {
        let consumed_fuel = self.consumed_fuel.checked_add(fuel).unwrap_or(u64::MAX);
        if let Some(fuel_limit) = self.config.fuel_limit {
            if consumed_fuel > fuel_limit {
                return Err(RwasmError::OutOfFuel);
            }
        }
        self.consumed_fuel = consumed_fuel;
        Ok(())
    }

    pub fn refund_fuel(&mut self, fuel: i64) {
        self.refunded_fuel += fuel;
    }

    pub fn adjust_fuel_limit(&mut self) -> u64 {
        let consumed_fuel = self.consumed_fuel;
        if let Some(fuel_limit) = self.config.fuel_limit.as_mut() {
            *fuel_limit -= self.consumed_fuel;
        }
        self.consumed_fuel = 0;
        consumed_fuel
    }

    pub fn remaining_fuel(&self) -> Option<u64> {
        Some(self.config.fuel_limit? - self.consumed_fuel)
    }

    pub fn fuel_consumed(&self) -> u64 {
        self.consumed_fuel
    }

    pub fn fuel_refunded(&self) -> i64 {
        self.refunded_fuel
    }

    pub fn tracer(&self) -> Option<&Tracer> {
        self.tracer.as_ref()
    }

    pub fn tracer_mut(&mut self) -> Option<&mut Tracer> {
        self.tracer.as_mut()
    }

    pub fn context(&self) -> &T {
        &self.context
    }

    pub fn context_mut(&mut self) -> &mut T {
        &mut self.context
    }

    pub fn run(&mut self) -> Result<i32, RwasmError> {
        match run_the_loop(self) {
            Ok(exit_code) => Ok(exit_code),
            Err(err) => match err {
                RwasmError::ExecutionHalted(exit_code) => Ok(exit_code),
                _ => Err(err),
            },
        }
    }

    pub(crate) fn resolve_table(&mut self, table_idx: TableIdx) -> &mut TableEntity {
        self.tables
            .get_mut(&table_idx)
            .expect("rwasm: missing table")
    }

    pub(crate) fn resolve_table_or_create(&mut self, table_idx: TableIdx) -> &mut TableEntity {
        self.tables
            .entry(table_idx)
            .or_insert_with(Self::empty_table)
    }

    fn empty_table() -> TableEntity {
        TableEntity::new(UntypedValue::from(FUNC_REF_NULL), 0)
    }

    fn empty_element_segment() -> ElementSegmentEntity {
        ElementSegmentEntity::new(vec![])
    }

    fn empty_data_segment() -> DataSegmentEntity {
        DataSegmentEntity::new([0x1].into())
    }

    pub(crate) fn resolve_data_or_create(
        &mut self,
        data_segment_idx: DataSegmentIdx,
    ) -> &mut DataSegmentEntity {
        self.data_segments
            .entry(data_segment_idx)
            .or_insert_with(Self::empty_data_segment)
    }

    pub(crate) fn resolve_element_or_create(
        &mut self,
        element_idx: ElementSegmentIdx,
    ) -> &mut ElementSegmentEntity {
        self.elements
            .entry(element_idx)
            .or_insert_with(Self::empty_element_segment)
    }

    pub(crate) fn resolve_table_with_element_or_create(
        &mut self,
        table_idx: TableIdx,
        element_idx: ElementSegmentIdx,
    ) -> (&mut TableEntity, &mut ElementSegmentEntity) {
        let table_entity = self
            .tables
            .entry(table_idx)
            .or_insert_with(Self::empty_table);
        let element_entity = self
            .elements
            .entry(element_idx)
            .or_insert_with(Self::empty_element_segment);
        (table_entity, element_entity)
    }

    pub(crate) fn fetch_drop_keep(&self, offset: usize) -> DropKeep {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.data() {
            OpcodeData::DropKeep(drop_keep) => *drop_keep,
            _ => unreachable!("rwasm: can't extract drop keep"),
        }
    }

    pub(crate) fn fetch_table_index(&self, offset: usize) -> TableIdx {
        let mut addr: InstructionPtr = self.ip;
        addr.add(offset);
        match addr.data() {
            OpcodeData::TableIdx(table_idx) => *table_idx,
            _ => unreachable!("rwasm: can't extract table index"),
        }
    }

    #[inline(always)]
    pub(crate) fn execute_load_extend(
        &mut self,
        offset: AddressOffset,
        load_extend: fn(
            memory: &[u8],
            address: UntypedValue,
            offset: u32,
        ) -> Result<UntypedValue, RwasmError>,
    ) -> Result<(), RwasmError> {
        self.sp.try_eval_top(|address| {
            let memory = self.global_memory.data();
            let value = load_extend(memory, address, offset.into_inner())?;
            Ok(value)
        })?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn execute_store_wrap(
        &mut self,
        offset: AddressOffset,
        store_wrap: fn(
            memory: &mut [u8],
            address: UntypedValue,
            offset: u32,
            value: UntypedValue,
        ) -> Result<(), RwasmError>,
        len: u32,
    ) -> Result<(), RwasmError> {
        let (address, value) = self.sp.pop2();
        let memory = self.global_memory.data_mut();
        store_wrap(memory, address, offset.into_inner(), value)?;
        self.ip.offset(0);
        let address = u32::from(address);
        let base_address = offset.into_inner() + address;
        if let Some(tracer) = self.tracer.as_mut() {
            tracer.memory_change(
                base_address,
                len,
                &memory[base_address as usize..(base_address + len) as usize],
            );
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn execute_unary(&mut self, f: fn(UntypedValue) -> UntypedValue) {
        self.sp.eval_top(f);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn execute_binary(&mut self, f: fn(UntypedValue, UntypedValue) -> UntypedValue) {
        self.sp.eval_top2(f);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn try_execute_unary(
        &mut self,
        f: fn(UntypedValue) -> Result<UntypedValue, RwasmError>,
    ) -> Result<(), RwasmError> {
        self.sp.try_eval_top(f)?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn try_execute_binary(
        &mut self,
        f: fn(UntypedValue, UntypedValue) -> Result<UntypedValue, RwasmError>,
    ) -> Result<(), RwasmError> {
        self.sp.try_eval_top2(f)?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn execute_call_internal(
        &mut self,
        is_nested_call: bool,
        skip: usize,
        func_idx: u32,
    ) -> Result<(), RwasmError> {
        self.ip.add(skip);
        self.value_stack.sync_stack_ptr(self.sp);
        if is_nested_call {
            if self.call_stack.len() > N_MAX_RECURSION_DEPTH {
                return Err(RwasmError::StackOverflow);
            }
            self.call_stack.push(self.ip);
        }
        let instr_ref = self
            .module
            .func_section
            .get(func_idx as usize)
            .copied()
            .expect("rwasm: unknown internal function");
        self.sp = self.value_stack.stack_ptr();
        self.ip = InstructionPtr::new(self.module.code_section.instr.as_ptr());
        self.ip.add(instr_ref as usize);
        Ok(())
    }
}
