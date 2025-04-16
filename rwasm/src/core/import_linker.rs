use crate::{
    engine::bytecode::{BlockFuel, FuncIdx},
    module::ImportName,
    FuncType,
};
use hashbrown::HashMap;

#[derive(Debug, Default, Clone)]
pub struct ImportLinker {
    func_by_name: HashMap<ImportName, ImportLinkerEntity>,
}

#[derive(Debug, Clone)]
pub struct ImportLinkerEntity {
    pub func_idx: FuncIdx,
    pub block_fuel: BlockFuel,
    pub func_type: FuncType,
}

impl ImportLinkerEntity {
    pub const fn new(func_idx: FuncIdx, block_fuel: BlockFuel, func_type: FuncType) -> Self {
        Self {
            func_idx,
            block_fuel,
            func_type,
        }
    }
}

impl<const N: usize> From<[(&'static str, &'static str, ImportLinkerEntity); N]> for ImportLinker {
    fn from(arr: [(&'static str, &'static str, ImportLinkerEntity); N]) -> Self {
        Self {
            func_by_name: HashMap::from_iter(arr.into_iter().map(
                |(module_name, fn_name, entity)| (ImportName::new(module_name, fn_name), entity),
            )),
        }
    }
}

impl<const N: usize> From<[(ImportName, ImportLinkerEntity); N]> for ImportLinker {
    fn from(arr: [(ImportName, ImportLinkerEntity); N]) -> Self {
        Self {
            func_by_name: HashMap::from(arr),
        }
    }
}

impl ImportLinker {
    pub fn insert_function<I: Into<u32>>(
        &mut self,
        import_name: ImportName,
        sys_func_index: I,
        block_fuel: BlockFuel,
        func_type: FuncType,
    ) {
        let last_value = self.func_by_name.insert(
            import_name,
            ImportLinkerEntity::new(FuncIdx::from(sys_func_index.into()), block_fuel, func_type),
        );
        assert!(last_value.is_none(), "import linker name collision");
    }

    pub fn resolve_by_import_name(&self, import_name: &ImportName) -> Option<&ImportLinkerEntity> {
        self.func_by_name.get(import_name)
    }
}
