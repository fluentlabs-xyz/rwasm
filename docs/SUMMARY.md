# rWasm Documentation

## Encoding Specification
- [Module Format](encoding/module.md)
- [Instruction Encoding](encoding/instruction.md)

## Instruction Reference

### System Instructions
- [Unreachable (0x00)](instructions/00_unreachable.md)
- [Trap (0x01)](instructions/01_trap.md)

### Local Variables
- [LocalGet (0x10)](instructions/01_local_get.md)
- [LocalSet (0x11)](instructions/02_local_set.md)
- [LocalTee (0x12)](instructions/03_local_tee.md)

### Control Flow
- [Br (0x20)](instructions/04_br.md)
- [BrIfEqz (0x21)](instructions/05_br_if.md)
- [BrIfNez (0x22)](instructions/06_br_if_nez.md)
- [BrTable (0x23)](instructions/23_br_table.md)

### Fuel Management
- [ConsumeFuel (0x30)](instructions/10_consume_fuel.md)
- [ConsumeFuelStack (0x31)](instructions/31_consume_fuel_stack.md)

### Function Calls
- [Return (0x40)](instructions/11_return.md)
- [ReturnCallInternal (0x41)](instructions/13_return_call_internal.md)
- [ReturnCall (0x42)](instructions/14_return_call.md)
- [ReturnCallIndirect (0x43)](instructions/15_return_call_indirect.md)
- [CallInternal (0x44)](instructions/16_call_internal.md)
- [Call (0x45)](instructions/17_call.md)
- [CallIndirect (0x46)](instructions/18_call_indirect.md)

### Stack Operations
- [SignatureCheck (0x50)](instructions/19_signature_check.md)
- [StackCheck (0x51)](instructions/51_stack_check.md)
- [RefFunc (0x60)](instructions/61_ref_func.md)
- [I32Const (0x61)](instructions/62_i32_const.md)
- [Drop (0x62)](instructions/20_drop.md)
- [Select (0x63)](instructions/63_select.md)

### Global Variables
- [GlobalGet (0x70)](instructions/70_global_get.md)
- [GlobalSet (0x71)](instructions/71_global_set.md)

### Memory Instructions
- [I32Load (0x80)](instructions/80_i32_load.md)
- [I32Load8S (0x81)](instructions/81_i32_load8_s.md)
- [I32Load8U (0x82)](instructions/82_i32_load8_u.md)
- [I32Load16S (0x83)](instructions/83_i32_load16_s.md)
- [I32Load16U (0x84)](instructions/84_i32_load16_u.md)
- [I32Store (0x85)](instructions/85_i32_store.md)
- [I32Store8 (0x86)](instructions/86_i32_store8.md)
- [I32Store16 (0x87)](instructions/87_i32_store16.md)
- [MemorySize (0x88)](instructions/47_memory_size.md)
- [MemoryGrow (0x89)](instructions/48_memory_grow.md)
- [MemoryFill (0x8a)](instructions/8a_memory_fill.md)
- [MemoryCopy (0x8b)](instructions/8b_memory_copy.md)
- [MemoryInit (0x8c)](instructions/8c_memory_init.md)
- [DataDrop (0x8d)](instructions/8d_data_drop.md)

### Table Instructions
- [TableSize (0x90)](instructions/53_table_size.md)
- [TableGrow (0x91)](instructions/54_table_grow.md)
- [TableFill (0x92)](instructions/55_table_fill.md)
- [TableGet (0x93)](instructions/56_table_get.md)
- [TableSet (0x94)](instructions/57_table_set.md)
- [TableCopy (0x95)](instructions/95_table_copy.md)
- [TableInit (0x96)](instructions/96_table_init.md)
- [ElemDrop (0x97)](instructions/97_elem_drop.md)

### I32 Instructions

#### Comparison Operations
- [I32Eqz (0xa0)](instructions/a0_i32_eqz.md)
- [I32Eq (0xa1)](instructions/a1_i32_eq.md)
- [I32Ne (0xa2)](instructions/a2_i32_ne.md)
- [I32LtS (0xa3)](instructions/a3_i32_lt_s.md)
- [I32LtU (0xa4)](instructions/a4_i32_lt_u.md)
- [I32GtS (0xa5)](instructions/a5_i32_gt_s.md)
- [I32GtU (0xa6)](instructions/a6_i32_gt_u.md)
- [I32LeS (0xa7)](instructions/a7_i32_le_s.md)
- [I32LeU (0xa8)](instructions/a8_i32_le_u.md)
- [I32GeS (0xa9)](instructions/a9_i32_ge_s.md)
- [I32GeU (0xaa)](instructions/aa_i32_ge_u.md)

#### Unary Operations
- [I32Clz (0xab)](instructions/ab_i32_clz.md)
- [I32Ctz (0xac)](instructions/ac_i32_ctz.md)
- [I32Popcnt (0xad)](instructions/ad_i32_popcnt.md)

#### Binary Operations
- [I32Add (0xae)](instructions/ae_i32_add.md)
- [I32Sub (0xaf)](instructions/af_i32_sub.md)
- [I32Mul (0xb0)](instructions/b0_i32_mul.md)
- [I32DivS (0xb1)](instructions/b1_i32_div_s.md)
- [I32DivU (0xb2)](instructions/b2_i32_div_u.md)
- [I32RemS (0xb3)](instructions/b3_i32_rem_s.md)
- [I32RemU (0xb4)](instructions/b4_i32_rem_u.md)

#### Bitwise Operations
- [I32And (0xb5)](instructions/b5_i32_and.md)
- [I32Or (0xb6)](instructions/b6_i32_or.md)
- [I32Xor (0xb7)](instructions/b7_i32_xor.md)
- [I32Shl (0xb8)](instructions/b8_i32_shl.md)
- [I32ShrS (0xb9)](instructions/b9_i32_shr_s.md)
- [I32ShrU (0xba)](instructions/ba_i32_shr_u.md)
- [I32Rotl (0xbb)](instructions/bb_i32_rotl.md)
- [I32Rotr (0xbc)](instructions/bc_i32_rotr.md)

#### Conversion Operations
- [I32WrapI64 (0xbd)](instructions/bd_i32_wrap_i64.md)
- [I32Extend8S (0xbe)](instructions/be_i32_extend8_s.md)
- [I32Extend16S (0xbf)](instructions/bf_i32_extend16_s.md)

#### 64-bit Optimized Operations
- [I32Mul64 (0xc0)](instructions/c0_i32_mul64.md)
- [I32Add64 (0xc1)](instructions/c1_i32_add64.md)
