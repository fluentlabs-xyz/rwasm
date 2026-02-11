use rwasm::{
    ImportLinker, ImportLinkerEntity, ImportName, StoreTr, TrapCode, TypedCaller, ValType, Value,
};
use std::sync::{Arc, OnceLock};

pub const FUNC_PRINT: u32 = 100;
pub const FUNC_PRINT_I32: u32 = 101;
pub const FUNC_PRINT_I64: u32 = 102;
pub const FUNC_PRINT_F32: u32 = 103;
pub const FUNC_PRINT_F64: u32 = 104;
pub const FUNC_PRINT_I32_F32: u32 = 105;
pub const FUNC_PRINT_I64_F64: u32 = 106;
pub const FUNC_GET_STATE: u32 = 107;

#[derive(Default)]
pub struct TestingContext {
    pub state: u32,
}

pub(crate) fn create_import_linker() -> Arc<ImportLinker> {
    static IMPORT_LINKER: OnceLock<Arc<ImportLinker>> = OnceLock::new();
    let import_linker_ref = IMPORT_LINKER.get_or_init(|| {
        ImportLinker::from([
            (
                ImportName::new("spectest", "print"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT,
                    syscall_fuel_param: Default::default(),
                    params: &[],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F32,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::F32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_F64,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::F64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i32_f32"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I32_F32,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I32, ValType::F32],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "print_i64_f64"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_PRINT_I64_F64,
                    syscall_fuel_param: Default::default(),
                    params: &[ValType::I64, ValType::F64],
                    result: &[],
                    intrinsic: None,
                },
            ),
            (
                ImportName::new("spectest", "get_state"),
                ImportLinkerEntity {
                    sys_func_idx: FUNC_GET_STATE,
                    syscall_fuel_param: Default::default(),
                    params: &[],
                    result: &[ValType::I32],
                    intrinsic: None,
                },
            ),
        ])
        .into()
    });
    import_linker_ref.clone()
}

pub(crate) fn testing_context_syscall_handler(
    caller: &mut TypedCaller<TestingContext>,
    func_idx: u32,
    params: &[Value],
    result: &mut [Value],
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
        FUNC_GET_STATE => {
            let state = caller.data_mut().state;
            result[0] = Value::I32(state as i32);
            Ok(())
        }
        _ => unimplemented!("not implemented syscall handler: {}", func_idx),
    }
}
