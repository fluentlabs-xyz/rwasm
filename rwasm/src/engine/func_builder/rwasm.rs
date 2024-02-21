use crate::{
    engine::{
        bytecode::{DataSegmentIdx, ElementSegmentIdx, Instruction, TableIdx},
        func_builder::InstructionsBuilder,
    },
    module::DEFAULT_MEMORY_INDEX,
};
use hashbrown::HashMap;

/// This constant is driven by WebAssembly standard, default
/// memory page size is 64kB
pub const N_BYTES_PER_MEMORY_PAGE: u32 = 65536;

/// We have a hard limit for max possible memory used
/// that is equal to ~64mB
pub const N_MAX_MEMORY_PAGES: u32 = 1024;

/// To optimize proving process we have to limit max
/// number of pages, tables, etc. We found 1024 is enough.
pub const N_MAX_TABLES: u32 = 1024;

pub const N_MAX_STACK_HEIGHT: usize = 4096;
pub const N_MAX_RECURSION_DEPTH: usize = 1024;

#[derive(Debug, Default)]
pub struct RwasmModuleBuilder {
    pub(crate) memory_section: Vec<u8>,
    pub(crate) passive_memory_sections: HashMap<DataSegmentIdx, (u32, u32)>,
    pub(crate) element_section: Vec<u32>,
    pub(crate) passive_element_sections: HashMap<ElementSegmentIdx, (u32, u32)>,
    pub(crate) total_allocated_pages: u32,
    pub(crate) entrypoint_injected: bool,
}

impl RwasmModuleBuilder {
    pub fn reset(&mut self) {}

    pub fn add_memory_pages(
        &mut self,
        code_section: &mut InstructionsBuilder,
        initial_pages: u32,
    ) -> bool {
        // there is a hard limit of max possible memory used (~64 mB)
        if self.total_allocated_pages + initial_pages >= N_MAX_MEMORY_PAGES {
            return false;
        }
        // it makes no sense to grow memory with 0 pages
        if initial_pages > 0 {
            code_section.push_inst(Instruction::I32Const(initial_pages.into()));
            code_section.push_inst(Instruction::MemoryGrow);
            code_section.push_inst(Instruction::Drop);
        }
        // increase total number of pages allocated
        self.total_allocated_pages += initial_pages;
        return true;
    }

    pub fn add_default_memory(
        &mut self,
        code_section: &mut InstructionsBuilder,
        offset: u32,
        bytes: &[u8],
    ) -> bool {
        // don't allow to grow default memory if there is no enough pages allocated
        let max_affected_page =
            (offset + bytes.len() as u32 + N_BYTES_PER_MEMORY_PAGE - 1) / N_BYTES_PER_MEMORY_PAGE;
        if max_affected_page > self.total_allocated_pages {
            return false;
        }
        // expand default memory
        let data_offset = self.memory_section.len();
        let data_length = bytes.len();
        self.memory_section.extend(bytes);
        // default memory is just a passive section with force memory init
        code_section.push_inst(Instruction::I32Const(offset.into()));
        code_section.push_inst(Instruction::I64Const(data_offset.into()));
        code_section.push_inst(Instruction::I64Const(data_length.into()));
        code_section.push_inst(Instruction::MemoryInit(DEFAULT_MEMORY_INDEX.into()));
        // we have enough memory pages so can grow
        return true;
    }

    pub fn add_passive_memory(&mut self, segment_idx: DataSegmentIdx, bytes: &[u8]) {
        // expand default memory
        let data_offset = self.memory_section.len() as u32;
        let data_length = bytes.len() as u32;
        self.memory_section.extend(bytes);
        // store passive section info
        self.passive_memory_sections
            .insert(segment_idx, (data_offset, data_length));
    }

    pub fn add_active_elements<T: IntoIterator<Item = u32>>(
        &mut self,
        code_section: &mut InstructionsBuilder,
        offset: u32,
        table_idx: TableIdx,
        elements: T,
    ) {
        // expand element section (remember offset and length)
        let segment_offset = self.element_section.len();
        self.element_section.extend(elements);
        let segment_length = self.element_section.len() - segment_offset;
        // init table with these elements
        code_section.push_inst(Instruction::I32Const(offset.into()));
        code_section.push_inst(Instruction::I64Const(segment_offset.into()));
        code_section.push_inst(Instruction::I64Const(segment_length.into()));
        code_section.push_inst(Instruction::TableInit(0.into()));
        code_section.push_inst(Instruction::TableGet(table_idx.into()));
    }

    pub fn add_passive_elements<T: IntoIterator<Item = u32>>(
        &mut self,
        segment_idx: ElementSegmentIdx,
        elements: T,
    ) {
        // expand element section
        let segment_offset = self.element_section.len() as u32;
        self.element_section.extend(elements);
        let segment_length = self.element_section.len() as u32 - segment_offset;
        // store passive section info
        self.passive_element_sections
            .insert(segment_idx, (segment_offset, segment_length));
    }
}
