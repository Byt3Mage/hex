//! Disassembler for a compiled `Program`.

use alloc::{
    collections::BTreeMap,
    format,
    string::{String, ToString},
};
use core::fmt::Write;

use crate::{
    FnType, Function, Program,
    instruction::{Instruction, Opcode, inst},
    word,
};

/// Operand shape per opcode — tells the disassembler how to decode + render.
#[derive(Clone, Copy)]
enum Shape {
    /// no operands (RET, HALT)
    None,
    /// a, b           (COPY, NOT, INEG, ...)
    AB,
    /// a, b, c        (ADD, LOAD, comparisons, ...)
    ABC,
    /// a, b, imm8 in c (ADDI/SUBI/MULI)
    ABI,
    /// a, #const-idx in c (ADDK/.../FDIVK)
    ABKc,
    /// a, bx as signed immediate (LOADI/LOADF)
    AImmBx,
    /// a, #const-idx in bx (LOADK)
    AKbx,
    /// a, bx as code target (JMP_T/JMP_F)
    ACondTarget,
    /// ax as code target (JMP)
    AxTarget,
    /// a=ret, bx=function id (CALL)
    ACall,
    /// a=ret, b=reg holding fn id (CALL_INDIRECT)
    ACallR,
    /// cmp-branch: regs in b,c; the FOLLOWING word is the jump target (JEQ.. JFGE)
    CmpBranch,
}

fn shape_of(op: Opcode) -> Shape {
    use Shape::*;
    match op {
        Opcode::COPY | Opcode::NOT | Opcode::BNOT | Opcode::INEG | Opcode::FNEG => AB,

        Opcode::LOADI | Opcode::LOADF => AImmBx,
        Opcode::LOADK => AKbx,

        Opcode::ADDI | Opcode::SUBI | Opcode::MULI => ABI,
        Opcode::ADDK | Opcode::SUBK | Opcode::MULK | Opcode::FADDK | Opcode::FSUBK | Opcode::FMULK | Opcode::FDIVK => {
            ABKc
        }

        Opcode::ADD
        | Opcode::SUB
        | Opcode::MUL
        | Opcode::SDIV
        | Opcode::SREM
        | Opcode::UDIV
        | Opcode::UREM
        | Opcode::FADD
        | Opcode::FSUB
        | Opcode::FMUL
        | Opcode::FDIV
        | Opcode::FREM
        | Opcode::EQ
        | Opcode::NE
        | Opcode::SLT
        | Opcode::SGT
        | Opcode::SLE
        | Opcode::SGE
        | Opcode::ULT
        | Opcode::UGT
        | Opcode::ULE
        | Opcode::UGE
        | Opcode::FEQ
        | Opcode::FNE
        | Opcode::FLT
        | Opcode::FGT
        | Opcode::FLE
        | Opcode::FGE
        | Opcode::LOAD
        | Opcode::STORE
        | Opcode::STORE_ADDRESS => ABC,

        Opcode::JMP => AxTarget,
        Opcode::JMP_T | Opcode::JMP_F => ACondTarget,

        Opcode::JEQ
        | Opcode::JNE
        | Opcode::JSLT
        | Opcode::JSGT
        | Opcode::JSLE
        | Opcode::JSGE
        | Opcode::JULT
        | Opcode::JUGT
        | Opcode::JULE
        | Opcode::JUGE
        | Opcode::JFEQ
        | Opcode::JFNE
        | Opcode::JFLT
        | Opcode::JFGT
        | Opcode::JFLE
        | Opcode::JFGE => CmpBranch,

        Opcode::CALL => ACall,
        Opcode::CALL_IND => ACallR,

        Opcode::RET | Opcode::HALT => None,

        _ => Shape::None, // unknown -> render raw
    }
}

/// Disassemble a whole program into human-readable text, with reconstructed labels.
pub fn disassemble(program: &Program) -> String {
    let code = program.instructions();
    let labels = collect_labels(code); // pc -> "L{n}"
    let mut out = String::new();

    // constants
    let consts = program.constants();
    let _ = writeln!(out, "; constants ({})", consts.len());
    for (i, c) in consts.iter().enumerate() {
        let _ = writeln!(out, ";   [{i}] {}", fmt_word(*c));
    }

    // functions
    let fns = program.functions();
    let _ = writeln!(out, "; functions ({})", fns.len());
    for (id, f) in fns.iter().enumerate() {
        let _ = writeln!(out, ";   #{id} {}", fmt_fn(f));
    }
    let _ = writeln!(out, "; code ({} words)", code.len());

    let entry_fid = |pc: usize| -> Option<usize> {
        fns.iter()
            .position(|f| matches!(f.ty, FnType::Hxvm { entry_pc } if entry_pc == pc))
    };

    let mut pc = 0usize;
    while pc < code.len() {
        if let Some(fid) = entry_fid(pc) {
            let _ = writeln!(out, "\nfn #{fid}:");
        }
        if let Some(name) = labels.get(&pc) {
            let _ = writeln!(out, "{name}:");
        }

        let i = code[pc];
        let op = inst::op(i);
        let shape = shape_of(op);

        let mut line = format!("{pc:04}  {i:08x}  {:<14} ", op.to_string());
        let consumed = render_operands(&mut line, shape, i, code, pc, program, &labels);
        let _ = writeln!(out, "{}", line.trim_end());

        pc += consumed;
    }

    out
}

/// Pass 1: walk the code respecting instruction widths, collect every target pc,
/// and assign L-names in ascending pc order.
fn collect_labels(code: &[Instruction]) -> BTreeMap<usize, String> {
    let mut targets: BTreeMap<usize, ()> = BTreeMap::new();
    let mut pc = 0usize;

    while pc < code.len() {
        let i = code[pc];
        let op = inst::op(i);
        match shape_of(op) {
            Shape::AxTarget => {
                targets.insert(inst::ax(i) as usize, ());
                pc += 1;
            }
            Shape::ACondTarget => {
                targets.insert(inst::bx(i) as usize, ());
                pc += 1;
            }
            Shape::CmpBranch => {
                // target is the following word (absolute pc)
                if let Some(t) = code.get(pc + 1) {
                    targets.insert(*t as usize, ());
                }
                pc += 2;
            }
            _ => pc += 1,
        }
    }

    // assign names in pc order (BTreeMap iterates sorted)
    targets
        .into_keys()
        .enumerate()
        .map(|(n, target_pc)| (target_pc, format!("L{n}")))
        .collect()
}

fn render_operands(
    line: &mut String,
    shape: Shape,
    i: Instruction,
    code: &[Instruction],
    pc: usize,
    program: &Program,
    labels: &BTreeMap<usize, String>,
) -> usize {
    let a = inst::a(i);
    let b = inst::b(i);
    let c = inst::c(i);

    let label_for = |target: usize| -> String {
        match labels.get(&target) {
            Some(name) => name.clone(),
            None => format!("{target}"), // unmapped (shouldn't happen for valid code)
        }
    };

    match shape {
        Shape::None => 1,
        Shape::AB => {
            let _ = write!(line, "R{a}, R{b}");
            1
        }
        Shape::ABC => {
            let _ = write!(line, "R{a}, R{b}, R{c}");
            1
        }
        Shape::ABI => {
            let _ = write!(line, "R{a}, R{b}, ${}", inst::imm8(i));
            1
        }
        Shape::ABKc => {
            let _ = write!(line, "R{a}, R{b}, #{} {}", c, const_comment(program, c as usize));
            1
        }
        Shape::AImmBx => {
            let _ = write!(line, "R{a}, ${}", inst::bx_imm(i));
            1
        }
        Shape::AKbx => {
            let idx = inst::bx(i) as usize;
            let _ = write!(line, "R{a}, #{} {}", idx, const_comment(program, idx));
            1
        }
        Shape::ACondTarget => {
            let _ = write!(line, "R{a}, @{}", label_for(inst::bx(i) as usize));
            1
        }
        Shape::AxTarget => {
            let _ = write!(line, "@{}", label_for(inst::ax(i) as usize));
            1
        }
        Shape::ACall => {
            let fid = inst::bx(i) as usize;
            let _ = write!(line, "R{a}, #{fid} {}", fn_comment(program, fid));
            1
        }
        Shape::ACallR => {
            let _ = write!(line, "R{a}, R{b}");
            1
        }
        Shape::CmpBranch => {
            let target = code.get(pc + 1).copied().unwrap_or(0) as usize;
            let _ = write!(line, "R{b}, R{c}, @{}", label_for(target));
            2
        }
    }
}

fn const_comment(program: &Program, idx: usize) -> String {
    match program.constants().get(idx) {
        Some(v) => format!("; {}", fmt_word(*v)),
        None => "; <oob>".into(),
    }
}

fn fn_comment(program: &Program, id: usize) -> String {
    match program.functions().get(id) {
        Some(f) => format!("; {}", fmt_fn(f)),
        None => "; <oob>".into(),
    }
}

fn fmt_fn(f: &Function) -> String {
    match f.ty {
        FnType::Hxvm { entry_pc } => {
            format!("vm@{entry_pc} (narg={}, nret={}, nreg={})", f.narg, f.nret, f.nreg)
        }
        FnType::Host { syscode } => {
            format!("host:{syscode} (narg={}, nret={})", f.narg, f.nret)
        }
        FnType::Native { fn_ptr } => {
            format!("native: {fn_ptr:?}")
        }
    }
}

/// A word is an opaque u64; show all plausible interpretations since the
/// constant pool doesn't carry type tags.
fn fmt_word(w: word) -> String {
    let as_f = f64::from_bits(w);
    format!("0x{w:016x} (i={} f={})", w as i64, as_f)
}
