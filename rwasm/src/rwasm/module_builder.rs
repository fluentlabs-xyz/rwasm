use crate::{
    core::{N_BYTES_PER_MEMORY_PAGE, N_MAX_MEMORY_PAGES},
    engine::{
        bytecode::{DataSegmentIdx, ElementSegmentIdx, Instruction, TableIdx},
        func_builder::InstructionsBuilder,
    },
    module::DEFAULT_MEMORY_INDEX,
};
use hashbrown::HashMap;

#[derive(Debug, Default)]
pub struct RwasmModuleBuilder {
    pub(crate) global_memory_section: Vec<u8>,
    pub(crate) memory_sections: HashMap<DataSegmentIdx, (u32, u32)>,
    pub(crate) global_element_section: Vec<u32>,
    pub(crate) element_sections: HashMap<ElementSegmentIdx, (u32, u32)>,
    pub(crate) total_allocated_pages: u32,
}

impl RwasmModuleBuilder {
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

    pub fn add_active_memory(
        &mut self,
        code_section: &mut InstructionsBuilder,
        segment_idx: DataSegmentIdx,
        offset: u32,
        bytes: &[u8],
    ) {
        // don't allow to grow default memory if there is no enough pages allocated
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
        code_section.push_inst(Instruction::I32Const(offset.into()));
        code_section.push_inst(Instruction::I64Const(data_offset.into()));
        if has_memory_overflow().unwrap_or_default() {
            code_section.push_inst(Instruction::I64Const(u32::MAX.into()));
        } else {
            code_section.push_inst(Instruction::I64Const(data_length.into()));
        }
        code_section.push_inst(Instruction::MemoryInit(DEFAULT_MEMORY_INDEX.into()));
        code_section.push_inst(Instruction::DataDrop((segment_idx.to_u32() + 1).into()));
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
        code_section: &mut InstructionsBuilder,
        segment_idx: ElementSegmentIdx,
        offset: u32,
        table_idx: TableIdx,
        elements: T,
    ) {
        // expand element section (remember offset and length)
        let segment_offset = self.global_element_section.len();
        self.global_element_section.extend(elements);
        let segment_length = self.global_element_section.len() - segment_offset;
        // init table with these elements
        code_section.push_inst(Instruction::I32Const(offset.into()));
        code_section.push_inst(Instruction::I64Const(segment_offset.into()));
        code_section.push_inst(Instruction::I64Const(segment_length.into()));
        code_section.push_inst(Instruction::TableInit((segment_idx.to_u32() + 1).into()));
        code_section.push_inst(Instruction::TableGet(table_idx.into()));
        code_section.push_inst(Instruction::ElemDrop((segment_idx.to_u32() + 1).into()));
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
