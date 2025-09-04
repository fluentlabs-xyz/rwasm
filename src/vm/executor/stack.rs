use crate::{CompiledFunc, LocalDepth, RwasmExecutor, UntypedValue};

impl<'a, T: Send + Sync> RwasmExecutor<'a, T> {
    #[inline(always)]
    pub(crate) fn visit_local_get(&mut self, local_depth: LocalDepth) {
        let value = self.sp.nth_back(local_depth as usize);
        self.sp.push(value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_local_set(&mut self, local_depth: LocalDepth) {
        let new_value = self.sp.pop();
        self.sp.set_nth_back(local_depth as usize, new_value);
        self.ip.add(1);
    }

    #[inline(always)]
    pub(crate) fn visit_local_tee(&mut self, local_depth: LocalDepth) {
        let new_value = self.sp.last();
        self.sp.set_nth_back(local_depth as usize, new_value);
        self.ip.add(1);
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
}
