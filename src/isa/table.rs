use crate::{ElementSegmentIdx, InstructionSet, TableIdx, TrapCode};
use rwasm_fuel_policy::{TABLE_ELEMS_PER_FUEL, TABLE_ELEMS_PER_FUEL_LOG2};

// SAFETY NOTE on `TABLE_ELEMS_PER_FUEL` and the limit checks, applies to every prologue below.
//
// Two pieces of the injected arithmetic are unsound in isolation:
//
// * The limit checks use a signed `i32.gt_s` over quantities that are unsigned by construction,
//   and the `n+s` / `n+table_size` sums use a wrapping `i32.add`. An `n` close to `i32::MAX`
//   makes the sum negative, so the guard passes.
// * Fuel is charged as `(n + TABLE_ELEMS_PER_FUEL - 1) >> TABLE_ELEMS_PER_FUEL_LOG2`, where the
//   `i32.add` wraps for `n >= 2^32 - (TABLE_ELEMS_PER_FUEL - 1)` and rounds down to (almost) zero
//   fuel for a nominally 4 G-element operation.
//
// Neither is reachable today: a table holds at most `N_MAX_TABLE_SIZE` (1024) elements, so every
// `n` big enough to wrap is rejected by the runtime table bounds check (`grow_untyped` uses
// `checked_add` plus the size cap, the bulk ops go through slice bounds checks) before any element
// is touched, and the undercharged operation never performs work. The guards here are an early
// trap, not the bounds check. Raising `N_MAX_TABLE_SIZE` toward `i32::MAX`, or growing
// `TABLE_ELEMS_PER_FUEL`, requires switching these compares to `i32.gt_u` and replacing the
// round-up with an overflow-safe form such as `(n >> LOG2) + ((n & MASK) != 0)` first.

impl InstructionSet {
    pub const MSH_TABLE_INIT_CHECKED: u32 = 2;
    pub const MSH_TABLE_GROW_CHECKED: u32 = 2;
    pub const MSH_TABLE_FILL_CHECKED: u32 = 2;
    pub const MSH_TABLE_COPY_CHECKED: u32 = 2;

    /// Max stack height: 2
    pub fn op_table_init_checked(
        &mut self,
        segment_index: ElementSegmentIdx,
        table_index: TableIdx,
        length: u32,
        offset: u32,
        inject_fuel_check: bool,
    ) {
        // [d, s, n]
        self.op_local_get(1); // n
        self.op_local_get(3); // s
        self.op_i32_add(); // n+s
        self.op_i32_const(length); // length
        self.op_i32_gt_s(); // n+s>length, signed: see the SAFETY NOTE at the top of this file
        self.op_br_if_eqz(2);
        self.op_trap(TrapCode::TableOutOfBounds);
        // we need to replace the offset on the stack with the new value
        if offset > 0 {
            self.op_i32_const(offset);
            self.op_local_get(3);
            self.op_i32_add();
            self.op_local_set(2);
        }
        // charge fuel for this call after all checks
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(TABLE_ELEMS_PER_FUEL - 1); // upper round
            self.op_i32_add(); // wrapping, see the SAFETY NOTE at the top of this file
            self.op_i32_const(TABLE_ELEMS_PER_FUEL_LOG2); // 2^4=16
            self.op_i32_shr_u(); // n/16
            self.op_consume_fuel_stack();
        }
        self.op_table_init(segment_index);
        self.op_table_get(table_index);
    }

    /// Max stack height: 2
    pub fn op_table_grow_checked(
        &mut self,
        table_idx: TableIdx,
        limit_check: Option<u32>,
        inject_fuel_check: bool,
    ) {
        // [init, delta]
        if let Some(limit) = limit_check {
            self.op_local_get(1); // n
            self.op_table_size(table_idx); // table_size
            self.op_i32_add(); // n+table_size
            self.op_i32_const(limit); // limit
            self.op_i32_gt_s(); // n+table_size>limit, signed: see the SAFETY NOTE at the top
            self.op_br_if_eqz(5);
            self.op_drop();
            self.op_drop();
            // we don't trap here, because, according to a wasm standard, we should put u32::MAX on
            // the top of the stack in case of overflow
            self.op_i32_const(u32::MAX);
            self.op_br(if inject_fuel_check { 8 } else { 2 });
        }
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(TABLE_ELEMS_PER_FUEL - 1); // upper round
            self.op_i32_add(); // wrapping, see the SAFETY NOTE at the top of this file
            self.op_i32_const(TABLE_ELEMS_PER_FUEL_LOG2); // 2^4=16
            self.op_i32_shr_u(); // n/16
            self.op_consume_fuel_stack();
        }
        self.op_table_grow(table_idx);
    }

    /// Max stack height: 2
    pub fn op_table_fill_checked(&mut self, table_idx: TableIdx, inject_fuel_check: bool) {
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(TABLE_ELEMS_PER_FUEL - 1); // upper round
            self.op_i32_add(); // wrapping, see the SAFETY NOTE at the top of this file
            self.op_i32_const(TABLE_ELEMS_PER_FUEL_LOG2); // 2^4=16
            self.op_i32_shr_u(); // n/16
            self.op_consume_fuel_stack();
        }
        self.op_table_fill(table_idx);
    }

    /// Max stack height: 2
    pub fn op_table_copy_checked(
        &mut self,
        dst_table_idx: TableIdx,
        src_table_idx: TableIdx,
        inject_fuel_check: bool,
    ) {
        if inject_fuel_check {
            self.op_local_get(1); // n
            self.op_i32_const(TABLE_ELEMS_PER_FUEL - 1); // upper round
            self.op_i32_add(); // wrapping, see the SAFETY NOTE at the top of this file
            self.op_i32_const(TABLE_ELEMS_PER_FUEL_LOG2); // 2^4=16
            self.op_i32_shr_u(); // n/16
            self.op_consume_fuel_stack();
        }
        self.op_table_copy(dst_table_idx, src_table_idx);
    }
}
