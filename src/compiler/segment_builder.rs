use crate::{
    instruction_set, split_i64_to_i32, CompilationError, DataSegmentIdx, ElementSegmentIdx,
    GlobalIdx, GlobalVariable, InstructionSet, TableIdx, DEFAULT_MEMORY_INDEX, NULL_FUNC_IDX,
    N_BYTES_PER_MEMORY_PAGE, N_MAX_MEMORY_PAGES,
};
use alloc::{vec, vec::Vec};
use hashbrown::HashMap;
use wasmparser::{RefType, TableType, ValType};

#[derive(Debug)]
pub struct SegmentBuilder {
    pub(crate) global_memory_section: Vec<u8>,
    pub(crate) memory_sections: HashMap<DataSegmentIdx, (u32, u32)>,
    pub(crate) global_element_section: Vec<u32>,
    pub(crate) element_sections: HashMap<ElementSegmentIdx, (u32, u32)>,
    pub(crate) total_allocated_pages: u32,
    pub(crate) entrypoint_bytecode: InstructionSet,
}

impl Default for SegmentBuilder {
    fn default() -> Self {
        let entrypoint_bytecode = instruction_set! {
            // entrypoint consumes max 3 stack elements during execution, but we should use 5,
            // because e2e testing suite passes param and state (2)
            // TODO(dmitry123): "ideally we need to fix the way we calc this"
            // during the calculation of this stack height we assume that input params have max 1 element
            // on the stack that is true for e2e testing suite and for fluentbase use cases, but theoretically
            // the use case with variadic number of params can exist, then we need to take max number
            // of params per each state config or start function and calc potential max stack height
            StackCheck(5)
        };
        Self {
            global_memory_section: vec![],
            memory_sections: Default::default(),
            global_element_section: vec![],
            element_sections: Default::default(),
            total_allocated_pages: 0,
            entrypoint_bytecode,
        }
    }
}

impl SegmentBuilder {
    pub fn add_global_variable(
        &mut self,
        global_idx: GlobalIdx,
        global_variable: &GlobalVariable,
    ) -> Result<(), CompilationError> {
        let global_type = global_variable.global_type.content_type;
        match global_type {
            ValType::I32 | ValType::F32 => self
                .entrypoint_bytecode
                .op_i32_const(global_variable.default_value),
            ValType::I64 | ValType::F64 => {
                let (lower, upper) = split_i64_to_i32(global_variable.default_value);
                self.entrypoint_bytecode.op_i32_const(lower);
                self.entrypoint_bytecode.op_i32_const(upper)
            }
            ValType::Ref(RefType::FUNC) | ValType::Ref(RefType::EXTERN)  => self
                .entrypoint_bytecode
                .op_ref_func(global_variable.default_value as u32),
            _ => return Err(CompilationError::NotSupportedGlobalType),
        };
        self.entrypoint_bytecode.op_global_set(global_idx * 2);
        if global_type == ValType::I64 || global_type == ValType::F64 {
            self.entrypoint_bytecode.op_global_set(global_idx * 2 + 1);
        }
        Ok(())
    }

    /// Max stack height: 3
    pub fn add_memory_pages(&mut self, initial_pages: u32) -> Result<(), CompilationError> {
        // there is a hard limit of max possible memory used (~64 mB)
        let next_pages = self
            .total_allocated_pages
            .checked_add(initial_pages)
            .unwrap_or(u32::MAX);
        if next_pages >= N_MAX_MEMORY_PAGES {
            return Err(CompilationError::MaxReadonlyDataReached);
        }
        // it makes no sense to grow memory with 0 pages
        if initial_pages > 0 {
            // TODO(dmitry123): "add stack height check?"
            self.entrypoint_bytecode.op_i32_const(initial_pages);
            self.entrypoint_bytecode.op_memory_grow_checked(None, true);
            // there is no need to verify for a potential trap because it can't overflow,
            // we have this check upper during the compilation time
            self.entrypoint_bytecode.op_drop();
        }
        // increase the total number of pages allocated
        self.total_allocated_pages = next_pages;
        Ok(())
    }

    pub fn emit_table_segment(
        &mut self,
        table_index: TableIdx,
        table_type: &TableType,
    ) -> Result<(), CompilationError> {
        // Wasm validation guarantees that number of table segments can't exceed 100 items,
        // that is why there is no need to check for potential overflow
        self.entrypoint_bytecode.op_ref_func(NULL_FUNC_IDX);
        self.entrypoint_bytecode.op_i32_const(table_type.initial);
        self.entrypoint_bytecode.op_table_grow(table_index);
        self.entrypoint_bytecode.op_drop();
        Ok(())
    }

    pub fn add_active_memory(&mut self, segment_idx: DataSegmentIdx, offset: u32, bytes: &[u8]) {
        // don't allow growing default memory if there are no enough pages allocated
        let has_memory_overflow = || -> Option<bool> {
            let max_affected_page = offset
                .checked_add(bytes.len() as u32)?
                .checked_add(N_BYTES_PER_MEMORY_PAGE - 1)?
                .checked_div(N_BYTES_PER_MEMORY_PAGE)?;
            Some(max_affected_page > self.total_allocated_pages)
        };
        // expand default memory
        let data_offset = self.global_memory_section.len();
        let data_length = bytes.len();
        self.global_memory_section.extend(bytes);
        // default memory is just a passive section with force memory init
        self.entrypoint_bytecode.op_i32_const(offset);
        self.entrypoint_bytecode.op_i32_const(data_offset);
        if has_memory_overflow().unwrap_or_default() {
            self.entrypoint_bytecode.op_i32_const(u32::MAX);
        } else {
            self.entrypoint_bytecode.op_i32_const(data_length);
        }
        // TODO(dmitry123): "add stack height check"
        self.entrypoint_bytecode
            .op_memory_init(DEFAULT_MEMORY_INDEX);
        self.entrypoint_bytecode.op_data_drop(segment_idx + 1);
        // store passive section info
        self.memory_sections
            .insert(segment_idx, (offset, bytes.len() as u32));
    }

    pub fn add_passive_memory(&mut self, segment_idx: DataSegmentIdx, bytes: &[u8]) {
        // expand default memory
        let data_offset = self.global_memory_section.len() as u32;
        let data_length = bytes.len() as u32;
        self.global_memory_section.extend(bytes);
        // store passive section info
        self.memory_sections
            .insert(segment_idx, (data_offset, data_length));
    }

    pub fn add_active_elements<T: IntoIterator<Item = u32>>(
        &mut self,
        segment_idx: ElementSegmentIdx,
        offset: u32,
        table_idx: TableIdx,
        elements: T,
    ) {
        // expand an element section (remember offset and length)
        let segment_offset = self.global_element_section.len();
        self.global_element_section.extend(elements);
        let segment_length = self.global_element_section.len() - segment_offset;
        // init table with these elements
        // TODO(dmitry123): "add stack height check"
        self.entrypoint_bytecode.op_i32_const(offset);
        self.entrypoint_bytecode.op_i32_const(segment_offset);
        self.entrypoint_bytecode.op_i32_const(segment_length);
        self.entrypoint_bytecode.op_table_init(segment_idx + 1);
        self.entrypoint_bytecode.op_table_get(table_idx);
        self.entrypoint_bytecode.op_elem_drop(segment_idx + 1);
        // store active section info
        self.element_sections
            .insert(segment_idx, (offset, segment_length as u32));
    }

    pub fn add_passive_elements<T: IntoIterator<Item = u32>>(
        &mut self,
        segment_idx: ElementSegmentIdx,
        elements: T,
    ) {
        // expand element section
        let segment_offset = self.global_element_section.len() as u32;
        self.global_element_section.extend(elements);
        let segment_length = self.global_element_section.len() as u32 - segment_offset;
        // store passive section info
        self.element_sections
            .insert(segment_idx, (segment_offset, segment_length));
    }
}
