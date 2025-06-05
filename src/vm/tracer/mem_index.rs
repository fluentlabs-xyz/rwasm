

use crate::{N_MAX_DATA_SEGMENTS_BITS, N_MAX_RECURSION_DEPTH, N_MAX_STACK_SIZE, N_MAX_TABLES, N_MAX_TABLE_SIZE};


///We map every type of data of rwasm engine including stack, tables and calls frames and memory  into one type of virtual indexing.
/// This indexing is only used to prove memory consistency and never actually implemented.
/// we provide helper functions to map recorded memory changes, table changes and calls into virtual memory changes.
/// The unit data type for virtual memory indexing is u8 i.e. a byte.
/// The basic data type of rwasm is u32 and it is represeted by 4 bytes in the virtual memory indexing.
/// (We always using the max capacity since the zkvm does not care about dynamic capacitiy).
/// Virtual indexing start with the stack, then function call frames, then tables, with memory comes last.
/// The stack has 4096 elements 
///
pub const UNIT:u32 = 4; // size_of<u32>() / size_of<u8>()
/// The stack starts with and invalid postion and every element in the stack has index less than SP_START.
pub const SP_START:u32 = N_MAX_STACK_SIZE as u32 * UNIT + UNIT;
/// this is the index when the stack reach the max length. So every valid index for stack is >0.
/// Making index of stack elemetn strictly larger than 0 makes circuit checking this bound simpler.
pub const SP_END:u32 = UNIT;

pub const FUNC_FRAME_SIZE: u32 = UNIT;// TODO (Dimitry) it looks like the call stack only save the returning pc right?
pub const FUNC_FRAME_START:u32 = SP_START +UNIT;
pub const FUNC_FRAME_END :u32 = FUNC_FRAME_START+FUNC_FRAME_SIZE* N_MAX_RECURSION_DEPTH as u32;
pub const TABLE_ELE_SIZE:u32 = UNIT;
pub const TALBE_SEG_START:u32 = FUNC_FRAME_END+UNIT;
pub const TALBE_SEG_END:u32 = TALBE_SEG_START + N_MAX_TABLES *N_MAX_TABLE_SIZE *TABLE_ELE_SIZE;
pub const DATA_SEG_ELE_SIZE:u32 = UNIT;
pub const DATA_SEG_START: u32 = TALBE_SEG_END+UNIT;
pub const DATA_SEG_END:u32 = DATA_SEG_START +N_MAX_DATA_SEGMENTS_BITS as u32*DATA_SEG_ELE_SIZE;
pub const GLOBAL_MEM_START:u32 = DATA_SEG_END+UNIT;
pub const GLOBAL_MEM_END:u32 = GLOBAL_MEM_START + (1<<8)<<20;




