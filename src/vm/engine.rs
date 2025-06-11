use crate::{
    execute_rwasm_module,
    CallStack,
    RwasmExecutor,
    RwasmModule,
    Store,
    TrapCode,
    ValueStack,
};

pub struct ExecutionEngine {
    value_stack: ValueStack,
    call_stack: CallStack,
}

impl ExecutionEngine {
    pub fn new() -> Self {
        Self {
            value_stack: ValueStack::default(),
            call_stack: CallStack::default(),
        }
    }

    pub fn value_stack(&mut self) -> &mut ValueStack {
        &mut self.value_stack
    }

    pub fn call_stack(&mut self) -> &mut CallStack {
        &mut self.call_stack
    }

    pub fn create_executor<'a, T>(
        &'a mut self,
        store: &'a mut Store<T>,
        module: &'a RwasmModule,
    ) -> RwasmExecutor<'a, T> {
        RwasmExecutor::new(&module, &mut self.value_stack, &mut self.call_stack, store)
    }

    pub fn execute<T>(
        &mut self,
        store: &mut Store<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        execute_rwasm_module(module, &mut self.value_stack, &mut self.call_stack, store)
    }

    pub fn reset<T>(&mut self, store: &mut Store<T>, keep_flags: bool) {
        self.value_stack.reset();
        self.call_stack.reset();
        store.reset(keep_flags)
    }
}
