use rwasm::{Caller, Store, TrapCode, TypedCaller, Value};

pub const FUNC_ENTRYPOINT: u32 = u32::MAX;
pub const FUNC_PRINT: u32 = 100;
pub const FUNC_PRINT_I32: u32 = 101;
pub const FUNC_PRINT_I64: u32 = 102;
pub const FUNC_PRINT_F32: u32 = 103;
pub const FUNC_PRINT_F64: u32 = 104;
pub const FUNC_PRINT_I32_F32: u32 = 105;
pub const FUNC_PRINT_I64_F64: u32 = 106;
pub const FUNC_GLOBAL_I32: u32 = 107;

#[derive(Default)]
pub struct TestingContext {
    pub program_counter: u32,
    pub state: u32,
}

pub(crate) fn testing_context_syscall_handler(
    caller: &mut TypedCaller<TestingContext>,
    func_idx: u32,
    params: &[Value],
    _result: &mut [Value],
) -> Result<(), TrapCode> {
    match func_idx {
        FUNC_PRINT => {
            println!("print");
            Ok(())
        }
        FUNC_PRINT_I32 => {
            let value = params[0].i32().unwrap();
            println!("print: {value}");
            Ok(())
        }
        FUNC_PRINT_I64 => {
            let value = params[0].i64().unwrap();
            println!("print: {value}");
            Ok(())
        }
        FUNC_PRINT_F32 => {
            let value = params[0].f32().unwrap();
            println!("print: {value}");
            Ok(())
        }
        FUNC_PRINT_F64 => {
            let value = params[0].f64().unwrap();
            println!("print: {value}");
            Ok(())
        }
        FUNC_PRINT_I32_F32 => {
            let v0 = params[0].i32().unwrap();
            let v1 = params[1].f32().unwrap();
            println!("print: {:?} {:?}", v0, f32::from(v1));
            Ok(())
        }
        FUNC_PRINT_I64_F64 => {
            let v0 = params[0].i64().unwrap();
            let v1 = params[1].f64().unwrap();
            println!("print: {:?} {:?}", v0, f64::from(v1));
            Ok(())
        }
        FUNC_ENTRYPOINT => {
            // yeah, dirty, but this is how we remember the program counter to reset,
            // since we're 100% sure the function is called using `Call`
            // that we can safely deduct 1 from PC (for `ReturnCall` we need to deduct 2)
            let pc = caller.program_counter();
            caller.data_mut().program_counter = pc - 1;
            // push state value into the stack
            let state = caller.data().state;
            caller.stack_push(state.into());
            Ok(())
        }
        FUNC_GLOBAL_I32 => {
            caller.stack_push(666.into());
            Ok(())
        }
        _ => todo!("not implemented syscall handler"),
    }
}
