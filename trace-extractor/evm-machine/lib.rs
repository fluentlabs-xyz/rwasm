#![no_main]

use hex_literal::hex;
use revm_bytecode::Bytecode;
use revm_interpreter::{
    CallInput, InputsImpl, Interpreter, SharedMemory,
    host::DummyHost,
    instruction_table,
    interpreter::{EthInterpreter, ExtBytecode},
};

#[unsafe(no_mangle)]
pub fn main() {
    let evm_bytecode = hex!(
        "608060405234801561000f575f5ffd5b5060043610610029575f3560e01c8063f9b7c7e51461002d575b5f5ffd5b610047600480360381019061004291906100f1565b61005d565b604051610054919061012b565b60405180910390f35b5f5f5f90505f600190505f600290505b8463ffffffff168163ffffffff16116100a9575f828461008d9190610171565b90508293508092505080806100a1906101a8565b91505061006d565b508092505050919050565b5f5ffd5b5f63ffffffff82169050919050565b6100d0816100b8565b81146100da575f5ffd5b50565b5f813590506100eb816100c7565b92915050565b5f60208284031215610106576101056100b4565b5b5f610113848285016100dd565b91505092915050565b610125816100b8565b82525050565b5f60208201905061013e5f83018461011c565b92915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f61017b826100b8565b9150610186836100b8565b9250828201905063ffffffff8111156101a2576101a1610144565b5b92915050565b5f6101b2826100b8565b915063ffffffff82036101c8576101c7610144565b5b60018201905091905056fea26469706673582212206f34ca4baf4d7f4a2ab9c7060b71c1f28bca433c9959aabaa5c1ac6323863d2364736f6c634300081e0033"
    );
    let bytecode = Bytecode::new_raw(evm_bytecode.into());
    let instruction_table = instruction_table::<EthInterpreter, DummyHost>();
    let mut interpreter = Interpreter::new(
        SharedMemory::new(),
        ExtBytecode::new_with_hash(bytecode.clone(), [1u8; 32].into()),
        InputsImpl {
            target_address: Default::default(),
            bytecode_address: None,
            caller_address: Default::default(),
            input: CallInput::Bytes(
                hex!("f9b7c7e5000000000000000000000000000000000000000000000000000000000000002b")
                    .into(),
            ),
            call_value: Default::default(),
        },
        true,
        Default::default(),
        100_000_000,
    );
    let result = interpreter.run_plain::<DummyHost>(&instruction_table, &mut DummyHost {});
    core::hint::black_box(result);
}
