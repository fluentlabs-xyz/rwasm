use crate::{
    engine::bytecode::{BlockFuel, FuncIdx},
    module::ImportName,
    FuncType,
};
use hashbrown::HashMap;

#[derive(Debug, Default, Clone)]
pub struct ImportLinker {
    func_by_name: HashMap<ImportName, (FuncIdx, BlockFuel, FuncType)>,
}

impl<const N: usize> From<[(&'static str, &'static str, u32, BlockFuel, FuncType); N]>
    for ImportLinker
{
    fn from(arr: [(&'static str, &'static str, u32, BlockFuel, FuncType); N]) -> Self {
        Self {
            func_by_name: HashMap::from_iter(arr.into_iter().map(
                |(module_name, fn_name, func_index, fuel_cost, func_type)| {
                    (
                        ImportName::new(module_name, fn_name),
                        (FuncIdx::from(func_index), fuel_cost, func_type),
                    )
                },
            )),
        }
    }
}

impl<const N: usize> From<[(ImportName, (FuncIdx, BlockFuel, FuncType)); N]> for ImportLinker {
    fn from(arr: [(ImportName, (FuncIdx, BlockFuel, FuncType)); N]) -> Self {
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
        fuel_cost: BlockFuel,
        func_type: FuncType,
    ) {
        let last_value = self.func_by_name.insert(
            import_name,
            (FuncIdx::from(sys_func_index.into()), fuel_cost, func_type),
        );
        assert!(last_value.is_none(), "import linker name collision");
    }

    pub fn resolve_by_import_name(
        &self,
        import_name: &ImportName,
    ) -> Option<&(FuncIdx, BlockFuel, FuncType)> {
        self.func_by_name.get(import_name)
    }
}
