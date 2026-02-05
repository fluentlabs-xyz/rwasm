use crate::{
    AddressOffset, DataSegmentIdx, I64ValueSplit, InstructionSet, TrapCode, N_BYTES_PER_MEMORY_PAGE,
};
use rwasm_fuel_policy::{MEMORY_BYTES_PER_FUEL, MEMORY_BYTES_PER_FUEL_LOG2};

impl InstructionSet {
    pub const MSH_I64_LOAD: u32 = 2;
    pub const MSH_I64_LOAD8_S: u32 = 1;
    pub const MSH_I64_LOAD8_U: u32 = 1;
    pub const MSH_I64_LOAD16_S: u32 = 1;
    pub const MSH_I64_LOAD16_U: u32 = 1;
    pub const MSH_I64_LOAD32_S: u32 = 1;
    pub const MSH_I64_LOAD32_U: u32 = 1;
    pub const MSH_I64_STORE: u32 = 2;
    pub const MSH_I64_STORE8: u32 = 0;
    pub const MSH_I64_STORE16: u32 = 0;
    pub const MSH_I64_STORE32: u32 = 0;
    pub const MSH_I64_CONST: u32 = 2;
    pub const MSH_MEMORY_GROW_CHECKED: u32 = 2;
    pub const MSH_MEMORY_FILL_CHECKED: u32 = 2;
    pub const MSH_MEMORY_COPY_CHECKED: u32 = 2;
    pub const MSH_MEMORY_INIT_CHECKED: u32 = 2;

    /// Loads a 64-bit word from a memory and copies it on the stack by as 32-bit words
    ///
    /// Input: [addr]
    /// Output: [hi, lo]
    ///
    /// Max stack height: 2
    pub fn op_i64_load(&mut self, offset: AddressOffset) {
        // [addr]
        self.op_local_get(1);
        // [addr, addr]
        self.op_i32_load(offset.checked_add(4).unwrap_or(u32::MAX));
        // [hi, addr]
        self.op_local_get(2);
        // [addr, hi, addr]
        self.op_i32_load(offset);
        // [lo, hi, addr]
        self.op_local_set(2);
        // [hi, lo]
    }

    /// Max stack height: 1
    pub fn op_i64_load8_s(&mut self, offset: AddressOffset) {
        self.op_i32_load8_s(offset);
        self.op_dup();
        self.op_i32_const(31);
        self.op_i32_shr_s();
    }

    /// Max stack height: 1
    pub fn op_i64_load8_u(&mut self, offset: AddressOffset) {
        self.op_i32_load8_u(offset);
        self.op_i32_const(0);
    }

    /// Max stack height: 1
    pub fn op_i64_load16_s(&mut self, offset: AddressOffset) {
        self.op_i32_load16_s(offset);
        self.op_dup();
        self.op_i32_const(31);
        self.op_i32_shr_s();
    }

    /// Max stack height: 1
    pub fn op_i64_load16_u(&mut self, offset: AddressOffset) {
        self.op_i32_load16_u(offset);
        self.op_i32_const(0);
    }

    /// Max stack height: 1
    pub fn op_i64_load32_s(&mut self, offset: AddressOffset) {
        self.op_i32_load(offset);
        self.op_dup();
        self.op_i32_const(31);
        self.op_i32_shr_s();
    }

    /// Max stack height: 1
    pub fn op_i64_load32_u(&mut self, offset: AddressOffset) {
        self.op_i32_load(offset);
        self.op_i32_const(0);
    }

    /// Max stack height: 2
    pub fn op_i64_store(&mut self, offset: AddressOffset) {
        self.op_local_get(3);
        self.op_local_get(2);
        self.op_i32_store(offset.checked_add(4).unwrap_or(u32::MAX));
        self.op_drop();
        self.op_i32_store(offset);
    }

    /// Max stack height: 0
    pub fn op_i64_store8(&mut self, offset: AddressOffset) {
        self.op_drop();
        self.op_i32_store8(offset);
    }

    /// Max stack height: 0
    pub fn op_i64_store16(&mut self, offset: AddressOffset) {
        self.op_drop();
        self.op_i32_store16(offset);
    }

    /// Max stack height: 0
    pub fn op_i64_store32(&mut self, offset: AddressOffset) {
        self.op_drop();
        self.op_i32_store(offset);
    }

    /// Max stack height: 2
    pub fn op_i64_const(&mut self, value: i64) {
        let (lo, hi) = value.split_into_i32_tuple();
        self.op_i32_const(lo); // [lo]
        self.op_i32_const(hi); // [hi, lo]
    }

    /// Max stack height: 2
    pub fn op_memory_grow_checked(&mut self, max_pages: Option<u32>, inject_fuel_check: bool) {
        // we must do max memory check before an execution
        if let Some(max_pages) = max_pages {
            self.op_local_get(1); // d
            self.op_memory_size(); // memory_size
            self.op_i32_add(); // memory_size+d
            self.op_i32_const(max_pages); // max_pages
            self.op_i32_gt_s(); // memory_size+d>max_pages
            self.op_br_if_eqz(4);
            self.op_drop();
            self.op_i32_const(u32::MAX);
            self.op_br(if inject_fuel_check { 8 } else { 2 });
        }
        // now we know that pages can't exceed i32::MAX,
        // so we can safely multiply the num of pages to the page size
        // to calculate fuel required for memory to grow
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(N_BYTES_PER_MEMORY_PAGE); // size of each memory page
            self.op_i32_mul(); // overflow is impossible here (we pass max pages in trustless mode)
            self.op_i32_const(MEMORY_BYTES_PER_FUEL_LOG2); // 2^6=64
            self.op_i32_shr_u(); // delta/64
            self.op_consume_fuel_stack();
        }
        // emit memory grows only if fuel is charged
        self.op_memory_grow();
    }

    /// Max stack height: 2
    pub fn op_memory_fill_checked(&mut self, inject_fuel_check: bool) {
        // [d, val, n]
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(MEMORY_BYTES_PER_FUEL - 1); // upper round
            self.op_i32_add();
            self.op_i32_const(MEMORY_BYTES_PER_FUEL_LOG2); // 2^6=64
            self.op_i32_shr_u(); // delta/64
            self.op_consume_fuel_stack();
        }
        // emit memory fill
        self.op_memory_fill();
    }

    /// Max stack height: 2
    pub fn op_memory_copy_checked(&mut self, inject_fuel_check: bool) {
        // [d, s, n]
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(MEMORY_BYTES_PER_FUEL - 1); // upper round
            self.op_i32_add();
            self.op_i32_const(MEMORY_BYTES_PER_FUEL_LOG2); // 2^6=64
            self.op_i32_shr_u(); // delta/64
            self.op_consume_fuel_stack();
        }
        // emit memory copy
        self.op_memory_copy();
    }

    /// MemoryInit opcode reads 3 elements from the stack (dst, src, len), where:
    /// - dst - Memory destination of copied data
    /// - src - Data source of copied data (in the passive section)
    /// - len - Length of copied data
    ///
    /// In the `passive_sections` field, we store info about all passive sections
    /// that are presented in the WebAssembly binary. When a passive section is activated
    /// though `memory.init` opcode, we find modified offsets in the data section
    /// and put on the stack by removing previous values.
    ///
    /// Here is the stack structure for `memory.init` call:
    /// - ... some other stack elements
    /// - dst
    /// - src
    /// - len
    /// - ... call of `memory.init` happens here
    ///
    /// Here we need to replace the `src` field with our modified, but since we don't know
    /// how the stack was structured, then we can achieve it by replacing a stack element using
    /// `local.set` opcode.
    ///
    /// - dst
    /// - src <-----+
    /// - len       |
    /// - new_src --+
    /// - ... call `local.set` to replace prev offset
    ///
    /// Here we use 1 offset because we pop `new_src`, and then count from the top, `len`
    /// has 0 offset and `src` has offset 1.
    ///
    /// Before doing these ops, we must ensure that the specified length of copied data
    /// doesn't exceed the original section size. We also inject GT check to make sure that
    /// there is no data section overflow.
    ///
    /// Max stack height: 2
    pub fn op_memory_init_checked(
        &mut self,
        rewrite_offset: Option<u32>,
        rewrite_length: Option<u32>,
        data_segment_index: DataSegmentIdx,
        inject_fuel_check: bool,
    ) {
        // do an overflow check
        if let Some(length) = rewrite_length.filter(|v| *v > 0) {
            self.op_local_get(1); // n
            self.op_local_get(3); // s
            self.op_i32_add(); // n + s
            self.op_i32_const(length);
            self.op_i32_gt_s(); // n + s > length
            self.op_br_if_eqz(2);
            self.op_trap(TrapCode::MemoryOutOfBounds);
        }
        // we need to replace the offset on the stack with the new value
        if let Some(offset) = rewrite_offset.filter(|v| *v > 0) {
            self.op_i32_const(offset);
            self.op_local_get(3);
            self.op_i32_add();
            self.op_local_set(2);
        }
        // [d, s, n]
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(MEMORY_BYTES_PER_FUEL - 1); // upper round
            self.op_i32_add();
            self.op_i32_const(MEMORY_BYTES_PER_FUEL_LOG2); // 2^6=64
            self.op_i32_shr_u(); // delta/64
            self.op_consume_fuel_stack();
        }
        // emit memory init
        self.op_memory_init(data_segment_index);
    }
}
