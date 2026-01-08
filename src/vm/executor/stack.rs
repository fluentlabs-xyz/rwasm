#[cfg(feature = "tracing")]
use crate::mem::MemoryRecordEnum;
use crate::{CompiledFunc, LocalDepth, RwasmExecutor, UntypedValue};

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_local_get(&mut self, local_depth: LocalDepth) {
        let value = self.sp.nth_back(local_depth as usize);
        self.sp.push(value);
        self.ip.add(1);

        #[cfg(feature = "tracing")]
        self.build_local_trace(local_depth, None);
    }

    #[inline(always)]
    pub(crate) fn visit_local_set(&mut self, local_depth: LocalDepth) {
        let new_value = self.sp.pop();
        self.sp.set_nth_back(local_depth as usize, new_value);
        self.ip.add(1);

        #[cfg(feature = "tracing")]
        self.build_local_trace(local_depth, Some(new_value.to_bits()));
    }

    #[inline(always)]
    pub(crate) fn visit_local_tee(&mut self, local_depth: LocalDepth) {
        let new_value = self.sp.last();
        self.sp.set_nth_back(local_depth as usize, new_value);
        self.ip.add(1);

        #[cfg(feature = "tracing")]
        self.build_local_trace(local_depth, Some(new_value.to_bits()));
    }

    #[inline(always)]
    pub(crate) fn visit_drop(&mut self) {
        self.sp.drop();
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_select(&mut self) {
        self.sp.eval_top3(|e1, e2, e3| {
            let condition = <bool as From<UntypedValue>>::from(e3);
            if condition {
                e1
            } else {
                e2
            }
        });
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_ref_func(&mut self, compiled_func: CompiledFunc) {
        self.sp.push_as(compiled_func);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_i32_const(&mut self, untyped_value: UntypedValue) {
        self.sp.push(untyped_value);
        self.ip.add(1);
    }

    #[cfg(feature = "tracing")]
    fn build_local_trace(&mut self, local_depth: u32, value: Option<u32>) {
        use crate::{mem_index::UNIT, InstrStateExtension, LocalStateExtension};

        let mut instr_state = self.store.tracer.logs.pop().unwrap();

        let local_depth_addr = instr_state.sp + local_depth * UNIT;

        let local_depth_access = if let Some(value) = value {
            self.store.tracer.state.next_cycle();

            MemoryRecordEnum::Write(self.store.tracer.mw(local_depth_addr, value))
        } else {
            MemoryRecordEnum::Read(self.store.tracer.mr(local_depth_addr))
        };

        let state_extension = LocalStateExtension { local_depth_access };

        instr_state.extension = Some(InstrStateExtension::Local(state_extension));

        self.store.tracer.logs.push(instr_state);
    }
}
