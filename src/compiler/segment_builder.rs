use crate::{
    split_i64_to_i32,
    CompilationError,
    CompiledFunc,
    DataSegmentIdx,
    ElementSegmentIdx,
    GlobalIdx,
    GlobalVariable,
    InstructionSet,
    TableIdx,
    UntypedValue,
    DEFAULT_MEMORY_INDEX,
    N_BYTES_PER_MEMORY_PAGE,
    N_MAX_MEMORY_PAGES,
};
use alloc::vec::Vec;
use hashbrown::HashMap;
use wasmparser::ValType;

#[derive(Debug, Default)]
pub struct SegmentBuilder {
    pub(crate) global_memory_section: Vec<u8>,
    pub(crate) memory_sections: HashMap<DataSegmentIdx, (u32, u32)>,
    pub(crate) global_element_section: Vec<u32>,
    pub(crate) element_sections: HashMap<ElementSegmentIdx, (u32, u32)>,
    pub(crate) total_allocated_pages: u32,
    pub(crate) entrypoint_bytecode: InstructionSet,
}

impl SegmentBuilder {
    pub fn add_global_variable(
        &mut self,
        global_idx: GlobalIdx,
        global_variable: &GlobalVariable,
    ) -> Result<(), CompilationError> {
        let global_type = global_variable.global_type.content_type;
        if let Some(value) = global_variable.init_expr.eval_const() {
            match global_type {
                ValType::I32 => self.entrypoint_bytecode.op_i32_const(value),
                ValType::I64 => {
                    let (lower, upper) = split_i64_to_i32(value.as_i64());
                    self.entrypoint_bytecode.op_i32_const(lower);
                    self.entrypoint_bytecode.op_i32_const(upper)
                }
                ValType::F32 => self.entrypoint_bytecode.op_f32_const(value),
                ValType::F64 => self.entrypoint_bytecode.op_i64_const(value),
                // ValType::FuncRef => {}
                // ValType::ExternRef => {}
                _ => return Err(CompilationError::NotSupportedGlobalType),
            };
        } else if let Some(value) = global_variable.init_expr.funcref() {
            self.entrypoint_bytecode.op_ref_func(value);
        } else if let Some(index) = global_variable.init_expr.global() {
            if global_type == ValType::I64 {
                self.entrypoint_bytecode
                    .op_global_get(index.to_u32() * 2 + 1);
            }
            self.entrypoint_bytecode.op_global_get(index.to_u32() * 2);
        } else {
            return Err(CompilationError::NotSupportedGlobalType);
        }
        self.entrypoint_bytecode
            .op_global_set(global_idx.to_u32() * 2);
        if global_type == ValType::I64 {
            self.entrypoint_bytecode
                .op_global_set(global_idx.to_u32() * 2 + 1);
        }
        Ok(())
    }

    pub fn add_memory_pages(&mut self, initial_pages: u32) -> Result<(), CompilationError> {
        // there is a hard limit of max possible memory used (~64 mB)
        let next_pages = self
            .total_allocated_pages
            .checked_add(initial_pages)
            .unwrap_or(u32::MAX);
        if next_pages >= N_MAX_MEMORY_PAGES {
            return Err(CompilationError::MemorySegmentsOverflow);
        }
        // it makes no sense to grow memory with 0 pages
        if initial_pages > 0 {
            // TODO(dmitry123): "add stack height check?"
            self.entrypoint_bytecode.op_i32_const(initial_pages);
            self.entrypoint_bytecode.op_memory_grow();
            self.entrypoint_bytecode.op_drop();
        }
        // increase the total number of pages allocated
        self.total_allocated_pages = next_pages;
        Ok(())
    }

    pub fn add_active_memory(
        &mut self,
        segment_idx: DataSegmentIdx,
        offset: UntypedValue,
        bytes: &[u8],
    ) {
        // don't allow growing default memory if there are no enough pages allocated
        let has_memory_overflow = || -> Option<bool> {
            let max_affected_page = offset
                .as_u32()
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
        self.entrypoint_bytecode
            .op_data_drop(segment_idx.to_u32() + 1);
        // store passive section info
        self.memory_sections
            .insert(segment_idx, (offset.as_u32(), bytes.len() as u32));
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
        offset: UntypedValue,
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
        self.entrypoint_bytecode
            .op_table_init(segment_idx.to_u32() + 1);
        self.entrypoint_bytecode.op_table_get(table_idx);
        self.entrypoint_bytecode
            .op_elem_drop(segment_idx.to_u32() + 1);
        // store active section info
        self.element_sections
            .insert(segment_idx, (offset.as_u32(), segment_length as u32));
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

    pub fn add_start_function(&mut self, func_idx: CompiledFunc) {
        // for the start section we must always invoke even if there is a main function,
        // otherwise it might be super misleading for devs
        self.entrypoint_bytecode.op_call_internal(func_idx);
    }
}
