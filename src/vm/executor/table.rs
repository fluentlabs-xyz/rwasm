use crate::{ElementSegmentIdx, RwasmExecutor, TableEntity, TableIdx, TrapCode};

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_table_size(&mut self, table_idx: TableIdx) {
        let table_size = self
            .store
            .tables
            .get(&table_idx)
            .expect("rwasm: unresolved table segment")
            .size();
        self.sp.push_as(table_size);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_table_grow(&mut self, table_idx: TableIdx) -> Result<(), TrapCode> {
        let (init, delta) = self.sp.pop2();
        let delta: u32 = delta.into();
        let table = self
            .store
            .tables
            .entry(table_idx)
            .or_insert_with(TableEntity::new);
        let result = table.grow_untyped(delta, init);
        self.sp.push_as(result);
        #[cfg(feature = "tracing")]
        {
            use crate::{
                event::FatOpEvent, mem::MemoryLocalEvent, mem_index::TypedAddress, N_MAX_TABLE_SIZE,
            };
            use hashbrown::HashMap;

            let fat_op = self
                .store
                .tracer
                .logs
                .last()
                .unwrap()
                .fat_op
                .clone()
                .unwrap();

            match fat_op {
                FatOpEvent::TableGrow(mut table_grow_event) => {
                    table_grow_event.init = init.into();
                    table_grow_event.delta = delta.into();
                    let mut local_memory_access: HashMap<u32, MemoryLocalEvent> =
                        HashMap::default();

                    for idx in 0..table_grow_event.local_mem_access.len() {
                        local_memory_access.insert(
                            table_grow_event.local_mem_access_addr[idx],
                            table_grow_event.local_mem_access[idx],
                        );
                    }

                    table_grow_event.table_size_read_acess =
                        self.store.tracer.mr_with_local_access(
                            TypedAddress::TableSize(table_idx as u32).to_virtual_addr(),
                            Some(&mut local_memory_access),
                        );

                    self.store.tracer.state.next_cycle();

                    if delta != 0 {
                        table_grow_event.result_write_access =
                            self.store.tracer.mw_with_local_access(
                                table_grow_event.sp,
                                result,
                                Some(&mut local_memory_access),
                            );

                        if result != u32::MAX {
                            table_grow_event.table_size_write_acess =
                                self.store.tracer.mw_with_local_access(
                                    TypedAddress::TableSize(table_idx as u32).to_virtual_addr(),
                                    result + delta,
                                    Some(&mut local_memory_access),
                                );

                            for offset in 0..delta {
                                let dst_addr = TypedAddress::Table(
                                    table_idx as u32 * N_MAX_TABLE_SIZE + result + offset,
                                );
                                let write_record = self.store.tracer.mw_with_local_access(
                                    dst_addr.to_virtual_addr(),
                                    init.into(),
                                    Some(&mut local_memory_access),
                                );
                                table_grow_event.memory_write_acess.push(write_record);
                            }
                        }
                    }

                    table_grow_event.table_idx = table_idx as u32;
                    table_grow_event.local_mem_access =
                        local_memory_access.iter().map(|(_, v)| (*v)).collect();
                    table_grow_event.local_mem_access_addr =
                        local_memory_access.iter().map(|(k, v)| (*k)).collect();
                    self.store.tracer.logs.last_mut().unwrap().fat_op =
                        Some(FatOpEvent::TableGrow(table_grow_event));
                }
                _ => unreachable!(),
            }
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_table_fill(&mut self, table_idx: TableIdx) -> Result<(), TrapCode> {
        let (i, val, n) = self.sp.pop3();
        self.store
            .tables
            .get_mut(&table_idx)
            .expect("rwasm: missing table")
            .fill_untyped(i.into(), val, n.into())?;
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_table_get(&mut self, table_idx: TableIdx) -> Result<(), TrapCode> {
        let index = self.sp.pop();
        let value = self
            .store
            .tables
            .get_mut(&table_idx)
            .expect("rwasm: missing table")
            .get_untyped(index.into())
            .ok_or(TrapCode::TableOutOfBounds)?;
        self.sp.push(value);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_table_set(&mut self, table_idx: TableIdx) -> Result<(), TrapCode> {
        let (index, value) = self.sp.pop2();
        self.store
            .tables
            .get_mut(&table_idx)
            .expect("rwasm: missing table")
            .set_untyped(index.into(), value)
            .map_err(|_| TrapCode::TableOutOfBounds)?;
        #[cfg(feature = "tracing")]
        self.store
            .tracer
            .table_change(table_idx as u32, index.into(), value);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_table_copy(
        &mut self,
        dst_table_idx: TableIdx,
        src_table_idx: TableIdx,
    ) -> Result<(), TrapCode> {
        let (d, s, n) = self.sp.pop3();
        let len = u32::from(n);
        let src_index = u32::from(s);
        let dst_index = u32::from(d);
        // Query both tables and check if they are the same:
        if src_table_idx != dst_table_idx {
            let [src, dst] = self
                .store
                .tables
                .get_many_mut([&src_table_idx, &dst_table_idx])
                .map(|v| v.expect("rwasm: unresolved table segment"));
            TableEntity::copy(dst, dst_index, src, src_index, len)?;
        } else {
            let src = self
                .store
                .tables
                .get_mut(&src_table_idx)
                .expect("rwasm: unresolved table segment");
            src.copy_within(dst_index, src_index, len)?;
        }
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_table_init(
        &mut self,
        element_segment_idx: ElementSegmentIdx,
    ) -> Result<(), TrapCode> {
        let table_idx = self.fetch_table_index(1);

        let (d, s, n) = self.sp.pop3();
        let len = u32::from(n);
        let src_index = u32::from(s);
        let dst_index = u32::from(d);

        // There is a trick with `element_segment_idx`:
        // it refers to the segment number.
        // However, in rwasm, all elements are stored in segment 0,
        // so there is no need to store information about the remaining segments.
        // According to the WebAssembly standards, though,
        // we must retain information about all dropped element segments
        // to perform an emptiness check.
        // Therefore, in `element_segment_idx`, we store the original index,
        // which is always > 0.
        let is_empty_segment = self
            .store
            .empty_elem_segments
            .get(element_segment_idx as usize)
            .as_deref()
            .copied()
            .unwrap_or(false);

        let mut module_elements_section = &self.module.elem_section[..];
        if is_empty_segment {
            module_elements_section = &[];
        }
        let table = self
            .store
            .tables
            .get_mut(&table_idx)
            .expect("rwasm: missing table");
        table.init_untyped(dst_index, module_elements_section, src_index, len)?;

        #[cfg(feature = "tracing")]
        {
            use crate::{
                event::FatOpEvent, mem::MemoryLocalEvent, mem_index::TypedAddress, N_MAX_TABLE_SIZE,
            };
            use hashbrown::HashMap;

            let fat_op = self
                .store
                .tracer
                .logs
                .last()
                .unwrap()
                .fat_op
                .clone()
                .unwrap();

            match fat_op {
                FatOpEvent::TableInit(mut table_init_event) => {
                    table_init_event.s = s.into();
                    table_init_event.d = d.into();
                    table_init_event.n = n.into();
                    let mut local_memory_access: HashMap<u32, MemoryLocalEvent> =
                        HashMap::default();
                    for idx in 0..table_init_event.local_mem_access.len() {
                        local_memory_access.insert(
                            table_init_event.local_mem_access_addr[idx],
                            table_init_event.local_mem_access[idx],
                        );
                    }
                    for offset in 0..len {
                        let src_addr = TypedAddress::Element(src_index + offset);

                        let read_record = self.store.tracer.mr_with_local_access(
                            src_addr.to_virtual_addr(),
                            Some(&mut local_memory_access),
                        );
                        let value = read_record.value;
                        table_init_event.memory_read_access.push(read_record);
                    }
                    self.store.tracer.state.next_cycle();
                    for offset in 0..len {
                        let value = table_init_event.memory_read_access[offset as usize].value;
                        let dst_addr = TypedAddress::Table(
                            table_idx as u32 * N_MAX_TABLE_SIZE + dst_index + offset,
                        );
                        let write_record = self.store.tracer.mw_with_local_access(
                            dst_addr.to_virtual_addr(),
                            value,
                            Some(&mut local_memory_access),
                        );
                        table_init_event.memory_write_acess.push(write_record);
                    }

                    table_init_event.table_idx = table_idx as u32;
                    table_init_event.local_mem_access =
                        local_memory_access.iter().map(|(_, v)| (*v)).collect();
                    table_init_event.local_mem_access_addr =
                        local_memory_access.iter().map(|(k, v)| (*k)).collect();
                    self.store.tracer.logs.last_mut().unwrap().fat_op =
                        Some(FatOpEvent::TableInit(table_init_event));
                }
                _ => {
                    unreachable!();
                }
            }
        }

        self.ip.add(2);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_element_drop(&mut self, element_segment_idx: ElementSegmentIdx) {
        let empty_elem_segments = &mut self.store.empty_elem_segments;
        empty_elem_segments.resize(element_segment_idx as usize + 1, false);
        empty_elem_segments.set(element_segment_idx as usize, true);
        self.ip.add(1);
    }
}
