use crate::{
    IsValue, Value,
    instruction::{Instruction, Opcode},
};

pub fn disassemble(bytecode: &[Instruction], constants: &[Value]) {
    for (i, &inst) in bytecode.iter().enumerate() {
        print!("{:4}: ", i);
        print_inst(inst, constants);
        println!();
    }
}

fn print_inst(inst: Instruction, constants: &[Value]) {
    match inst.op() {
        Opcode::COPY => print!("copy   r{}, r{}", inst.a(), inst.b()),
        Opcode::CONST => {
            let idx = inst.bx() as usize;
            let val = if idx < constants.len() {
                format!("{}", u64::from_value(constants[idx]))
            } else {
                format!("c{}", idx)
            };
            print!("const  r{}, {}", inst.a(), val)
        }

        Opcode::ADD => print!("add   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::SUB => print!("sub   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::MUL => print!("mul   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::SDIV => print!("idiv   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::SREM => print!("irem   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UDIV => print!("udiv   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UREM => print!("urem   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::FADD => print!("fadd   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FSUB => print!("fsub   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FMUL => print!("fmul   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FDIV => print!("fdiv   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FREM => print!("frem   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::EQ => print!("eq    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::NE => print!("ne    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::ILT => print!("ilt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IGT => print!("igt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::ILE => print!("ile    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IGE => print!("ige    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::ULT => print!("ult    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UGT => print!("ugt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::ULE => print!("ule    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UGE => print!("uge    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::FEQ => print!("feq    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FNE => print!("fne    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FLT => print!("flt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FGT => print!("fgt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FLE => print!("fle    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FGE => print!("fge    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::NOT => print!("not   r{}, r{}", inst.a(), inst.b()),
        Opcode::BNOT => print!("bnot   r{}, r{}", inst.a(), inst.b()),
        Opcode::INEG => print!("ineg   r{}, r{}", inst.a(), inst.b()),
        Opcode::FNEG => print!("fneg   r{}, r{}", inst.a(), inst.b()),

        Opcode::JMP => print!("jmp    @{}", inst.ax()),
        Opcode::JMP_T => print!("jmpt  r{}, @{}", inst.a(), inst.bx()),
        Opcode::JMP_F => print!("jmpf  r{}, @{}", inst.a(), inst.bx()),

        Opcode::CALL => print!("call   r{}, fn{}", inst.a(), inst.bx()),
        Opcode::CALLR => print!("callr  r{}, r{}", inst.a(), inst.b()),
        Opcode::RET => print!("ret"),

        Opcode::HALT => print!("halt"),

        op => print!("??? op={}", op),
    }
}
