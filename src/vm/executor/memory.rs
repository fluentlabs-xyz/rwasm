use crate::{AddressOffset, DataSegmentIdx, Pages, RwasmExecutor, TrapCode, UntypedValue};

macro_rules! impl_visit_load {
    ( $( fn $visit_ident:ident($untyped_ident:ident); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident(&mut self, address_offset: AddressOffset) -> Result<(), TrapCode> {
                self.execute_load_extend(address_offset, UntypedValue::$untyped_ident)
            }
        )*
    }
}

macro_rules! impl_visit_store {
    ( $( fn $visit_ident:ident($untyped_ident:ident, $type_size:literal); )* ) => {
        $(
            #[inline(always)]
            pub(crate) fn $visit_ident(&mut self, address_offset: AddressOffset) -> Result<(), TrapCode> {
                self.execute_store_wrap(address_offset, UntypedValue::$untyped_ident, $type_size)
            }
        )*
    }
}

impl<'a, T> RwasmExecutor<'a, T> {
    impl_visit_load! {
        fn visit_i32_load(i32_load);

        fn visit_i32_load_i8_s(i32_load8_s);
        fn visit_i32_load_i8_u(i32_load8_u);
        fn visit_i32_load_i16_s(i32_load16_s);
        fn visit_i32_load_i16_u(i32_load16_u);
    }

    impl_visit_store! {
        fn visit_i32_store(i32_store, 4);
        fn visit_i32_store_8(i32_store8, 1);
        fn visit_i32_store_16(i32_store16, 2);
    }

    #[inline(always)]
    pub(crate) fn visit_memory_size(&mut self) {
        let result: u32 = self.store.global_memory.current_pages().into();
        self.sp.push_as(result);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_memory_grow(&mut self) -> Result<(), TrapCode> {
        let delta: u32 = self.sp.pop_as();
        let delta = match Pages::new(delta) {
            Some(delta) => delta,
            None => {
                self.sp.push_as(u32::MAX);
                self.ip.add(1);
                return Ok(());
            }
        };
        let new_pages = self
            .store
            .global_memory
            .grow(delta)
            .map(u32::from)
            .unwrap_or(u32::MAX);
        self.sp.push_as(new_pages);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_memory_fill(&mut self) -> Result<(), TrapCode> {
        let (d, val, n) = self.sp.pop3();
        let n = i32::from(n) as usize;
        let offset = i32::from(d) as usize;
        let byte = u8::from(val);
        if self.store.config.fuel_enabled {
            self.store
                .try_consume_fuel(self.store.fuel_costs.fuel_for_bytes(n as u64))?;
        }
        let memory = self
            .store
            .global_memory
            .data_mut()
            .get_mut(offset..)
            .and_then(|memory| memory.get_mut(..n))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        memory.fill(byte);
        #[cfg(feature = "tracing")]
        self.store
            .tracer
            .memory_change(offset as u32, n as u32, memory);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_memory_copy(&mut self) -> Result<(), TrapCode> {
        let (d, s, n) = self.sp.pop3();
        let n = i32::from(n) as usize;
        let src_offset = i32::from(s) as usize;
        let dst_offset = i32::from(d) as usize;
        if self.store.config.fuel_enabled {
            self.store
                .try_consume_fuel(self.store.fuel_costs.fuel_for_bytes(n as u64))?;
        }
        // these accesses just perform the bound checks required by the Wasm spec.
        let data = self.store.global_memory.data_mut();
        data.get(src_offset..)
            .and_then(|memory| memory.get(..n))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        data.get(dst_offset..)
            .and_then(|memory| memory.get(..n))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        data.copy_within(src_offset..src_offset.wrapping_add(n), dst_offset);
        #[cfg(feature = "tracing")]
        self.store.tracer.memory_change(
            dst_offset as u32,
            n as u32,
            &data[dst_offset..(dst_offset + n)],
        );
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_memory_init(
        &mut self,
        data_segment_idx: DataSegmentIdx,
    ) -> Result<(), TrapCode> {
        let is_empty_data_segment = self
            .store
            .empty_data_segments
            .get(data_segment_idx as usize)
            .as_deref()
            .copied()
            .unwrap_or(false);
        let (d, s, n) = self.sp.pop3();
        let n = i32::from(n) as usize;
        let src_offset = i32::from(s) as usize;
        let dst_offset = i32::from(d) as usize;
        if self.store.config.fuel_enabled {
            self.store
                .try_consume_fuel(self.store.fuel_costs.fuel_for_bytes(n as u64))?;
        }
        let memory = self
            .store
            .global_memory
            .data_mut()
            .get_mut(dst_offset..)
            .and_then(|memory| memory.get_mut(..n))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        let mut memory_section = self.module.data_section.as_slice();
        if is_empty_data_segment {
            memory_section = &[];
        }
        let data = memory_section
            .get(src_offset..)
            .and_then(|data| data.get(..n))
            .ok_or(TrapCode::MemoryOutOfBounds)?;
        memory.copy_from_slice(data);
        #[cfg(feature = "tracing")]
        self.store
            .tracer
            .global_memory(dst_offset as u32, n as u32, memory);
        self.ip.add(1);
        Ok(())
    }

    #[inline(always)]
    pub(crate) fn visit_data_drop(&mut self, data_segment_idx: DataSegmentIdx) {
        self.store
            .empty_data_segments
            .set(data_segment_idx as usize, true);
        self.ip.add(1);
    }
}
