use crate::{core::ValueType, module::ImportName, FuncType};
use hashbrown::{hash_map::DefaultHashBuilder, HashMap};
use std::hash::Hash;

#[derive(Debug, Clone, Hash)]
pub struct ImportFunc {
    import_name: ImportName,
    index: u32,
    func_type: FuncType,
    fuel_amount: u32,
}

impl ImportFunc {
    pub fn new(import_name: ImportName, index: u32, func_type: FuncType, fuel_amount: u32) -> Self {
        Self {
            import_name,
            index,
            func_type,
            fuel_amount,
        }
    }

    pub fn new_env<'a>(
        module_name: &str,
        fn_name: &str,
        index: u32,
        input: &'a [ValueType],
        output: &'a [ValueType],
        fuel_amount: u32,
    ) -> Self {
        let func_type = FuncType::new_with_refs(input, output);
        Self::new(
            ImportName::new(module_name, fn_name),
            index,
            func_type,
            fuel_amount,
        )
    }

    pub fn import_name(&self) -> ImportName {
        self.clone().import_name.into()
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn func_type(&self) -> &FuncType {
        &self.func_type
    }
}

#[derive(Debug, Default, Clone)]
pub struct ImportLinker {
    func_by_name: HashMap<ImportName, (u32, u32)>,
}

impl<const N: usize> From<[(ImportName, (u32, u32)); N]> for ImportLinker {
    fn from(arr: [(ImportName, (u32, u32)); N]) -> Self {
        Self {
            func_by_name: HashMap::from(arr),
        }
    }
}

impl ImportLinker {
    pub fn insert_function(&mut self, import_func: ImportFunc) {
        if self.func_by_name.contains_key(&import_func.import_name()) {
            return;
        }
        self.func_by_name.insert(
            import_func.import_name(),
            (import_func.index, import_func.fuel_amount),
        );
    }

    pub fn index_mapping(&self) -> &HashMap<ImportName, (u32, u32)> {
        &self.func_by_name
    }
}
