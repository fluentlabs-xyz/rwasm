use crate::{core::ValueType, module::ImportName, Func, FuncType};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ImportFuncName(String, String);

impl Into<ImportName> for ImportFuncName {
    fn into(self) -> ImportName {
        ImportName::new(self.0.as_str(), self.1.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ImportFunc {
    import_name: ImportFuncName,
    index: u32,
    func_type: FuncType,
    fuel_amount: u32,
}

impl ImportFunc {
    pub fn new(
        import_name: ImportFuncName,
        index: u32,
        func_type: FuncType,
        fuel_amount: u32,
    ) -> Self {
        Self {
            import_name,
            index,
            func_type,
            fuel_amount,
        }
    }

    pub fn new_env<'a>(
        module_name: String,
        fn_name: String,
        index: u32,
        input: &'a [ValueType],
        output: &'a [ValueType],
        fuel_amount: u32,
    ) -> Self {
        let func_type = FuncType::new_with_refs(input, output);
        Self::new(
            ImportFuncName(module_name, fn_name),
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
    pub func_by_index: BTreeMap<u32, ImportFunc>,
    pub func_by_name: BTreeMap<ImportName, (u32, u32)>,
    pub linked_trampolines: BTreeMap<u32, Func>,
}

impl ImportLinker {
    pub fn insert_function(&mut self, import_func: ImportFunc) {
        if self.func_by_index.contains_key(&import_func.index) {
            return;
        }
        self.func_by_index
            .insert(import_func.index, import_func.clone());
        if self.func_by_name.contains_key(&import_func.import_name()) {
            return;
        }
        self.func_by_name.insert(
            import_func.import_name(),
            (import_func.index, import_func.fuel_amount),
        );
    }

    pub fn resolve_by_index(&self, index: u32) -> Option<&ImportFunc> {
        self.func_by_index.get(&index)
    }

    pub fn index_mapping(&self) -> &BTreeMap<ImportName, (u32, u32)> {
        &self.func_by_name
    }

    pub fn register_trampoline(&mut self, sys_func_index: u32, func: Func) {
        self.linked_trampolines.insert(sys_func_index, func);
    }

    pub fn resolve_trampoline(&self, sys_func_index: u32) -> Option<&Func> {
        self.linked_trampolines.get(&sys_func_index)
    }
}
