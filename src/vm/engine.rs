use crate::{
    execute_rwasm_module,
    CallStack,
    RwasmExecutor,
    RwasmModule,
    Store,
    TrapCode,
    ValueStack,
};

pub struct ExecutionEngine<'a, T> {
    store: &'a mut Store<T>,
    value_stack: ValueStack,
    call_stack: CallStack,
}

impl<'a, T> ExecutionEngine<'a, T> {
    pub fn new(store: &'a mut Store<T>) -> Self {
        Self {
            store,
            value_stack: ValueStack::default(),
            call_stack: CallStack::default(),
        }
    }

    pub fn store(&mut self) -> &mut Store<T> {
        self.store
    }

    pub fn value_stack(&mut self) -> &mut ValueStack {
        &mut self.value_stack
    }

    pub fn call_stack(&mut self) -> &mut CallStack {
        &mut self.call_stack
    }

    pub fn execute(&mut self, module: &RwasmModule) -> Result<(), TrapCode> {
        execute_rwasm_module(
            module,
            &mut self.value_stack,
            &mut self.call_stack,
            &mut self.store,
        )
    }

    pub fn reset(&mut self, keep_flags: bool) {
        self.value_stack.reset();
        self.call_stack.reset();
        self.store.reset(keep_flags)
    }
}
