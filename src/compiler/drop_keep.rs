use crate::{compiler::value_stack::ValueStackHeight, DropKeep, InstructionSet};

pub fn translate_drop_keep(
    instr_builder: &mut InstructionSet,
    drop_keep: DropKeep,
    height: &mut ValueStackHeight,
) -> usize {
    let (drop, keep) = (drop_keep.drop(), drop_keep.keep());
    if drop == 0 {
        return 0;
    }
    let mut opcode_count = 0;
    if drop >= keep {
        (0..keep).for_each(|_| {
            instr_builder.op_local_set(drop as u32);
            opcode_count += 1;
        });
        (0..(drop - keep)).for_each(|_| {
            instr_builder.op_drop();
            opcode_count += 1;
        });
    } else {
        height.push();
        height.pop1();
        (0..keep).for_each(|i| {
            instr_builder.op_local_get(keep as u32 - i as u32);
            instr_builder.op_local_set(keep as u32 + drop as u32 - i as u32);
            opcode_count += 2;
        });
        (0..drop).for_each(|_| {
            instr_builder.op_drop();
            opcode_count += 1;
        });
    }
    opcode_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Opcode, OpcodeData};

    #[test]
    fn test_drop_keep_translation() {
        macro_rules! drop_keep {
            ($drop:literal, $keep:literal) => {
                DropKeep::new($drop, $keep).unwrap()
            };
        }
        let tests = vec![
            (vec![100, 20, 120], vec![120], drop_keep!(2, 1)),
            (vec![1, 2], vec![1, 2], drop_keep!(0, 0)),
            (vec![1, 2, 3], vec![1, 2, 3], drop_keep!(0, 3)),
            (vec![1, 2, 3, 4], vec![3, 4], drop_keep!(2, 2)),
            (vec![2, 3, 7], vec![3, 7], drop_keep!(1, 2)),
            (vec![1, 2, 3, 4, 5, 6], vec![3, 4, 5, 6], drop_keep!(2, 4)),
            (vec![7, 100, 20, 3], vec![7], drop_keep!(3, 0)),
            (vec![100, 20, 120], vec![120], drop_keep!(2, 1)),
            (vec![1, 2, 3, 4, 5], vec![5], drop_keep!(4, 1)),
        ];
        for (input, output, drop_keep) in tests.iter() {
            let mut instr_builder = InstructionSet::new();
            let mut stack_height = ValueStackHeight::default();
            translate_drop_keep(&mut instr_builder, *drop_keep, &mut stack_height);
            let mut stack = input.clone();
            for instr in instr_builder.instr {
                match instr {
                    (Opcode::LocalSet, OpcodeData::LocalDepth(index)) => {
                        let last = stack.last().unwrap();
                        let len = stack.len();
                        *stack.get_mut(len - 1 - index.to_usize()).unwrap() = *last;
                        stack.pop();
                    }
                    (Opcode::LocalGet, OpcodeData::LocalDepth(index)) => {
                        let len = stack.len();
                        let item = *stack.get(len - index.to_usize()).unwrap();
                        stack.push(item);
                    }
                    (Opcode::Drop, _) => {
                        stack.pop();
                    }
                    _ => unreachable!("unknown opcode: {:?}", instr),
                }
            }
            assert_eq!(stack, *output);
        }
    }
}
