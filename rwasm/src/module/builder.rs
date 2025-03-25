use super::{
    export::ExternIdx,
    import::FuncTypeIdx,
    ConstExpr,
    CustomSectionsBuilder,
    DataSegment,
    DataSegmentKind,
    ElementSegment,
    ElementSegmentItems,
    ElementSegmentKind,
    ExternTypeIdx,
    FuncIdx,
    Global,
    GlobalIdx,
    Import,
    ImportName,
    Module,
};
use crate::{
    core::{
        ValueType,
        N_MAX_DATA_SEGMENTS,
        N_MAX_ELEM_SEGMENTS,
        N_MAX_GLOBALS,
        N_MAX_MEMORY_PAGES,
        N_MAX_TABLES,
        N_MAX_TABLE_ELEMENTS,
    },
    engine::{CompiledFunc, DedupFuncType},
    errors::ModuleError,
    Engine,
    FuncType,
    GlobalType,
    MemoryType,
    Mutability,
    TableType,
};
use alloc::{boxed::Box, collections::BTreeMap, vec::Vec};

/// A builder for a WebAssembly [`Module`].
#[derive(Debug)]
pub struct ModuleBuilder<'engine> {
    engine: &'engine Engine,
    pub func_types: Vec<DedupFuncType>,
    pub imports: ModuleImports,
    pub funcs: Vec<DedupFuncType>,
    pub tables: Vec<TableType>,
    pub memories: Vec<MemoryType>,
    pub globals: Vec<GlobalType>,
    pub globals_init: Vec<ConstExpr>,
    pub exports: BTreeMap<Box<str>, ExternIdx>,
    pub start: Option<FuncIdx>,
    pub compiled_funcs: Vec<CompiledFunc>,
    pub element_segments: Vec<ElementSegment>,
    pub data_segments: Vec<DataSegment>,
    pub import_mapping: BTreeMap<u32, FuncIdx>,
    pub custom_sections: CustomSectionsBuilder,
}

/// The import names of the [`Module`] imports.
#[derive(Debug, Default)]
pub struct ModuleImports {
    pub funcs: Vec<ImportName>,
    pub tables: Vec<ImportName>,
    pub memories: Vec<ImportName>,
    pub globals: Vec<ImportName>,
}

impl ModuleImports {
    /// Returns the number of imported global variables.
    pub fn len_globals(&self) -> usize {
        self.globals.len()
    }

    /// Returns the number of imported functions.
    pub fn len_funcs(&self) -> usize {
        self.funcs.len()
    }

    /// Returns the number of imported tables.
    pub fn len_tables(&self) -> usize {
        self.tables.len()
    }
    /// Returns the number of imported memories.
    pub fn len_memories(&self) -> usize {
        self.memories.len()
    }
}

/// The resources of a [`Module`] required for translating function bodies.
#[derive(Debug, Copy, Clone)]
pub struct ModuleResources<'a> {
    pub(crate) res: &'a ModuleBuilder<'a>,
}

impl<'a> ModuleResources<'a> {
    /// Returns the [`Engine`] of the [`ModuleResources`].
    pub fn engine(&'a self) -> &'a Engine {
        self.res.engine
    }

    /// Creates new [`ModuleResources`] from the given [`ModuleBuilder`].
    pub fn new(res: &'a ModuleBuilder) -> Self {
        Self { res }
    }

    /// Returns the [`FuncType`] at the given index.
    pub fn get_func_type(&self, func_type_idx: FuncTypeIdx) -> &DedupFuncType {
        &self.res.func_types[func_type_idx.into_u32() as usize]
    }

    /// Returns the [`FuncType`] of the indexed function.
    pub fn get_type_of_func(&self, func_idx: FuncIdx) -> &DedupFuncType {
        &self.res.funcs[func_idx.into_u32() as usize]
    }

    /// Returns the [`GlobalType`] the the indexed global variable.
    pub fn get_type_of_global(&self, global_idx: GlobalIdx) -> GlobalType {
        self.res.globals[global_idx.into_u32() as usize]
    }

    /// Returns the [`CompiledFunc`] for the given [`FuncIdx`].
    ///
    /// Returns `None` if [`FuncIdx`] refers to an imported function.
    pub fn get_compiled_func(&self, func_idx: FuncIdx) -> Option<CompiledFunc> {
        let index = if let Some(rwasm_config) = self.engine().config().get_rwasm_config() {
            if rwasm_config.wrap_import_functions {
                // if we wrap import functions then just return index, there is no intersection
                // between imports and compiled functions
                func_idx.into_u32() as usize
            } else {
                let index =
                    (func_idx.into_u32() as usize).checked_sub(self.res.imports.len_funcs())?;
                // otherwise, we must disallow accessing entrypoint
                if index == self.res.compiled_funcs.len() - 1 {
                    return None;
                }
                // return adjusted compiled func index
                index
            }
        } else {
            (func_idx.into_u32() as usize).checked_sub(self.res.imports.len_funcs())?
        };
        // Note: It is a bug if this index access is out of bounds
        //       therefore we panic here instead of using `get`.
        Some(self.res.compiled_funcs[index])
    }

    /// Returns the global variable type and optional initial value.
    pub fn get_global(&self, global_idx: GlobalIdx) -> (GlobalType, Option<&ConstExpr>) {
        let index = global_idx.into_u32() as usize;
        let len_imports = self.res.imports.len_globals();
        let global_type = self.get_type_of_global(global_idx);
        if index < len_imports {
            // The index refers to an imported global without init value.
            (global_type, None)
        } else {
            // The index refers to an internal global with init value.
            let init_expr = &self.res.globals_init[index - len_imports];
            (global_type, Some(init_expr))
        }
    }
}

impl<'engine> ModuleBuilder<'engine> {
    /// Creates a new [`ModuleBuilder`] for the given [`Engine`].
    pub fn new(engine: &'engine Engine) -> Self {
        Self {
            engine,
            func_types: Vec::new(),
            imports: ModuleImports::default(),
            funcs: Vec::new(),
            tables: Vec::new(),
            memories: Vec::new(),
            globals: Vec::new(),
            globals_init: Vec::new(),
            exports: BTreeMap::new(),
            start: None,
            compiled_funcs: Vec::new(),
            element_segments: Vec::new(),
            data_segments: Vec::new(),
            import_mapping: BTreeMap::new(),
            custom_sections: CustomSectionsBuilder::default(),
        }
    }

    /// Returns a shared reference to the [`Engine`] of the [`Module`] under construction.
    pub fn engine(&self) -> &Engine {
        self.engine
    }

    /// Pushes the given function types to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a function type fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_func_types<T>(&mut self, func_types: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<FuncType, ModuleError>>,
    {
        assert!(
            self.func_types.is_empty(),
            "tried to initialize module function types twice"
        );
        for func_type in func_types {
            let func_type = func_type?;
            let dedup = self.engine.alloc_func_type(func_type);
            self.func_types.push(dedup)
        }
        Ok(())
    }

    pub(crate) fn ensure_empty_func_type_exists(&mut self) -> FuncTypeIdx {
        if self.engine.config().get_i32_translator() {
            self.ensure_func_type_index(FuncType::new::<_, _, true>([], []))
        } else {
            self.ensure_func_type_index(FuncType::new::<_, _, false>([], []))
        }
    }

    pub(crate) fn ensure_func_type_index(&mut self, func_type: FuncType) -> FuncTypeIdx {
        // try to find func type inside module builder
        let found_type = self
            .func_types
            .iter()
            .enumerate()
            .map(|(i, t)| (i, self.engine.resolve_func_type(t, |t| t.clone())))
            .find(|(_, t)| *t == func_type);
        if let Some(func_type) = found_type {
            return FuncTypeIdx::from(func_type.0 as u32);
        }
        // try to find inside engine
        // if let Some(dedup_func_type) = self.engine.find_func_type(&func_type) {
        //     let type_index = self.func_types.len() as u32;
        //     self.func_types.push(dedup_func_type);
        //     return FuncTypeIdx::from(type_index);
        // }
        // create new func type
        let empty_func_type = if self.engine.config().get_i32_translator() {
            FuncType::new::<_, _, true>([], [])
        } else {
            FuncType::new::<_, _, false>([], [])
        };

        self.func_types
            .push(self.engine.alloc_func_type(empty_func_type.clone()));
        return FuncTypeIdx::from(self.func_types.len() as u32 - 1);
    }

    pub fn push_func_type(&mut self, func_type: FuncType) -> Result<(), ModuleError> {
        let dedup = self.engine.alloc_func_type(func_type);
        self.func_types.push(dedup);
        Ok(())
    }

    /// Pushes the given imports to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If an import fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_imports<T>(&mut self, imports: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<Import, ModuleError>>,
    {
        for import in imports {
            let import = import?;
            let (name, kind) = import.into_name_and_type();
            match kind {
                ExternTypeIdx::Func(func_type_idx) => {
                    self.imports.funcs.push(name);
                    let func_type = self.func_types[func_type_idx.into_u32() as usize];
                    self.funcs.push(func_type);
                    // for rWASM we store special compiled wrapper, it's needed for tables
                    if self.engine.config().get_rwasm_wrap_import_funcs() {
                        self.compiled_funcs.push(self.engine.alloc_func());
                    }
                }
                ExternTypeIdx::Table(table_type) => {
                    self.imports.tables.push(name);
                    self.tables.push(table_type);
                }
                ExternTypeIdx::Memory(memory_type) => {
                    self.imports.memories.push(name);
                    self.memories.push(memory_type);
                }
                ExternTypeIdx::Global(global_type) => {
                    self.imports.globals.push(name);
                    self.globals.push(global_type);
                }
            }
        }
        Ok(())
    }

    pub fn push_entrypoint(&mut self) -> (FuncIdx, CompiledFunc) {
        // resolve empty func type of create if its missing
        let func_type_index = self.ensure_empty_func_type_exists().into_u32() as usize;
        let empty_func_type = self.func_types[func_type_index];
        // push new func and compiled func for our entrypoint
        self.funcs.push(empty_func_type);
        self.compiled_funcs.push(self.engine.alloc_func());
        // resolve compiled func
        let index = self.compiled_funcs.len() - 1;
        let compiled_func = self.compiled_funcs[index];
        // resolve func index
        let func_idx = FuncIdx::from(self.funcs.len() as u32 - 1);
        (func_idx, compiled_func)
    }

    pub fn push_function_import(
        &mut self,
        name: ImportName,
        func_type_idx: FuncTypeIdx,
    ) -> Result<(), ModuleError> {
        self.imports.funcs.push(name);
        let func_type = self.func_types[func_type_idx.into_u32() as usize];
        self.funcs.push(func_type);
        Ok(())
    }

    /// Pushes the given function declarations to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a function declaration fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_funcs<T>(&mut self, funcs: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<FuncTypeIdx, ModuleError>>,
    {
        assert_eq!(
            self.funcs.len(),
            self.imports.funcs.len(),
            "tried to initialize module function declarations twice"
        );
        for func in funcs {
            let func_type_idx = func?;
            let func_type = self.func_types[func_type_idx.into_u32() as usize];
            self.funcs.push(func_type);
            self.compiled_funcs.push(self.engine.alloc_func());
        }
        Ok(())
    }

    /// Pushes the given table types to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a table declaration fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_tables<T>(&mut self, tables: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<TableType, ModuleError>>,
    {
        assert_eq!(
            self.tables.len(),
            self.imports.tables.len(),
            "tried to initialize module table declarations twice"
        );
        for table in tables {
            let table = table?;
            self.tables.push(table);
        }
        Ok(())
    }

    /// Pushes the given linear memory types to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a linear memory declaration fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_memories<T>(&mut self, memories: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<MemoryType, ModuleError>>,
    {
        assert_eq!(
            self.memories.len(),
            self.imports.memories.len(),
            "tried to initialize module linear memory declarations twice"
        );
        for memory in memories {
            let memory = memory?;
            self.memories.push(memory);
        }
        Ok(())
    }

    pub fn push_default_memory(&mut self, initial: u32, maximum: Option<u32>) {
        self.memories
            .push(MemoryType::new(initial, maximum).unwrap());
    }

    /// Pushes the given global variables to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If a global variable declaration fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_globals<T>(&mut self, globals: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<Global, ModuleError>>,
    {
        assert_eq!(
            self.globals.len(),
            self.imports.globals.len(),
            "tried to initialize module global variable declarations twice"
        );
        for global in globals {
            let global = global?;
            let (global_decl, global_init) = global.into_type_and_init();
            self.globals.push(global_decl);
            self.globals_init.push(global_init);
        }
        Ok(())
    }

    pub fn push_rwasm_globals(&mut self) {
        let global_decl = GlobalType::new(ValueType::I64, Mutability::Var);
        (0..N_MAX_GLOBALS).for_each(|_| {
            self.globals.push(global_decl);
            self.globals_init.push(ConstExpr::zero());
        });
    }

    pub fn rewrite_memory(&mut self) -> Result<(), ModuleError> {
        // rewrite memory section
        let max_memory_pages = self
            .memories
            .get(0)
            .and_then(|memory| memory.maximum_pages().map(|v| v.into_inner()))
            .unwrap_or(N_MAX_MEMORY_PAGES);
        self.memories.clear();
        self.memories
            .push(MemoryType::new(0, Some(max_memory_pages)).unwrap());
        // calc one big passive data section
        let mut data_section = Vec::with_capacity(0);
        for data in self.data_segments.iter() {
            data_section.extend(&*data.bytes);
        }
        // rewrite data section
        self.data_segments.clear();
        self.push_rwasm_data_segment(&data_section);
        Ok(())
    }

    pub fn rewrite_tables(&mut self) -> Result<(), ModuleError> {
        // rewrite tables
        let num_tables = self.tables.len();
        self.tables.clear();
        (0..num_tables).for_each(|_| {
            self.tables
                .push(TableType::new(ValueType::FuncRef, 0, None))
        });
        // rewrite elements
        let mut element_section = Vec::with_capacity(0);
        for data in self.element_segments.iter() {
            element_section.extend(data.items.exprs.iter().map(|v| {
                if let Some(value) = v.eval_const() {
                    value.as_u32()
                } else {
                    v.funcref().unwrap().into_u32()
                }
            }));
        }
        self.element_segments.clear();
        self.push_rwasm_elem_segment(&element_section);
        Ok(())
    }

    pub fn push_rwasm_tables(&mut self) {
        let global_decl = TableType::new(ValueType::FuncRef, 0, Some(N_MAX_TABLE_ELEMENTS));
        (0..N_MAX_TABLES).for_each(|_| {
            self.tables.push(global_decl);
        });
    }

    /// Pushes the given exports to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If an export declaration fails to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_exports<T>(&mut self, exports: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<(Box<str>, ExternIdx), ModuleError>>,
    {
        assert!(
            self.exports.is_empty(),
            "tried to initialize module export declarations twice"
        );
        self.exports = exports.into_iter().collect::<Result<BTreeMap<_, _>, _>>()?;
        Ok(())
    }

    pub fn rewrite_exports<I>(&mut self, name: Box<str>, index: I) -> Result<(), ModuleError>
    where
        I: Into<ExternIdx>,
    {
        self.exports.clear();
        self.exports.insert(name, index.into());
        Ok(())
    }

    pub fn push_export<I>(&mut self, name: Box<str>, index: I)
    where
        I: Into<ExternIdx>,
    {
        self.exports.insert(name, index.into());
    }

    /// Sets the start function of the [`Module`] to the given index.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn set_start(&mut self, start: FuncIdx) {
        if let Some(old_start) = &self.start {
            panic!("encountered multiple start functions: {old_start:?}, {start:?}")
        }
        self.start = Some(start);
    }

    pub fn remove_start(&mut self) {
        self.start = None;
    }

    /// Pushes the given table elements to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If any of the table elements fail to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_element_segments<T>(&mut self, elements: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<ElementSegment, ModuleError>>,
    {
        assert!(
            self.element_segments.is_empty(),
            "tried to initialize module export declarations twice"
        );
        self.element_segments = elements.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    /// Pushes the given linear memory data segments to the [`Module`] under construction.
    ///
    /// # Errors
    ///
    /// If any of the linear memory data segments fail to validate.
    ///
    /// # Panics
    ///
    /// If this function has already been called on the same [`ModuleBuilder`].
    pub fn push_data_segments<T>(&mut self, data: T) -> Result<(), ModuleError>
    where
        T: IntoIterator<Item = Result<DataSegment, ModuleError>>,
    {
        assert!(
            self.data_segments.is_empty(),
            "tried to initialize module linear memory data segments twice"
        );
        self.data_segments = data.into_iter().collect::<Result<Vec<_>, _>>()?;
        Ok(())
    }

    pub fn push_rwasm_data_segment(&mut self, bytes: &[u8]) {
        self.data_segments.push(DataSegment {
            kind: DataSegmentKind::Passive,
            bytes: bytes.into(),
        });
        // fill max possible data segments (save some random byte, we use it as an indicator that
        // means data is not dropped)
        (0..N_MAX_DATA_SEGMENTS).for_each(|_| {
            self.data_segments.push(DataSegment {
                kind: DataSegmentKind::Passive,
                bytes: [0x1].into(),
            });
        });
    }

    pub fn push_rwasm_elem_segment(&mut self, bytes: &[u32]) {
        let items = ElementSegmentItems {
            exprs: bytes.iter().map(|v| ConstExpr::new_funcref(*v)).collect(),
        };
        self.element_segments.push(ElementSegment {
            kind: ElementSegmentKind::Passive,
            ty: ValueType::FuncRef,
            items,
        });
        // fill max possible data segments (save some random byte, we use it as an indicator that
        // means data is not dropped)
        (0..N_MAX_ELEM_SEGMENTS).for_each(|_| {
            self.element_segments.push(ElementSegment {
                kind: ElementSegmentKind::Passive,
                ty: ValueType::FuncRef,
                items: ElementSegmentItems { exprs: [].into() },
            });
        });
    }

    /// Finishes construction of the WebAssembly [`Module`].
    pub fn finish(self) -> Module {
        Module::from_builder(self)
    }
}
