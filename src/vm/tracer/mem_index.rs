use crate::{
    N_MAX_DATA_SEGMENTS_BITS, N_MAX_ELEM_SEGMENTS_BITS, N_MAX_RECURSION_DEPTH, N_MAX_STACK_SIZE,
    N_MAX_TABLES, N_MAX_TABLE_SIZE,
};

/// We map every type of data of rwasm engine including stack, tables and call frames and memory
/// into one type of virtual indexing. This indexing is only used to prove memory consistency and
/// never actually implemented. We provide helper functions to map recorded memory changes, table
/// changes and calls into virtual memory changes. The unit data type for virtual memory indexing is
/// u8 i.e., a byte. The basic data type of rwasm is u32, and it is represented by 4 bytes in the
/// virtual memory indexing. (We're always using the max capacity since the zkVM does not care about
/// dynamic capacity). Virtual indexing starts with the stack, then function call frames, then
/// tables, with memory comes last. The stack has 4096 elements
pub const UNIT: u32 = 4; // size_of<u32>() / size_of<u8>()

/// The stack starts with and invalid position, and every element in the stack has an index less
/// than SP_START.
pub const SP_START: u32 = N_MAX_STACK_SIZE as u32 * UNIT + SP_END;

/// This is the index when the stack reaches the max length. So every valid index for the stack is
/// >0. Making the index of a stack element strictly larger than 0 makes circuit checking this bound
/// simpler.
///
/// We add 32 to prevent writes to the SP1 registers
pub const SP_END: u32 = 32 + UNIT;
/// a special memory addresss reserved for saving the signature id of last call indirect op
pub const RESERVED_ADDR_START: u32 = SP_END + UNIT;
pub const RESERVED_ADDR_END: u32 = RESERVED_ADDR_START + 1024 * UNIT;
pub const LAST_SIG_ADDR: u32 = SP_START + UNIT;
pub const FUNC_FRAME_SIZE: u32 = UNIT; // TODO (dmitry123): "it looks like the call stack only save the returning pc right?, Yes(Yao)"
pub const FUNC_FRAME_START: u32 = LAST_SIG_ADDR + UNIT;
pub const FUNC_FRAME_END: u32 = FUNC_FRAME_START + FUNC_FRAME_SIZE * N_MAX_RECURSION_DEPTH as u32;
pub const TABLE_ELEM_SIZE: u32 = UNIT;
pub const TABLE_SEG_START: u32 = FUNC_FRAME_END + UNIT;
pub const TABLE_SEG_END: u32 = TABLE_SEG_START + N_MAX_TABLES * N_MAX_TABLE_SIZE * TABLE_ELEM_SIZE;
pub const TABLE_SIZES_START: u32 = TABLE_SEG_END + UNIT;
pub const TABLE_SIZES_END: u32 = TABLE_SIZES_START + N_MAX_TABLES * UNIT;
pub const DATA_SEG_ELEM_SIZE: u32 = UNIT;
pub const DATA_SEG_START: u32 = TABLE_SIZES_END + UNIT;
pub const DATA_SEG_END: u32 = DATA_SEG_START + N_MAX_DATA_SEGMENTS_BITS as u32 * DATA_SEG_ELEM_SIZE;
pub const ELEMENT_SEG_SIZE: u32 = UNIT;
pub const ELEMENT_SEG_START: u32 = DATA_SEG_END + UNIT;
pub const ELEMENT_SEG_END: u32 =
    ELEMENT_SEG_START + N_MAX_ELEM_SEGMENTS_BITS as u32 * ELEMENT_SEG_SIZE;
pub const GLOBAL_MEM_START: u32 = ELEMENT_SEG_END + UNIT;
pub const GLOBAL_MEM_END: u32 = GLOBAL_MEM_START + (1 << 8) << 20;

#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub enum ReservedAddrEnum {
    LastSig = 0,
    FuelLimitHi = 1,
    FuelLimitLow = 2,
    ConsumedFuelLow = 3,
    ConsumedFuelHi = 4,
}

#[cfg_attr(feature = "tracing", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy)]
pub enum TypedAddress {
    Stack(u32),
    ReservedAddrEnum(ReservedAddrEnum),
    FuncFrame(u32),
    Table(u32),
    TableSize(u32),
    Data(u32),
    Element(u32),
    GlobalMemory(u32),
}

impl TypedAddress {
    pub fn from_stack_vaddr(sp: u32) -> TypedAddress {
        TypedAddress::Stack((SP_START - sp - UNIT) / 4)
    }

    pub fn to_virtual_addr(&self) -> u32 {
        match self {
            TypedAddress::Stack(offset) => {
                let v_addr = (SP_START - offset * UNIT) - UNIT;
                debug_assert!(v_addr >= SP_END);
                debug_assert!(v_addr < SP_START);
                v_addr
            }
            TypedAddress::ReservedAddrEnum(reserved_adr) => {
                let v_addr = RESERVED_ADDR_START + (*reserved_adr as u32) * UNIT;
                debug_assert!(v_addr >= RESERVED_ADDR_START);
                debug_assert!(v_addr < RESERVED_ADDR_END);
                v_addr
            }
            TypedAddress::FuncFrame(offset) => {
                let v_addr = FUNC_FRAME_START + *offset * UNIT;
                debug_assert!(v_addr >= FUNC_FRAME_START);
                debug_assert!(v_addr < FUNC_FRAME_END);
                v_addr
            }
            TypedAddress::Table(offset) => {
                let v_addr = TABLE_SEG_START + offset * UNIT;
                debug_assert!(v_addr >= TABLE_SEG_START);
                debug_assert!(v_addr < TABLE_SEG_END);
                v_addr
            }
            TypedAddress::TableSize(offset) => {
                let v_addr = TABLE_SIZES_START + offset * UNIT;
                debug_assert!(v_addr >= TABLE_SIZES_START);
                debug_assert!(v_addr < TABLE_SIZES_END);
                v_addr
            }
            TypedAddress::Data(offset) => {
                let v_addr = DATA_SEG_START + offset * UNIT;
                debug_assert!(v_addr >= DATA_SEG_START);
                debug_assert!(v_addr < DATA_SEG_END);
                v_addr
            }
            TypedAddress::Element(offset) => {
                let v_addr = ELEMENT_SEG_START + offset * UNIT;
                debug_assert!(v_addr >= ELEMENT_SEG_START);
                debug_assert!(v_addr < ELEMENT_SEG_END);
                v_addr
            }
            TypedAddress::GlobalMemory(offset) => {
                let v_addr = GLOBAL_MEM_START + offset;
                debug_assert!(v_addr >= GLOBAL_MEM_START);
                debug_assert!(v_addr < GLOBAL_MEM_END);
                v_addr
            }
        }
    }

    pub fn v_addr_to_pre_addr(v_addr: u32) -> Option<TypedAddress> {
        if v_addr >= ELEMENT_SEG_START && v_addr < ELEMENT_SEG_END {
            Some(TypedAddress::Element((v_addr - ELEMENT_SEG_START) / UNIT))
        } else if v_addr >= DATA_SEG_START && v_addr < DATA_SEG_END {
            Some(TypedAddress::Element((v_addr - DATA_SEG_START) / UNIT))
        } else {
            None
        }
    }

    pub fn from_reserved_addr(addr: ReservedAddrEnum) -> TypedAddress {
        TypedAddress::ReservedAddrEnum(addr)
    }
}
