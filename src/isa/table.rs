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
        // Spec bound check on the original `s`/`n`: trap when `s > len` or `n > len - s`.
        //
        // Both compares are unsigned and neither operand sum can wrap: the subtraction only runs
        // after `s <= len` has been established. The previous form compared the wrapping sum
        // `n + s` against `len` with a signed `gt_s`, so a source index near `u32::MAX` wrapped
        // into range and the init installed another segment's elements instead of trapping.
        // [d, s, n]
        self.op_local_get(2); // s
        self.op_i32_const(length);
        self.op_i32_gt_u(); // s > len
        self.op_br_if_eqz(2);
        self.op_trap(TrapCode::TableOutOfBounds);
        // [d, s, n]
        self.op_i32_const(length); // len
        self.op_local_get(3); // s
        self.op_i32_sub(); // len - s, exact because s <= len
        self.op_local_get(2); // n
        self.op_i32_lt_u(); // len - s < n, i.e. n > len - s
        self.op_br_if_eqz(2);
        self.op_trap(TrapCode::TableOutOfBounds);
        // Address the segment inside the flattened element blob.
        //
        // The blob offset is applied only while the segment is live: a dropped segment keeps its
        // original source offset so the runtime's empty-window check implements the spec rule for
        // a zero-length segment (only `s == 0 && n == 0` survives).
        if offset > 0 {
            self.op_element_segment_live(segment_index);
            self.op_i32_const(offset);
            self.op_i32_mul(); // offset while live, 0 after `elem.drop`
            self.op_local_get(3); // s
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
        //
        // Two unsigned, non-wrapping checks instead of the previous signed `n + table_size >
        // limit`: the sum wrapped for a `delta` near `u32::MAX`, and `limit` is the
        // *module-declared* maximum, which is negative as `i32` for any maximum >= 2^31. Both
        // made the guard report an overflow that never happened.
        if let Some(limit) = limit_check {
            self.op_local_get(1); // n
            self.op_i32_const(limit);
            self.op_i32_gt_u(); // n > limit
            self.op_br_if_eqz(5);
            self.op_drop();
            self.op_drop();
            self.op_i32_const(u32::MAX);
            // we don't trap here, because, according to a wasm standard, we should put u32::MAX on
            // the top of the stack in case of overflow
            self.op_br(if inject_fuel_check { 18 } else { 12 });
            // `n <= limit` now, so `table_size + n` cannot wrap
            self.op_local_get(1); // n
            self.op_table_size(table_idx); // table_size
            self.op_i32_add(); // n+table_size
            self.op_i32_const(limit); // limit
            self.op_i32_gt_u(); // n+table_size>limit
            self.op_br_if_eqz(5);
            self.op_drop();
            self.op_drop();
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
