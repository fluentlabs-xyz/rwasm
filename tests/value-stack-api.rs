//! Unit tests for the [`ValueStack`] accounting API.
//!
//! `tests/value-stack-bounds.rs` covers the same guarantees as observed through the interpreter.
//! This file drives the stack directly, pinning down the bookkeeping the compiler relies on when
//! it sizes a `StackCheck`: the height watermark behind the `MSH_*` constants, the suppressed
//! out-of-bounds accesses, and the push/pop/drop arithmetic around the stack pointer.

use rwasm::{TrapCode, UntypedValue, ValueStack};

fn val(x: u32) -> UntypedValue {
    UntypedValue::from(x)
}

fn stack_with(values: &[u32]) -> ValueStack {
    let mut stack = ValueStack::default();
    for &v in values {
        stack.push(val(v));
    }
    stack
}

#[test]
fn empty_stack_does_not_allocate_and_reports_zero_height() {
    let stack = ValueStack::empty();
    assert!(stack.is_empty());
    assert_eq!(stack.max_stack_height(), 0);
    assert!(!stack.is_out_of_bounds());
}

#[test]
fn push_and_pop_round_trip_in_lifo_order() {
    let mut stack = stack_with(&[1, 2, 3]);
    assert!(!stack.is_empty());
    assert_eq!(stack.pop(), val(3));
    assert_eq!(stack.pop(), val(2));
    assert_eq!(stack.pop(), val(1));
    assert!(stack.is_empty());
    assert!(!stack.is_out_of_bounds());
}

/// The watermark is what `InstructionSet::MSH_*` is compared against in `tests/snippets.rs`. It is
/// fed by `sync_stack_ptr`, which is how the interpreter publishes the height it reached, and it
/// records the peak rather than the current height.
#[test]
fn max_stack_height_records_the_peak_not_the_current_height() {
    let mut stack = ValueStack::default();
    stack.reserve(8).unwrap();
    let base = stack.stack_ptr();

    stack.sync_stack_ptr(base.into_add(4));
    assert_eq!(stack.max_stack_height(), 4);

    stack.sync_stack_ptr(base.into_add(2));
    assert_eq!(
        stack.max_stack_height(),
        4,
        "descending must not lower the peak"
    );

    stack.sync_stack_ptr(base.into_add(6));
    assert_eq!(stack.max_stack_height(), 6, "exceeding the peak raises it");
}

#[test]
fn popping_an_empty_stack_yields_zero_and_flags_out_of_bounds() {
    let mut stack = ValueStack::default();
    assert_eq!(stack.pop(), UntypedValue::default());
    assert!(stack.is_out_of_bounds());
}

#[test]
fn dropping_more_than_the_stack_holds_clamps_and_flags_out_of_bounds() {
    let mut stack = stack_with(&[1, 2]);
    stack.drop(1);
    assert!(!stack.is_out_of_bounds());
    stack.drop(5);
    assert!(stack.is_empty());
    assert!(stack.is_out_of_bounds());
}

#[test]
fn peeking_deeper_than_the_stack_returns_empty_and_flags_out_of_bounds() {
    let mut stack = stack_with(&[1, 2, 3]);
    assert_eq!(stack.peek_as_slice_mut(2), &[val(2), val(3)]);
    assert!(!stack.is_out_of_bounds());
    assert!(stack.peek_as_slice_mut(9).is_empty());
    assert!(stack.is_out_of_bounds());
}

#[test]
fn peeked_slice_writes_back_into_the_stack() {
    let mut stack = stack_with(&[1, 2, 3]);
    stack.peek_as_slice_mut(2)[0] = val(42);
    assert_eq!(stack.pop(), val(3));
    assert_eq!(stack.pop(), val(42));
}

#[test]
fn reset_clears_the_contents_the_watermark_and_the_out_of_bounds_flag() {
    let mut stack = stack_with(&[1, 2, 3]);
    stack.pop();
    stack.pop();
    stack.pop();
    stack.pop(); // underflow, sets the flag
    assert!(stack.is_out_of_bounds());

    stack.reset();
    assert!(stack.is_empty());
    assert_eq!(stack.max_stack_height(), 0);
    assert!(!stack.is_out_of_bounds());
}

#[test]
fn drain_returns_every_entry_and_empties_the_stack() {
    let mut stack = stack_with(&[7, 8, 9]);
    assert_eq!(stack.drain(), &[val(7), val(8), val(9)]);
    assert!(stack.is_empty());
}

#[test]
fn extend_zeros_appends_default_cells_above_the_live_entries() {
    let mut stack = stack_with(&[1]);
    stack.reserve(4).unwrap();
    stack.extend_zeros(3);
    assert_eq!(stack.dump_stack().len(), 4);
    assert_eq!(stack.pop(), UntypedValue::default());
    assert_eq!(stack.pop(), UntypedValue::default());
    assert_eq!(stack.pop(), UntypedValue::default());
    assert_eq!(stack.pop(), val(1));
}

#[test]
fn reserving_beyond_the_maximum_length_reports_stack_overflow() {
    let mut stack = ValueStack::new(4, 8);
    assert_eq!(stack.reserve(4), Ok(()));
    assert_eq!(stack.reserve(usize::MAX), Err(TrapCode::StackOverflow));
    assert_eq!(stack.reserve(9), Err(TrapCode::StackOverflow));
}

#[test]
fn dump_stack_and_as_slice_expose_the_live_entries_only() {
    let mut stack = stack_with(&[4, 5, 6]);
    stack.pop();
    assert_eq!(stack.dump_stack(), vec![val(4), val(5)]);
    assert_eq!(stack.as_slice(), &[val(4), val(5)]);
}

#[test]
fn extend_pushes_every_item_of_the_iterator() {
    let mut stack = ValueStack::default();
    stack.extend([val(1), val(2), val(3)]);
    assert_eq!(stack.dump_stack(), vec![val(1), val(2), val(3)]);
}

#[test]
fn equality_compares_live_entries_and_ignores_the_spare_capacity() {
    let mut a = stack_with(&[1, 2]);
    let mut b = stack_with(&[1, 2, 3]);
    assert_ne!(a, b);

    b.pop();
    assert_eq!(a, b, "the dropped cell must not affect equality");

    a.push(val(9));
    assert_ne!(a, b);
}

#[test]
fn debug_output_reports_the_pointer_and_the_live_entries() {
    let stack = stack_with(&[1, 2]);
    let rendered = format!("{stack:?}");
    assert!(rendered.contains("ValueStack"), "{rendered}");
    assert!(rendered.contains("stack_ptr"), "{rendered}");
}

#[test]
fn stack_len_is_measured_against_the_stack_pointer() {
    let mut stack = ValueStack::new(4, 8);
    stack.push(val(1));
    stack.push(val(2));

    let sp = stack.stack_ptr();
    assert_eq!(stack.stack_len(sp), 2);
    assert!(!stack.has_stack_overflowed(sp));
    assert_eq!(stack.stack_len(sp.into_sub(2)), 0);
}

/// A pointer can never be walked past the cells it was handed: the attempt is recorded and the
/// pointer is parked on the base, so no caller can dereference a wild address.
#[test]
fn advancing_a_pointer_past_its_capacity_is_refused_and_recorded() {
    let mut stack = ValueStack::new(4, 8);
    stack.push(val(1));

    let sp = stack.stack_ptr();
    assert!(!sp.is_out_of_bounds());

    let beyond = sp.into_add(64);
    assert!(beyond.is_out_of_bounds());
    assert_eq!(
        stack.stack_len(beyond),
        0,
        "the pointer is parked on the base"
    );

    let below = sp.into_sub(9);
    assert!(below.is_out_of_bounds());
}

#[test]
fn sync_stack_ptr_moves_the_height_to_the_given_pointer() {
    let mut stack = stack_with(&[1, 2, 3]);
    let sp = stack.stack_ptr();

    stack.push(val(4));
    assert_eq!(stack.dump_stack().len(), 4);

    stack.sync_stack_ptr(sp);
    assert_eq!(stack.dump_stack(), vec![val(1), val(2), val(3)]);
}
