use crate::{ElementSegmentIdx, RwasmExecutor, TableEntity, TableIdx, TrapCode};

#[inline(always)]
pub(crate) fn visit_table_size<T>(vm: &mut RwasmExecutor<T>, table_idx: TableIdx) {
    let table_size = vm
        .tables
        .get(&table_idx)
        .expect("rwasm: unresolved table segment")
        .size();
    vm.sp.push_as(table_size);
    vm.ip.add(1);
}

#[inline(always)]
pub(crate) fn visit_table_grow<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let (init, delta) = vm.sp.pop2();
    let delta: u32 = delta.into();
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(delta as u64))?;
    }
    let table = vm.tables.entry(table_idx).or_insert_with(TableEntity::new);
    let result = table.grow_untyped(delta, init);
    vm.sp.push_as(result);
    #[cfg(feature = "tracing")]
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.table_size_change(table_idx, init.into(), delta);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_fill<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let (i, val, n) = vm.sp.pop3();
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(n.into()))?;
    }
    vm.tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .fill_untyped(i.into(), val, n.into())?;
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_get<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let index = vm.sp.pop();
    let value = vm
        .tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .get_untyped(index.into())
        .ok_or(TrapCode::TableOutOfBounds)?;
    vm.sp.push(value);
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_set<T>(
    vm: &mut RwasmExecutor<T>,
    table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let (index, value) = vm.sp.pop2();
    vm.tables
        .get_mut(&table_idx)
        .expect("rwasm: missing table")
        .set_untyped(index.into(), value)
        .map_err(|_| TrapCode::TableOutOfBounds)?;
    #[cfg(feature = "tracing")]
    if let Some(tracer) = vm.tracer.as_mut() {
        tracer.table_change(table_idx, index.into(), value);
    }
    vm.ip.add(1);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_copy<T>(
    vm: &mut RwasmExecutor<T>,
    dst_table_idx: TableIdx,
) -> Result<(), TrapCode> {
    let src_table_idx = vm.fetch_table_index(1);
    let (d, s, n) = vm.sp.pop3();
    let len = u32::from(n);
    let src_index = u32::from(s);
    let dst_index = u32::from(d);
    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(len as u64))?;
    }
    // Query both tables and check if they are the same:
    if src_table_idx != dst_table_idx {
        let [src, dst] = vm
            .tables
            .get_many_mut([&src_table_idx, &dst_table_idx])
            .map(|v| v.expect("rwasm: unresolved table segment"));
        TableEntity::copy(dst, dst_index, src, src_index, len)?;
    } else {
        let src = vm
            .tables
            .get_mut(&src_table_idx)
            .expect("rwasm: unresolved table segment");
        src.copy_within(dst_index, src_index, len)?;
    }
    vm.ip.add(2);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_table_init<T>(
    vm: &mut RwasmExecutor<T>,
    element_segment_idx: ElementSegmentIdx,
) -> Result<(), TrapCode> {
    let table_idx = vm.fetch_table_index(1);

    let (d, s, n) = vm.sp.pop3();
    let len = u32::from(n);
    let src_index = u32::from(s);
    let dst_index = u32::from(d);

    if vm.config.fuel_enabled {
        vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(len as u64))?;
    }

    // There is a trick with `element_segment_idx`:
    // it refers to the segment number.
    // However, in rwasm, all elements are stored in segment 0,
    // so there is no need to store information about the remaining segments.
    // According to the WebAssembly standards, though,
    // we must retain information about all dropped element segments
    // to perform an emptiness check.
    // Therefore, in `element_segment_idx`, we store the original index,
    // which is always > 0.
    let is_empty_segment = vm
        .empty_elem_segments
        .get(element_segment_idx as usize)
        .as_deref()
        .copied()
        .unwrap_or(false);

    let mut module_elements_section = &vm.module.elem_section[..];
    if is_empty_segment {
        module_elements_section = &[];
    }
    let table = vm.tables.get_mut(&table_idx).expect("rwasm: missing table");
    table.init_untyped(dst_index, module_elements_section, src_index, len)?;

    vm.ip.add(2);
    Ok(())
}

#[inline(always)]
pub(crate) fn visit_element_drop<T>(
    vm: &mut RwasmExecutor<T>,
    element_segment_idx: ElementSegmentIdx,
) {
    vm.empty_elem_segments
        .set(element_segment_idx as usize, true);
    vm.ip.add(1);
}
