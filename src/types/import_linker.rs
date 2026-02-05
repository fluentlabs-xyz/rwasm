use crate::{intrinsic::Intrinsic, ImportName};
use alloc::vec::Vec;
use hashbrown::HashMap;
use rwasm_fuel_policy::SyscallFuelParams;
use wasmparser::{FuncType, ValType};

#[derive(Debug, Default, Clone)]
pub struct ImportLinker {
    entities: Vec<ImportLinkerEntity>,
    name_to_entity: HashMap<ImportName, usize>,
    idx_to_entity: HashMap<u32, usize>,
}

#[derive(Debug, Clone)]
pub struct ImportLinkerEntity {
    pub sys_func_idx: u32,
    pub syscall_fuel_param: SyscallFuelParams,
    pub params: &'static [ValType],
    pub result: &'static [ValType],
    pub intrinsic: Option<Intrinsic>,
}

impl<const N: usize> From<[(ImportName, ImportLinkerEntity); N]> for ImportLinker {
    fn from(arr: [(ImportName, ImportLinkerEntity); N]) -> Self {
        let mut result = Self::default();
        for (import_name, entity) in arr {
            result.insert_entity(import_name, entity);
        }
        result
    }
}

impl ImportLinkerEntity {
    pub fn matches_func_type(&self, func_type: &FuncType) -> bool {
        if func_type.params().len() != self.params.len()
            || func_type.results().len() != self.result.len()
        {
            return false;
        }
        for (a, b) in func_type.params().iter().zip(self.params.iter()) {
            if a != b {
                return false;
            }
        }
        for (a, b) in func_type.results().iter().zip(self.result.iter()) {
            if a != b {
                return false;
            }
        }
        true
    }
}

#[derive(Debug)]
struct ImportLinkerIter<'a> {
    items: hashbrown::hash_map::Iter<'a, ImportName, usize>,
    entities: &'a Vec<ImportLinkerEntity>,
}

impl<'a> Iterator for ImportLinkerIter<'a> {
    type Item = (ImportName, ImportLinkerEntity);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let (name, offset) = self.items.next()?;
        let entity = self.entities.get(*offset)?;
        Some((name.clone(), entity.clone()))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.items.size_hint()
    }
}

impl ImportLinker {
    pub fn insert_function(
        &mut self,
        import_name: ImportName,
        sys_func_idx: u32,
        syscall_fuel_param: SyscallFuelParams,
        params: &'static [ValType],
        result: &'static [ValType],
    ) {
        self.insert_entity(
            import_name,
            ImportLinkerEntity {
                sys_func_idx,
                syscall_fuel_param,
                params,
                result,
                intrinsic: None,
            },
        );
    }

    pub fn insert_intrinsic(
        &mut self,
        import_name: ImportName,
        sys_func_idx: u32,
        intrinsic: Intrinsic,
        params: &'static [ValType],
        result: &'static [ValType],
    ) {
        self.insert_entity(
            import_name,
            ImportLinkerEntity {
                sys_func_idx,
                syscall_fuel_param: Default::default(),
                params,
                result,
                intrinsic: Some(intrinsic),
            },
        );
    }

    pub fn iter(&self) -> impl Iterator<Item = (ImportName, ImportLinkerEntity)> + use<'_> {
        ImportLinkerIter {
            items: self.name_to_entity.iter(),
            entities: &self.entities,
        }
    }

    pub fn insert_entity(&mut self, import_name: ImportName, entity: ImportLinkerEntity) {
        let sys_func_idx = entity.sys_func_idx;
        if self.name_to_entity.contains_key(&import_name) {
            panic!("import linker name collision: {}", import_name)
        } else if self.idx_to_entity.contains_key(&sys_func_idx) {
            panic!("import linker name collision: {}", import_name)
        }
        let index = self.entities.len();
        self.entities.push(entity);
        self.name_to_entity.insert(import_name, index);
        self.idx_to_entity.insert(sys_func_idx, index);
    }

    pub fn find_symbols(&self) -> Vec<ImportName> {
        let mut symbols: Vec<ImportName> = self.name_to_entity.keys().cloned().collect();
        symbols.sort();
        symbols
    }

    pub fn resolve_by_import_name(&self, import_name: &ImportName) -> Option<&ImportLinkerEntity> {
        let index = self.name_to_entity.get(import_name).copied()?;
        self.entities.get(index)
    }

    pub fn resolve_by_func_idx(&self, sys_func_idx: u32) -> Option<&ImportLinkerEntity> {
        let index = self.idx_to_entity.get(&sys_func_idx).copied()?;
        self.entities.get(index)
    }
}
