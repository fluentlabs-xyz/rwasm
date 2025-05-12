use crate::{Instruction, Opcode, Pages, RwasmExecutor, TableEntity, TrapCode};

#[inline(always)]
pub(crate) fn exec_system_opcode<T>(
    vm: &mut RwasmExecutor<T>,
    instr: Instruction,
) -> Result<(), TrapCode> {
    use Opcode::*;
    match instr.opcode() {
        MemorySize => {
            let result: u32 = vm.global_memory.current_pages().into();
            vm.sp.push_as(result);
            vm.ip.add(1);
        }
        MemoryGrow => {
            let delta: u32 = vm.sp.pop_as();
            let delta = match Pages::new(delta) {
                Some(delta) => delta,
                None => {
                    vm.sp.push_as(u32::MAX);
                    vm.ip.add(1);
                    return Ok(());
                }
            };
            if vm.config.fuel_enabled {
                let delta_in_bytes = delta.to_bytes().unwrap_or(0) as u64;
                vm.try_consume_fuel(vm.fuel_costs.fuel_for_bytes(delta_in_bytes))?;
            }
            let new_pages = vm
                .global_memory
                .grow(delta)
                .map(u32::from)
                .unwrap_or(u32::MAX);
            vm.sp.push_as(new_pages);
            vm.ip.add(1);
        }
        MemoryFill => {
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
        }
        MemoryCopy => {
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
        }
        MemoryInit => {
            let data_segment_idx = match instr {
                Instruction::DataSegmentIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let is_empty_data_segment = vm
                .empty_data_segments
                .get(data_segment_idx.to_u32() as usize)
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
            let mut memory_section = vm.module.memory_section.as_slice();
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
        }
        DataDrop => {
            let data_segment_idx = match instr {
                Instruction::DataSegmentIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            vm.empty_data_segments
                .set(data_segment_idx.to_u32() as usize, true);
            vm.ip.add(1);
        }
        TableSize => {
            let table_idx = match instr {
                Instruction::TableIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let table_size = vm
                .tables
                .get(&table_idx)
                .expect("rwasm: unresolved table segment")
                .size();
            vm.sp.push_as(table_size);
            vm.ip.add(1);
        }
        TableGrow => {
            let table_idx = match instr {
                Instruction::TableIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
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
                tracer.table_size_change(table_idx.to_u32(), init.into(), delta);
            }
            vm.ip.add(1);
        }
        TableFill => {
            let table_idx = match instr {
                Instruction::TableIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let (i, val, n) = vm.sp.pop3();
            if vm.config.fuel_enabled {
                vm.try_consume_fuel(vm.fuel_costs.fuel_for_elements(n.as_u32().into()))?;
            }
            vm.tables
                .get_mut(&table_idx)
                .expect("rwasm: missing table")
                .fill_untyped(i.into(), val, n.into())?;
            vm.ip.add(1);
        }
        TableGet => {
            let table_idx = match instr {
                Instruction::TableIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let index = vm.sp.pop();
            let value = vm
                .tables
                .get_mut(&table_idx)
                .expect("rwasm: missing table")
                .get_untyped(index.into())
                .ok_or(TrapCode::TableOutOfBounds)?;
            vm.sp.push(value);
            vm.ip.add(1);
        }
        TableSet => {
            let table_idx = match instr {
                Instruction::TableIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            let (index, value) = vm.sp.pop2();
            vm.tables
                .get_mut(&table_idx)
                .expect("rwasm: missing table")
                .set_untyped(index.into(), value)
                .map_err(|_| TrapCode::TableOutOfBounds)?;
            #[cfg(feature = "tracing")]
            if let Some(tracer) = vm.tracer.as_mut() {
                tracer.table_change(table_idx.to_u32(), index.into(), value);
            }
            vm.ip.add(1);
        }
        TableCopy => {
            let dst_table_idx = match instr {
                Instruction::TableIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
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
        }
        TableInit => {
            let element_segment_idx = match instr {
                Instruction::ElementSegmentIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
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
                .empty_elements_segments
                .get(element_segment_idx.to_u32() as usize)
                .as_deref()
                .copied()
                .unwrap_or(false);

            let mut module_elements_section = &vm.default_elements_segment[..];
            if is_empty_segment {
                module_elements_section = &[];
            }
            let table = vm.tables.get_mut(&table_idx).expect("rwasm: missing table");
            table.init_untyped(dst_index, module_elements_section, src_index, len)?;

            vm.ip.add(2);
        }
        ElemDrop => {
            let element_segment_idx = match instr {
                Instruction::ElementSegmentIdx(_, value) => value,
                _ => unreachable!("rwasm: missing instr data"),
            };
            vm.empty_elements_segments
                .set(element_segment_idx.to_u32() as usize, true);
            vm.ip.add(1);
        }
        _ => unreachable!(),
    }
    Ok(())
}
