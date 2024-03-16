use crate::module::ImportName;
use core::hash::Hash;
use hashbrown::HashMap;

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
    pub fn insert_function<I: Into<u32>>(
        &mut self,
        import_name: ImportName,
        sys_func_index: I,
        fuel_cost: u32,
    ) {
        let last_value = self
            .func_by_name
            .insert(import_name, (sys_func_index.into(), fuel_cost));
        assert!(last_value.is_none(), "import linker name collision");
    }

    pub fn resolve_by_import_name(&self, import_name: &ImportName) -> Option<(u32, u32)> {
        self.func_by_name.get(import_name).copied()
    }
}
