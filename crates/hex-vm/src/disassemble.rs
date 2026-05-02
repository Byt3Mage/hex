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
        Opcode::MOV => print!("mov    r{}, r{}", inst.a(), inst.b()),
        Opcode::CONST => {
            let idx = inst.bx() as usize;
            let val = if idx < constants.len() {
                format!("{}", u64::from_value(constants[idx]))
            } else {
                format!("c{}", idx)
            };
            print!("const  r{}, {}", inst.a(), val)
        }

        Opcode::IADD => print!("iadd   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::ISUB => print!("isub   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IMUL => print!("imul   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IDIV => print!("idiv   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IREM => print!("irem   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::UADD => print!("uadd   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::USUB => print!("usub   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UMUL => print!("umul   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UDIV => print!("udiv   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UREM => print!("urem   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::FADD => print!("fadd   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FSUB => print!("fsub   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FMUL => print!("fmul   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FDIV => print!("fdiv   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::FREM => print!("frem   r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::IEQ => print!("ieq    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::INE => print!("ine    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::ILT => print!("ilt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IGT => print!("igt    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::ILE => print!("ile    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::IGE => print!("ige    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),

        Opcode::UEQ => print!("ueq    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
        Opcode::UNE => print!("une    r{}, r{}, r{}", inst.a(), inst.b(), inst.c()),
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

        Opcode::BNOT => print!("bnot   r{}, r{}", inst.a(), inst.b()),
        Opcode::INOT => print!("inot   r{}, r{}", inst.a(), inst.b()),
        Opcode::UNOT => print!("unot   r{}, r{}", inst.a(), inst.b()),
        Opcode::INEG => print!("ineg   r{}, r{}", inst.a(), inst.b()),
        Opcode::FNEG => print!("fneg   r{}, r{}", inst.a(), inst.b()),

        Opcode::JMP => print!("jmp    @{}", inst.ax()),
        Opcode::JMP_T => print!("jmp_t  r{}, @{}", inst.a(), inst.bx()),
        Opcode::JMP_F => print!("jmp_f  r{}, @{}", inst.a(), inst.bx()),

        Opcode::CALL => print!("call   r{}, fn{}", inst.a(), inst.bx()),
        Opcode::CALLR => print!("callr  r{}, r{}", inst.a(), inst.b()),
        Opcode::CALLN => print!("calln  r{}, native{}", inst.a(), inst.bx()),
        Opcode::CALLNR => print!("callnr r{}, r{}", inst.a(), inst.b()),
        Opcode::CALLT => print!("callt  r{}, fn{}", inst.a(), inst.bx()),

        Opcode::RET => print!("ret"),
        Opcode::HALT => print!("halt"),

        op => print!("??? op={}", op),
    }
}
