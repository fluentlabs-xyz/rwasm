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
        self.store
            .tracer
            .table_size_change(table_idx as u32, init.into(), delta);
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
