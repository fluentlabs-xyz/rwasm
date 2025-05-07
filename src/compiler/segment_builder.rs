use crate::{
    DataSegmentIdx,
    ElementSegmentIdx,
    InstructionSet,
    TableIdx,
    DEFAULT_MEMORY_INDEX,
    N_BYTES_PER_MEMORY_PAGE,
    N_MAX_MEMORY_PAGES,
};
use alloc::vec::Vec;
use hashbrown::HashMap;

#[derive(Debug, Default)]
pub struct SegmentBuilder {
    pub(crate) global_memory_section: Vec<u8>,
    pub(crate) memory_sections: HashMap<DataSegmentIdx, (u32, u32)>,
    pub(crate) global_element_section: Vec<u32>,
    pub(crate) element_sections: HashMap<ElementSegmentIdx, (u32, u32)>,
    pub(crate) total_allocated_pages: u32,
}

impl SegmentBuilder {
    pub fn add_memory_pages(
        &mut self,
        code_section: &mut InstructionSet,
        initial_pages: u32,
    ) -> bool {
        // there is a hard limit of max possible memory used (~64 mB)
        if self.total_allocated_pages + initial_pages >= N_MAX_MEMORY_PAGES as u32 {
            return false;
        }
        // it makes no sense to grow memory with 0 pages
        if initial_pages > 0 {
            // TODO(dmitry123): "add stack height check"
            code_section.op_i32_const(initial_pages);
            code_section.op_memory_grow();
            code_section.op_drop();
        }
        // increase the total number of pages allocated
        self.total_allocated_pages += initial_pages;
        true
    }

    pub fn add_active_memory(
        &mut self,
        code_section: &mut InstructionSet,
        segment_idx: DataSegmentIdx,
        offset: u32,
        bytes: &[u8],
    ) {
        // don't allow growing default memory if there is no enough pages allocated
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
        code_section.op_i32_const(offset);
        code_section.op_i64_const(data_offset);
        if has_memory_overflow().unwrap_or_default() {
            code_section.op_i64_const(u32::MAX);
        } else {
            code_section.op_i64_const(data_length);
        }
        // TODO(dmitry123): "add stack height check"
        code_section.op_memory_init(DEFAULT_MEMORY_INDEX);
        code_section.op_data_drop(segment_idx + 1);
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
        code_section: &mut InstructionSet,
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
        code_section.op_i32_const(offset);
        code_section.op_i64_const(segment_offset);
        code_section.op_i64_const(segment_length);
        code_section.op_table_init(segment_idx + 1);
        code_section.op_table_get(table_idx);
        code_section.op_elem_drop(segment_idx + 1);
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
