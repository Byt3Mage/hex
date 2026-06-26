//! hxasm — assembler targeting the hex VM `Program`.
//!
//! Directives:
//!   const NAME = 42            int constant  -> Value
//!   const NAME = 3.14          float constant -> Value
//!   host  NAME(narg, nret) = 7 host function (syscode 7)
//!   fn    NAME(narg, nret, nreg):   VM function; entry_pc = next instruction
//!       <body>
//!       ret
//!
//! Operands:
//!   R0 / r1     register
//!   $123 / $-4  signed immediate (LOADI/LOADF, ADDI/SUBI/MULI)
//!   #NAME       constant-pool reference (LOADK, *K)
//!   @label      jump target (`@label:` defines it)
//!   NAME        function reference (CALL only; const here = error)

use std::collections::HashMap;
use std::fmt;

use hex_vm::*;

#[derive(Debug, Clone)]
pub struct AsmError {
    pub line: usize,
    pub msg: String,
}
impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}
impl std::error::Error for AsmError {}

type R<T> = Result<T, AsmError>;
fn err<T>(line: usize, msg: impl Into<String>) -> R<T> {
    Err(AsmError { line, msg: msg.into() })
}

// ── Operands ──────────────────────────────────────────────────────────────

enum Operand {
    Reg(Reg),
    Imm(i64),
    Const(String),
    Label(String),
    Name(String),
}

fn parse_operand(tok: &str, line: usize) -> R<Operand> {
    let t = tok.trim();
    if t.is_empty() {
        return err(line, "empty operand");
    }
    let first = t.chars().next().unwrap();
    match first {
        'R' | 'r' if t.len() > 1 && t[1..].bytes().all(|b| b.is_ascii_digit()) => {
            let n: u32 = t[1..].parse().unwrap();
            if n > Reg::MAX as u32 {
                return err(line, format!("register out of range '{t}'"));
            }
            Ok(Operand::Reg(n as Reg))
        }
        '$' => {
            let v: i64 = t[1..]
                .parse()
                .map_err(|_| AsmError { line, msg: format!("bad immediate '{t}'") })?;
            Ok(Operand::Imm(v))
        }
        '#' => Ok(Operand::Const(t[1..].to_string())),
        '@' => Ok(Operand::Label(t[1..].to_string())),
        _ => Ok(Operand::Name(t.to_string())),
    }
}

fn as_reg(op: &Operand, line: usize) -> R<Reg> {
    match op {
        Operand::Reg(r) => Ok(*r),
        _ => err(line, "expected register"),
    }
}
fn as_imm(op: &Operand, line: usize) -> R<i64> {
    match op {
        Operand::Imm(v) => Ok(*v),
        _ => err(line, "expected $immediate"),
    }
}

fn split_mnemonic_operands(text: &str) -> (String, Vec<String>) {
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let mnem = parts.next().unwrap_or("").to_lowercase();
    let rest = parts.next().unwrap_or("");
    let ops = rest
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (mnem, ops)
}

// ── Pending instructions (pre-resolution) ─────────────────────────────────

enum Pending {
    Word(Instruction),
    CmpBranch {
        op: Opcode,
        a: Reg,
        b: Reg,
        label: String,
        line: usize,
    },
    JmpAx {
        label: String,
        line: usize,
    },
    JmpCond {
        op: Opcode,
        cond: Reg,
        label: String,
        line: usize,
    },
    LoadK {
        dst: Reg,
        name: String,
        line: usize,
    },
    ArithK {
        op: Opcode,
        dst: Reg,
        src: Reg,
        name: String,
        line: usize,
    },
    Call {
        ret: Reg,
        name: String,
        tail: bool,
        line: usize,
    },
}

// FnType building needs to know host vs vm; track separately from Function
// because entry_pc isn't known until layout for vm fns.
struct PendingFn {
    name: String,
    narg: Reg,
    nret: Reg,
    nreg: Reg,
    kind: PendingFnKind,
}
enum PendingFnKind {
    Vm { entry_pc: usize },
    Host { syscode: u8 },
}

#[derive(Default)]
struct Builder {
    code: Vec<Pending>,
    constants: Vec<Value>,
    const_by_name: HashMap<String, usize>,
    fns: Vec<PendingFn>,
    fn_index: HashMap<String, usize>,
    labels: HashMap<String, usize>,
    pc: usize,
}

pub fn assemble(src: &str) -> Result<Program, AsmError> {
    let mut b = Builder::default();

    for (idx, raw) in src.lines().enumerate() {
        let line = idx + 1;
        let text = strip_comment(raw).trim();
        if text.is_empty() {
            continue;
        }

        if let Some(rest) = text.strip_prefix("const ") {
            let (name, v) = parse_const_def(rest, line)?;
            if b.const_by_name.contains_key(&name) {
                return err(line, format!("duplicate const '{name}'"));
            }
            let id = b.constants.len();
            b.constants.push(v);
            b.const_by_name.insert(name, id);
            continue;
        }

        if let Some(rest) = text.strip_prefix("host ") {
            let pf = parse_host_header(rest, line)?;
            register_fn(&mut b, pf, line)?;
            continue;
        }

        if let Some(rest) = text.strip_prefix("fn ") {
            let pf = parse_fn_header(rest, b.pc, line)?;
            register_fn(&mut b, pf, line)?;
            continue;
        }

        if let Some(lbl) = text.strip_prefix('@') {
            if let Some(name) = lbl.strip_suffix(':') {
                let name = name.trim().to_string();
                if b.labels.contains_key(&name) {
                    return err(line, format!("duplicate label '@{name}'"));
                }
                b.labels.insert(name, b.pc);
                continue;
            }
            return err(line, "label must end with ':'");
        }

        emit_instruction(&mut b, text, line)?;
    }

    resolve(b)
}

fn register_fn(b: &mut Builder, pf: PendingFn, line: usize) -> R<()> {
    if b.fn_index.contains_key(&pf.name) {
        return err(line, format!("duplicate function '{}'", pf.name));
    }
    b.fn_index.insert(pf.name.clone(), b.fns.len());
    b.fns.push(pf);
    Ok(())
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn parse_const_def(rest: &str, line: usize) -> R<(String, Value)> {
    let mut sides = rest.splitn(2, '=');
    let name = sides
        .next()
        .ok_or_else(|| AsmError { line, msg: "missing const name".into() })?
        .trim()
        .to_string();
    let val = sides
        .next()
        .ok_or_else(|| AsmError { line, msg: "missing '=' in const".into() })?
        .trim();
    if name.is_empty() {
        return err(line, "empty const name");
    }
    let value = if val.contains('.') || val.contains('e') || val.contains('E') {
        let f: f64 = val
            .parse()
            .map_err(|_| AsmError { line, msg: format!("bad float '{val}'") })?;
        f.into_value()
    } else {
        let i: i64 = val
            .parse()
            .map_err(|_| AsmError { line, msg: format!("bad int '{val}'") })?;
        i.into_value()
    };
    Ok((name, value))
}

fn parse_counts<'a>(rest: &'a str, line: usize) -> R<(String, Vec<&'a str>)> {
    let open = rest
        .find('(')
        .ok_or_else(|| AsmError { line, msg: "header needs '('".into() })?;
    let close = rest
        .find(')')
        .ok_or_else(|| AsmError { line, msg: "header needs ')'".into() })?;
    let name = rest[..open].trim().to_string();
    let args = rest[open + 1..close].split(',').map(|s| s.trim()).collect();
    Ok((name, args))
}

fn parse_u8(s: &str, line: usize) -> R<Reg> {
    s.parse::<Reg>()
        .map_err(|_| AsmError { line, msg: format!("bad count '{s}'") })
}

fn parse_fn_header(rest: &str, entry_pc: usize, line: usize) -> R<PendingFn> {
    let rest = rest.trim().trim_end_matches(':').trim();
    let (name, args) = parse_counts(rest, line)?;
    if args.len() != 3 {
        return err(line, "fn header needs (narg, nret, nreg)");
    }
    Ok(PendingFn {
        name,
        narg: parse_u8(args[0], line)?,
        nret: parse_u8(args[1], line)?,
        nreg: parse_u8(args[2], line)?,
        kind: PendingFnKind::Vm { entry_pc },
    })
}

fn parse_host_header(rest: &str, line: usize) -> R<PendingFn> {
    // host NAME(narg, nret) = syscode
    let mut sides = rest.splitn(2, '=');
    let head = sides.next().unwrap().trim();
    let code = sides
        .next()
        .ok_or_else(|| AsmError { line, msg: "host needs '= syscode'".into() })?
        .trim();
    let (name, args) = parse_counts(head, line)?;
    if args.len() != 2 {
        return err(line, "host header needs (narg, nret)");
    }
    let syscode: u8 = code
        .parse()
        .map_err(|_| AsmError { line, msg: format!("bad syscode '{code}'") })?;
    Ok(PendingFn {
        name,
        narg: parse_u8(args[0], line)?,
        nret: parse_u8(args[1], line)?,
        nreg: 0, // host fns use no VM registers
        kind: PendingFnKind::Host { syscode },
    })
}

fn emit_instruction(b: &mut Builder, text: &str, line: usize) -> R<()> {
    let (mnem, ops_raw) = split_mnemonic_operands(text);
    let ops: Vec<Operand> = ops_raw.iter().map(|s| parse_operand(s, line)).collect::<R<Vec<_>>>()?;

    macro_rules! reg {
        ($i:expr) => {
            as_reg(
                ops.get($i)
                    .ok_or_else(|| AsmError { line, msg: "too few operands".into() })?,
                line,
            )?
        };
    }
    macro_rules! imm {
        ($i:expr) => {
            as_imm(
                ops.get($i)
                    .ok_or_else(|| AsmError { line, msg: "too few operands".into() })?,
                line,
            )?
        };
    }
    macro_rules! cname {
        ($i:expr) => {
            match ops.get($i) {
                Some(Operand::Const(s)) => s.clone(),
                _ => return err(line, "expected #const operand"),
            }
        };
    }
    macro_rules! label {
        ($i:expr) => {
            match ops.get($i) {
                Some(Operand::Label(s)) => s.clone(),
                _ => return err(line, "expected @label operand"),
            }
        };
    }
    macro_rules! fname {
        ($i:expr) => {
            match ops.get($i) {
                Some(Operand::Name(s)) => s.clone(),
                _ => return err(line, "expected function name operand"),
            }
        };
    }

    let one = |b: &mut Builder, p: Pending| {
        b.code.push(p);
        b.pc += 1;
    };
    let two = |b: &mut Builder, p: Pending| {
        b.code.push(p);
        b.pc += 2;
    };

    match mnem.as_str() {
        "copy" => one(b, Pending::Word(copy(reg!(0), reg!(1)))),
        "loadi" => one(b, Pending::Word(loadi(reg!(0), imm!(1)))),
        "loadf" => one(b, Pending::Word(loadf(reg!(0), imm!(1)))),
        "loadk" => one(b, Pending::LoadK { dst: reg!(0), name: cname!(1), line }),

        "not" => one(b, Pending::Word(not(reg!(0), reg!(1)))),
        "bnot" => one(b, Pending::Word(bnot(reg!(0), reg!(1)))),
        "ineg" => one(b, Pending::Word(ineg(reg!(0), reg!(1)))),
        "fneg" => one(b, Pending::Word(fneg(reg!(0), reg!(1)))),

        "add" => one(b, Pending::Word(add(reg!(0), reg!(1), reg!(2)))),
        "sub" => one(b, Pending::Word(sub(reg!(0), reg!(1), reg!(2)))),
        "mul" => one(b, Pending::Word(mul(reg!(0), reg!(1), reg!(2)))),
        "addi" => one(b, Pending::Word(addi(reg!(0), reg!(1), mk_imm8(imm!(2), line)?))),
        "subi" => one(b, Pending::Word(subi(reg!(0), reg!(1), mk_imm8(imm!(2), line)?))),
        "muli" => one(b, Pending::Word(muli(reg!(0), reg!(1), mk_imm8(imm!(2), line)?))),
        "addk" => one(
            b,
            Pending::ArithK {
                op: Opcode::ADDK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "subk" => one(
            b,
            Pending::ArithK {
                op: Opcode::SUBK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "mulk" => one(
            b,
            Pending::ArithK {
                op: Opcode::MULK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "faddk" => one(
            b,
            Pending::ArithK {
                op: Opcode::FADDK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "fsubk" => one(
            b,
            Pending::ArithK {
                op: Opcode::FSUBK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "fmulk" => one(
            b,
            Pending::ArithK {
                op: Opcode::FMULK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "fdivk" => one(
            b,
            Pending::ArithK {
                op: Opcode::FDIVK,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),

        "sdiv" => one(b, Pending::Word(sdiv(reg!(0), reg!(1), reg!(2)))),
        "srem" => one(b, Pending::Word(srem(reg!(0), reg!(1), reg!(2)))),
        "udiv" => one(b, Pending::Word(udiv(reg!(0), reg!(1), reg!(2)))),
        "urem" => one(b, Pending::Word(urem(reg!(0), reg!(1), reg!(2)))),

        "fadd" => one(b, Pending::Word(fadd(reg!(0), reg!(1), reg!(2)))),
        "fsub" => one(b, Pending::Word(fsub(reg!(0), reg!(1), reg!(2)))),
        "fmul" => one(b, Pending::Word(fmul(reg!(0), reg!(1), reg!(2)))),
        "fdiv" => one(b, Pending::Word(fdiv(reg!(0), reg!(1), reg!(2)))),
        "frem" => one(b, Pending::Word(frem(reg!(0), reg!(1), reg!(2)))),

        "eq" => one(b, Pending::Word(eq(reg!(0), reg!(1), reg!(2)))),
        "ne" => one(b, Pending::Word(ne(reg!(0), reg!(1), reg!(2)))),
        "slt" => one(b, Pending::Word(ilt(reg!(0), reg!(1), reg!(2)))),
        "sgt" => one(b, Pending::Word(igt(reg!(0), reg!(1), reg!(2)))),
        "sle" => one(b, Pending::Word(ile(reg!(0), reg!(1), reg!(2)))),
        "sge" => one(b, Pending::Word(ige(reg!(0), reg!(1), reg!(2)))),
        "ult" => one(b, Pending::Word(ult(reg!(0), reg!(1), reg!(2)))),
        "ugt" => one(b, Pending::Word(ugt(reg!(0), reg!(1), reg!(2)))),
        "ule" => one(b, Pending::Word(ule(reg!(0), reg!(1), reg!(2)))),
        "uge" => one(b, Pending::Word(uge(reg!(0), reg!(1), reg!(2)))),
        "feq" => one(b, Pending::Word(feq(reg!(0), reg!(1), reg!(2)))),
        "fne" => one(b, Pending::Word(fne(reg!(0), reg!(1), reg!(2)))),
        "flt" => one(b, Pending::Word(flt(reg!(0), reg!(1), reg!(2)))),
        "fgt" => one(b, Pending::Word(fgt(reg!(0), reg!(1), reg!(2)))),
        "fle" => one(b, Pending::Word(fle(reg!(0), reg!(1), reg!(2)))),
        "fge" => one(b, Pending::Word(fge(reg!(0), reg!(1), reg!(2)))),

        "jmp" => one(b, Pending::JmpAx { label: label!(0), line }),
        "jmpt" => one(
            b,
            Pending::JmpCond {
                op: Opcode::JMP_T,
                cond: reg!(0),
                label: label!(1),
                line,
            },
        ),
        "jmpf" => one(
            b,
            Pending::JmpCond {
                op: Opcode::JMP_F,
                cond: reg!(0),
                label: label!(1),
                line,
            },
        ),

        "jeq" | "jne" | "jslt" | "jsgt" | "jsle" | "jsge" | "jult" | "jugt" | "jule" | "juge" | "jfeq" | "jfne"
        | "jflt" | "jfgt" | "jfle" | "jfge" => {
            let op = cmp_branch_opcode(&mnem).unwrap();
            two(
                b,
                Pending::CmpBranch { op, a: reg!(0), b: reg!(1), label: label!(2), line },
            );
        }

        "load" => one(b, Pending::Word(load(reg!(0), reg!(1), reg!(2)))),
        "store" => one(b, Pending::Word(store(reg!(0), reg!(1), reg!(2)))),
        "store_address" => one(b, Pending::Word(store_address(reg!(0), reg!(1), reg!(2)))),

        "call" => one(b, Pending::Call { ret: reg!(0), name: fname!(1), tail: false, line }),
        "callr" => one(b, Pending::Word(callr(reg!(0), reg!(1)))),
        "tcall" => one(b, Pending::Call { ret: reg!(0), name: fname!(1), tail: true, line }),
        "tcallr" => one(b, Pending::Word(tcallr(reg!(0), reg!(1)))),
        "ret" => one(b, Pending::Word(ret())),
        "halt" => one(b, Pending::Word(halt())),

        other => return err(line, format!("unknown mnemonic '{other}'")),
    }
    Ok(())
}

fn mk_imm8(v: i64, line: usize) -> R<Imm8> {
    Imm8::from_int(v).ok_or_else(|| AsmError {
        line,
        msg: format!("immediate {v} out of imm8 range"),
    })
}

fn cmp_branch_opcode(mnem: &str) -> Option<Opcode> {
    Some(match mnem {
        "jeq" => Opcode::JEQ,
        "jne" => Opcode::JNE,
        "jslt" => Opcode::JSLT,
        "jsgt" => Opcode::JSGT,
        "jsle" => Opcode::JSLE,
        "jsge" => Opcode::JSGE,
        "jult" => Opcode::JULT,
        "jugt" => Opcode::JUGT,
        "jule" => Opcode::JULE,
        "juge" => Opcode::JUGE,
        "jfeq" => Opcode::JFEQ,
        "jfne" => Opcode::JFNE,
        "jflt" => Opcode::JFLT,
        "jfgt" => Opcode::JFGT,
        "jfle" => Opcode::JFLE,
        "jfge" => Opcode::JFGE,
        _ => return None,
    })
}

fn resolve(b: Builder) -> Result<Program, AsmError> {
    let mut code: Vec<Instruction> = Vec::with_capacity(b.code.len());

    let const_id = |name: &str, line: usize| -> R<usize> {
        b.const_by_name
            .get(name)
            .copied()
            .ok_or_else(|| AsmError { line, msg: format!("unknown const '#{name}'") })
    };
    let label_pc = |name: &str, line: usize| -> R<usize> {
        b.labels
            .get(name)
            .copied()
            .ok_or_else(|| AsmError { line, msg: format!("unknown label '@{name}'") })
    };

    for p in &b.code {
        match p {
            Pending::Word(w) => code.push(*w),
            Pending::LoadK { dst, name, line } => {
                code.push(loadk(*dst, const_id(name, *line)? as Instruction));
            }
            Pending::ArithK { op, dst, src, name, line } => {
                let idx = const_id(name, *line)?;
                if idx > Reg::MAX as usize {
                    return err(*line, format!("const index {idx} exceeds c-field (max {})", Reg::MAX));
                }
                code.push(encode_abc(*op, *dst, *src, idx as Reg));
            }
            Pending::JmpAx { label, line } => {
                code.push(jmp(label_pc(label, *line)? as Instruction));
            }
            Pending::JmpCond { op, cond, label, line } => {
                code.push(encode_abx(*op, *cond, label_pc(label, *line)? as Instruction));
            }
            Pending::CmpBranch { op, a, b: bb, label, line } => {
                let target = label_pc(label, *line)? as Instruction;
                code.push(encode_abc(*op, hex_vm::R0, *a, *bb));
                code.push(target);
            }
            Pending::Call { ret, name, tail, line } => {
                let fid = b.fn_index.get(name).copied().ok_or_else(|| AsmError {
                    line: *line,
                    msg: format!("unknown function '{name}'"),
                })?;
                if fid > u16::MAX as usize {
                    return err(*line, "too many functions");
                }
                code.push(if *tail { tcall(*ret, fid as Instruction) } else { call(*ret, fid as Instruction) });
            }
        }
    }

    // build the Function table in fn_index order (== push order)
    let functions: Vec<Function> = b
        .fns
        .iter()
        .map(|pf| Function {
            ty: match pf.kind {
                PendingFnKind::Vm { entry_pc } => FnType::Hxvm { entry_pc },
                PendingFnKind::Host { syscode } => FnType::Host { syscode },
            },
            narg: pf.narg,
            nret: pf.nret,
            nreg: pf.nreg,
        })
        .collect();

    Ok(Program::new(
        code.into_boxed_slice(),
        b.constants.into_boxed_slice(),
        functions.into_boxed_slice(),
    ))
}

struct Host;
impl hex_vm::Host for Host {
    fn syscall(&mut self, _: Syscode, _: HostCtx) -> Result<Flow, Error> {
        unimplemented!()
    }
}

#[test]
fn test_assemble() {
    let source = include_str!("test.hxa");

    let mut mem = vec![0];

    let program = assemble(source).unwrap();

    println!("{}", disassemble(&program));

    let args = &[5u64.into_value(), 1u64.into_value()];
    let args = Args::new(args).unwrap();
    let mut vm = VM::from_entry(&program, 0, args).unwrap();

    match hex_vm::run(&mut vm, &program, &mut Host, &mut mem) {
        Ok(outcome) => match outcome {
            RunOutcome::Completed => {
                let ret: u64 = vm.registers[0].get();
                println!("ret: {ret}")
            }
            RunOutcome::Suspended => println!("suspended"),
            RunOutcome::Trapped(fault) => println!("trapped: {fault}"),
        },
        Err(err) => println!("{err}"),
    }
}
