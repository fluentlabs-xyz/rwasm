use crate::{
    constraint_builder::{AdviceColumn, ToExpr},
    exec_step::{ExecStep, GadgetError},
    runtime_circuit::{
        constraint_builder::OpConstraintBuilder,
        execution_state::ExecutionState,
        opcodes::ExecutionGadget,
    },
    util::Field,
};
use fluentbase_runtime::SysFuncIdx;
use halo2_proofs::circuit::Region;
use std::marker::PhantomData;

#[derive(Clone)]
pub struct WasiFdWriteGadget<F: Field> {
    fd: AdviceColumn,
    iovs_ptr: AdviceColumn,
    iovs_len: AdviceColumn,
    rp0_ptr: AdviceColumn,
    pd: PhantomData<F>,
}

impl<F: Field> ExecutionGadget<F> for WasiFdWriteGadget<F> {
    const NAME: &'static str = "WASM_CALL_HOST(wasi_snapshot_preview1::fd_write)";
    const EXECUTION_STATE: ExecutionState =
        ExecutionState::WASM_CALL_HOST(SysFuncIdx::WASI_FD_WRITE);

    fn configure(cb: &mut OpConstraintBuilder<F>) -> Self {
        let fd = cb.query_cell();
        let iovs_ptr = cb.query_cell();
        let iovs_len = cb.query_cell();
        let rp0_ptr = cb.query_cell();
        cb.stack_pop(rp0_ptr.current());
        cb.stack_pop(iovs_len.current());
        cb.stack_pop(iovs_ptr.current());
        cb.stack_pop(fd.current());
        // always push error
        cb.stack_push(wasi::ERRNO_CANCELED.raw().expr());
        Self {
            fd,
            iovs_ptr,
            iovs_len,
            rp0_ptr,
            pd: Default::default(),
        }
    }

    fn assign_exec_step(
        &self,
        region: &mut Region<'_, F>,
        offset: usize,
        trace: &ExecStep,
    ) -> Result<(), GadgetError> {
        let rp0_ptr = trace.curr_nth_stack_value(0)?;
        self.rp0_ptr.assign(region, offset, rp0_ptr.as_u64());
        let iovs_len = trace.curr_nth_stack_value(1)?;
        self.iovs_len.assign(region, offset, iovs_len.as_u64());
        let iovs_ptr = trace.curr_nth_stack_value(2)?;
        self.iovs_ptr.assign(region, offset, iovs_ptr.as_u64());
        let fd = trace.curr_nth_stack_value(3)?;
        self.fd.assign(region, offset, fd.as_u64());
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::runtime_circuit::testing::test_ok;
    use fluentbase_runtime::SysFuncIdx;
    use fluentbase_rwasm::instruction_set;

    #[test]
    fn test_exit() {
        test_ok(instruction_set! {
            I32Const(0)
            I32Const(0)
            I32Const(0)
            Call(SysFuncIdx::WASI_FD_WRITE)
            Drop
        });
    }
}