use crate::{
    CallStack, InstructionPtr, ReusableContext, RwasmExecutor, RwasmModule, RwasmStore, TrapCode,
    Value, ValueStack,
};
use alloc::sync::Arc;
use core::mem::take;
use spin::Mutex;

/// Represents the core execution engine for managing the execution of a program,
/// including the handling of values and function calls.
#[derive(Default, Clone)]
pub struct ExecutionEngine {
    inner: Arc<Mutex<ExecutionEngineInner>>,
}

impl ExecutionEngine {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub fn entrypoint<T>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.entrypoint(store, module)
    }

    #[inline(always)]
    pub fn execute<T>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.execute(store, module, params, result)
    }

    #[inline(always)]
    pub fn resume<T>(
        &self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.resume(store, params, result)
    }

    #[inline(always)]
    pub(crate) fn resume_for_module<T>(
        &self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut ctx = self.inner.lock();
        ctx.resume_for_module(store, module, params, result)
    }
}

#[derive(Default)]
struct ExecutionEngineInner {
    // we should store a reusable stack here
}

impl ExecutionEngineInner {
    pub(crate) fn entrypoint<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
    ) -> Result<(), TrapCode> {
        let mut value_stack = ValueStack::default();
        let mut call_stack = CallStack::default();
        if store.resumable_context.is_some() {
            return Err(TrapCode::AlreadySuspended);
        }
        let mut executor =
            RwasmExecutor::entrypoint(module, &mut value_stack, &mut call_stack, store);
        match executor.run(&[], &mut []) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(module.clone(), store, value_stack, call_stack, ip)
            }
            res => res,
        }
    }

    /// Executes a rWasm module's function with the given parameters and stores the result.
    pub(crate) fn execute<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let mut value_stack = ValueStack::default();
        let mut call_stack = CallStack::default();
        if store.resumable_context.is_some() {
            return Err(TrapCode::AlreadySuspended);
        }
        let sp = value_stack.stack_ptr();
        let mut ip = InstructionPtr::new(module.code_section.as_ptr());
        debug_assert!(module.source_pc < module.code_section.len() as u32);
        ip.offset(module.source_pc as isize);
        let mut executor =
            RwasmExecutor::new(module, &mut value_stack, sp, &mut call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(module.clone(), store, value_stack, call_stack, ip)
            }
            res => res,
        }
    }

    /// Resumes the execution of a WASM (WebAssembly) function that was previously interrupted.
    pub(crate) fn resume<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.resume_checked(store, None, params, result)
    }

    pub(crate) fn resume_for_module<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        module: &RwasmModule,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        self.resume_checked(store, Some(module), params, result)
    }

    fn resume_checked<T>(
        &mut self,
        store: &mut RwasmStore<T>,
        expected_module: Option<&RwasmModule>,
        params: &[Value],
        result: &mut [Value],
    ) -> Result<(), TrapCode> {
        let context = store
            .resumable_context
            .as_ref()
            .ok_or(TrapCode::NotSuspended)?;
        if expected_module.is_some_and(|module| !context.module.has_same_identity(module)) {
            return Err(TrapCode::WrongInstance);
        }

        let ReusableContext {
            module,
            mut call_stack,
            instruction_index,
            mut value_stack,
        } = take(&mut store.resumable_context).ok_or(TrapCode::NotSuspended)?;
        let instruction_index = instruction_index as usize;
        if instruction_index >= module.code_section.len() {
            return Err(TrapCode::IncompatibleModule);
        }
        let mut ip = InstructionPtr::new(module.code_section.as_ptr());
        ip.add(instruction_index);
        let sp = value_stack.stack_ptr();
        let mut executor =
            RwasmExecutor::new(&module, &mut value_stack, sp, &mut call_stack, ip, store);
        match executor.run(params, result) {
            Err(TrapCode::InterruptionCalled) => {
                let (ip, sp) = (executor.ip, executor.sp);
                value_stack.sync_stack_ptr(sp);
                self.remember_context(module, store, value_stack, call_stack, ip)
            }
            res => res,
        }
    }

    fn remember_context<T>(
        &mut self,
        module: RwasmModule,
        store: &mut RwasmStore<T>,
        value_stack: ValueStack,
        call_stack: CallStack,
        ip: InstructionPtr,
    ) -> Result<(), TrapCode> {
        let base = module.code_section.as_ptr() as usize;
        let current = ip.ptr as usize;
        let byte_offset = current
            .checked_sub(base)
            .ok_or(TrapCode::IncompatibleModule)?;
        if byte_offset % size_of::<crate::Opcode>() != 0 {
            return Err(TrapCode::IncompatibleModule);
        }
        let instruction_index = byte_offset / size_of::<crate::Opcode>();
        if instruction_index >= module.code_section.len() {
            return Err(TrapCode::IncompatibleModule);
        }
        let instruction_index =
            u32::try_from(instruction_index).map_err(|_| TrapCode::IncompatibleModule)?;
        store.resumable_context = Some(ReusableContext {
            module,
            call_stack,
            instruction_index,
            value_stack,
        });
        Err(TrapCode::InterruptionCalled)
    }
}

impl ExecutionEngine {
    pub fn acquire_shared() -> ExecutionEngine {
        static ENGINE: spin::Once<ExecutionEngine> = spin::Once::new();
        ENGINE.call_once(ExecutionEngine::default).clone()
    }
}
