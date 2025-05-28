use crate::{AddressOffset, DataSegmentIdx, Pages, RwasmExecutor, TrapCode, UntypedValue};

macro_rules! impl_visit_load {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>, address_offset: AddressOffset) -> Result<(), TrapCode> {
                vm.execute_load_extend(address_offset, UntypedValue::$untyped_ident)
            }
        )*
    }
}

impl_visit_load! {
    fn visit_i32_load(i32_load);

    fn visit_i32_load_i8_s(i32_load8_s);
    fn visit_i32_load_i8_u(i32_load8_u);
    fn visit_i32_load_i16_s(i32_load16_s);
    fn visit_i32_load_i16_u(i32_load16_u);
}

macro_rules! impl_visit_store {
    ( $( fn $visit_ident:ident($untyped_ident:ident, $type_size:literal); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident<T>(vm: &mut RwasmExecutor<T>, address_offset: AddressOffset) -> Result<(), TrapCode> {
                vm.execute_store_wrap(address_offset, UntypedValue::$untyped_ident, $type_size)
            }
        )*
    }
}

impl_visit_store! {
    fn visit_i32_store(i32_store, 4);
    fn visit_i32_store_8(i32_store8, 1);
    fn visit_i32_store_16(i32_store16, 2);
}

#[inline(always)]
pub(crate) fn visit_memory_size<T>(vm: &mut RwasmExecutor<T>) {
    let result: u32 = vm.global_memory.current_pages().into();
    vm.sp.push_as(result);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_memory_grow<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let delta: u32 = vm.sp.pop_as();
    let delta = match Pages::new(delta) {
        Some(delta) => delta,
        None => {
            vm.sp.push_as(u32::MAX);
            vm.ip.add(1);
            return Ok(());
        }
    };
    let new_pages = vm
        .global_memory
        .grow(delta)
        .map(u32::from)
        .unwrap_or(u32::MAX);
    vm.sp.push_as(new_pages);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_fill<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let (d, val, n) = vm.sp.pop3();
    let n = i32::from(n) as usize;
    let offset = i32::from(d) as usize;
    let byte = u8::from(val);
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(n as u64))?;
    }
    let memory = vm
        .global_memory
        .data_mut()
        .get_mut(offset..)
        .and_then(|memory| memory.get_mut(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    memory.fill(byte);
    #[cfg(feature = "tracing")]
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.memory_change(offset as u32, n as u32, memory);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_copy<T>(vm: &mut RwasmExecutor<T>) -> Result<(), TrapCode> {
    let (d, s, n) = vm.sp.pop3();
    let n = i32::from(n) as usize;
    let src_offset = i32::from(s) as usize;
    let dst_offset = i32::from(d) as usize;
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(n as u64))?;
    }
    // these accesses just perform the bound checks required by the Wasm spec.
    let data = vm.global_memory.data_mut();
    data.get(src_offset..)
        .and_then(|memory| memory.get(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    data.get(dst_offset..)
        .and_then(|memory| memory.get(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    data.copy_within(src_offset..src_offset.wrapping_add(n), dst_offset);
    #[cfg(feature = "tracing")]
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.memory_change(
            dst_offset as u32,
            n as u32,
            &data[dst_offset..(dst_offset + n)],
        );
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_memory_init<T>(
    vm: &mut RwasmExecutor<T>,
    data_segment_idx: DataSegmentIdx,
) -> Result<(), TrapCode> {
    let is_empty_data_segment = vm
        .empty_data_segments
        .get(data_segment_idx as usize)
        .as_deref()
        .copied()
        .unwrap_or(false);
    let (d, s, n) = vm.sp.pop3();
    let n = i32::from(n) as usize;
    let src_offset = i32::from(s) as usize;
    let dst_offset = i32::from(d) as usize;
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(n as u64))?;
    }
    let memory = vm
        .global_memory
        .data_mut()
        .get_mut(dst_offset..)
        .and_then(|memory| memory.get_mut(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    let mut memory_section = vm.module.data_section.as_slice();
    if is_empty_data_segment {
        memory_section = &[];
    }
    let data = memory_section
        .get(src_offset..)
        .and_then(|data| data.get(..n))
        .ok_or(TrapCode::MemoryOutOfBounds)?;
    memory.copy_from_slice(data);
    #[cfg(feature = "tracing")]
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.global_memory(dst_offset as u32, n as u32, memory);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_data_drop<T>(vm: &mut RwasmExecutor<T>, data_segment_idx: DataSegmentIdx) {
    vm.empty_data_segments.set(data_segment_idx as usize, true);
    vm.ip.add(1);
}
