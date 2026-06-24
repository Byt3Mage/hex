use crate::{Value, instruction::Instruction};

pub fn disassemble(bytecode: &[Instruction], constants: &[Value]) {
    for (i, &inst) in bytecode.iter().enumerate() {
        print!("{:4}: ", i);
        print_inst(inst, constants);
        println!();
    }
}

fn print_inst(_inst: Instruction, _constants: &[Value]) {}
