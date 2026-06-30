//! hxasm — assembler targeting the hex VM `Program`.
//!
//! Directives:
//!   const NAME = 42             int constant   -> Value
//!   const NAME = 3.14           float constant -> Value
//!   host  NAME(narg, nret) = 7  host function (syscode 7)
//!   fn    NAME(narg, nret, nreg):    VM function; entry_pc = next instruction
//!       <body>
//!       ret
//!
//!   try @handler, Rcatch        begin protected region (covers following insts)
//!       <body>
//!   endtry                      end protected region
//!   @handler:                   handler label (normal label); Rcatch holds thrown value
//!
//! Operands:
//!   R0 / r1     register
//!   $123 / $-4  signed immediate (LOADI/LOADF, ADDI/SUBI/MULI)
//!   #NAME       constant-pool reference (LOADK, *K)
//!   @label      jump / handler target
//!   NAME        function reference (CALL/TCALL only; const here = error)

use hex_vm::*;
use std::collections::HashMap;
use std::fmt;

// ── Errors ────────────────────────────────────────────────────────────────

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
    /// cmp-branch: two words; regs known, target label resolved later
    CmpBranch {
        f: CmpFn,
        a: Reg,
        b: Reg,
        label: String,
        line: usize,
    },
    JmpAx {
        label: String,
        line: usize,
    },
    JmpT {
        cond: Reg,
        label: String,
        line: usize,
    },
    JmpF {
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
        f: ArithKFn,
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

type CmpFn = fn(Reg, Reg, Instruction) -> [Instruction; 2];
type ArithKFn = fn(Reg, Reg, Reg) -> Instruction;

enum PendingFnKind {
    Vm { entry_pc: usize },
    Host { syscode: u8 },
}
struct PendingFn {
    narg: Reg,
    nret: Reg,
    nreg: Reg,
    kind: PendingFnKind,
}

struct OpenTry {
    start_pc: usize,
    handler_label: String,
    catch_reg: Reg,
    line: usize,
}
struct PendingHandler {
    fn_idx: usize,
    start_pc: usize,
    end_pc: usize,
    handler_label: String,
    catch_reg: Reg,
    line: usize,
}

#[derive(Default)]
struct Builder {
    code: Vec<Pending>,
    constants: Vec<word>,
    const_by_name: HashMap<String, usize>,
    fns: Vec<PendingFn>,
    fn_index: HashMap<String, usize>,
    labels: HashMap<String, usize>,
    pc: usize,

    open_tries: Vec<OpenTry>,
    handlers: Vec<PendingHandler>,
    cur_fn: Option<usize>,
}

pub fn assemble(src: &str) -> Result<ProgramBuf, AsmError> {
    let mut b = Builder::default();

    for (idx, raw) in src.lines().enumerate() {
        let line = idx + 1;
        let text = strip_comment(raw).trim();
        if text.is_empty() {
            continue;
        }

        // const NAME = value
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

        // host NAME(narg, nret) = syscode
        if let Some(rest) = text.strip_prefix("host ") {
            if !b.open_tries.is_empty() {
                return err(line, "unclosed 'try' at function boundary");
            }
            let (name, pf) = parse_host_header(rest, line)?;
            let i = register_fn(&mut b, name, pf, line)?;
            b.cur_fn = Some(i);
            continue;
        }

        // fn NAME(narg, nret, nreg):
        if let Some(rest) = text.strip_prefix("fn ") {
            if !b.open_tries.is_empty() {
                return err(line, "unclosed 'try' at function boundary");
            }
            let (name, pf) = parse_fn_header(rest, b.pc, line)?;
            let i = register_fn(&mut b, name, pf, line)?;
            b.cur_fn = Some(i);
            continue;
        }

        // try @handler, Rcatch
        if let Some(rest) = text.strip_prefix("try ") {
            let (label, reg) = parse_try(rest, line)?;
            b.open_tries.push(OpenTry {
                start_pc: b.pc,
                handler_label: label,
                catch_reg: reg,
                line,
            });
            continue;
        }

        // endtry
        if text == "endtry" {
            let fn_idx = b
                .cur_fn
                .ok_or_else(|| AsmError { line, msg: "'endtry' outside a function".into() })?;
            let open = b.open_tries.pop().ok_or_else(|| AsmError {
                line,
                msg: "'endtry' without matching 'try'".into(),
            })?;
            b.handlers.push(PendingHandler {
                fn_idx,
                start_pc: open.start_pc,
                end_pc: b.pc,
                handler_label: open.handler_label,
                catch_reg: open.catch_reg,
                line: open.line,
            });
            continue;
        }

        // @label:
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

    if !b.open_tries.is_empty() {
        let t = b.open_tries.last().unwrap();
        return err(t.line, "unclosed 'try' at end of input");
    }

    resolve(b)
}

fn register_fn(b: &mut Builder, name: String, pf: PendingFn, line: usize) -> R<usize> {
    if b.fn_index.contains_key(&name) {
        return err(line, format!("duplicate function '{name}'"));
    }
    let idx = b.fns.len();
    b.fn_index.insert(name, idx);
    b.fns.push(pf);
    Ok(idx)
}

fn strip_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

fn parse_const_def(rest: &str, line: usize) -> R<(String, word)> {
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
        f.into_word()
    } else {
        let i: i64 = val
            .parse()
            .map_err(|_| AsmError { line, msg: format!("bad int '{val}'") })?;
        i.into_word()
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

fn parse_fn_header(rest: &str, entry_pc: usize, line: usize) -> R<(String, PendingFn)> {
    let rest = rest.trim().trim_end_matches(':').trim();
    let (name, args) = parse_counts(rest, line)?;
    if args.len() != 3 {
        return err(line, "fn header needs (narg, nret, nreg)");
    }
    Ok((
        name,
        PendingFn {
            narg: parse_u8(args[0], line)?,
            nret: parse_u8(args[1], line)?,
            nreg: parse_u8(args[2], line)?,
            kind: PendingFnKind::Vm { entry_pc },
        },
    ))
}

fn parse_host_header(rest: &str, line: usize) -> R<(String, PendingFn)> {
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
    Ok((
        name,
        PendingFn {
            narg: parse_u8(args[0], line)?,
            nret: parse_u8(args[1], line)?,
            nreg: 0,
            kind: PendingFnKind::Host { syscode },
        },
    ))
}

fn parse_try(rest: &str, line: usize) -> R<(String, Reg)> {
    let mut parts = rest.split(',').map(|s| s.trim());
    let label = parts.next().unwrap_or("");
    let reg = parts
        .next()
        .ok_or_else(|| AsmError { line, msg: "try needs '@handler, Rreg'".into() })?;
    let label = label
        .strip_prefix('@')
        .ok_or_else(|| AsmError { line, msg: "try handler must be @label".into() })?
        .to_string();
    let reg = match parse_operand(reg, line)? {
        Operand::Reg(r) => r,
        _ => return err(line, "try catch target must be a register"),
    };
    Ok((label, reg))
}

// ── Instruction emission ──────────────────────────────────────────────────

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
    macro_rules! lbl {
        ($i:expr) => {
            match ops.get($i) {
                Some(Operand::Label(s)) => s.clone(),
                _ => return err(line, "expected @label operand"),
            }
        };
    }
    macro_rules! fnm {
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
        // moves
        "copy" => one(b, Pending::Word(copy(reg!(0), reg!(1)))),
        "loadi" => one(b, Pending::Word(loadi(reg!(0), imm!(1)))),
        "loadf" => one(b, Pending::Word(loadf(reg!(0), imm!(1)))),
        "loadk" => one(b, Pending::LoadK { dst: reg!(0), name: cname!(1), line }),

        // unary
        "not" => one(b, Pending::Word(not(reg!(0), reg!(1)))),
        "bnot" => one(b, Pending::Word(bnot(reg!(0), reg!(1)))),
        "ineg" => one(b, Pending::Word(ineg(reg!(0), reg!(1)))),
        "fneg" => one(b, Pending::Word(fneg(reg!(0), reg!(1)))),

        // int arith
        "add" => one(b, Pending::Word(add(reg!(0), reg!(1), reg!(2)))),
        "sub" => one(b, Pending::Word(sub(reg!(0), reg!(1), reg!(2)))),
        "mul" => one(b, Pending::Word(mul(reg!(0), reg!(1), reg!(2)))),
        "addi" => one(b, Pending::Word(addi(reg!(0), reg!(1), mk_imm8(imm!(2), line)?))),
        "subi" => one(b, Pending::Word(subi(reg!(0), reg!(1), mk_imm8(imm!(2), line)?))),
        "muli" => one(b, Pending::Word(muli(reg!(0), reg!(1), mk_imm8(imm!(2), line)?))),
        "addk" => one(
            b,
            Pending::ArithK {
                f: addk,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "subk" => one(
            b,
            Pending::ArithK {
                f: subk,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "mulk" => one(
            b,
            Pending::ArithK {
                f: mulk,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "faddk" => one(
            b,
            Pending::ArithK {
                f: faddk,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "fsubk" => one(
            b,
            Pending::ArithK {
                f: fsubk,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "fmulk" => one(
            b,
            Pending::ArithK {
                f: fmulk,
                dst: reg!(0),
                src: reg!(1),
                name: cname!(2),
                line,
            },
        ),
        "fdivk" => one(
            b,
            Pending::ArithK {
                f: fdivk,
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

        // float arith
        "fadd" => one(b, Pending::Word(fadd(reg!(0), reg!(1), reg!(2)))),
        "fsub" => one(b, Pending::Word(fsub(reg!(0), reg!(1), reg!(2)))),
        "fmul" => one(b, Pending::Word(fmul(reg!(0), reg!(1), reg!(2)))),
        "fdiv" => one(b, Pending::Word(fdiv(reg!(0), reg!(1), reg!(2)))),
        "frem" => one(b, Pending::Word(frem(reg!(0), reg!(1), reg!(2)))),

        // comparisons (write bool)
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

        // jumps
        "jmp" => one(b, Pending::JmpAx { label: lbl!(0), line }),
        "jmpt" => one(b, Pending::JmpT { cond: reg!(0), label: lbl!(1), line }),
        "jmpf" => one(b, Pending::JmpF { cond: reg!(0), label: lbl!(1), line }),

        // compare-branches (two words)
        "jeq" => two(
            b,
            Pending::CmpBranch {
                f: jeq,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jne" => two(
            b,
            Pending::CmpBranch {
                f: jne,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jslt" => two(
            b,
            Pending::CmpBranch {
                f: jslt,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jsgt" => two(
            b,
            Pending::CmpBranch {
                f: jsgt,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jsle" => two(
            b,
            Pending::CmpBranch {
                f: jsle,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jsge" => two(
            b,
            Pending::CmpBranch {
                f: jsge,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jult" => two(
            b,
            Pending::CmpBranch {
                f: jult,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jugt" => two(
            b,
            Pending::CmpBranch {
                f: jugt,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jule" => two(
            b,
            Pending::CmpBranch {
                f: jule,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "juge" => two(
            b,
            Pending::CmpBranch {
                f: juge,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jfeq" => two(
            b,
            Pending::CmpBranch {
                f: jfeq,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jfne" => two(
            b,
            Pending::CmpBranch {
                f: jfne,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jflt" => two(
            b,
            Pending::CmpBranch {
                f: jflt,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jfgt" => two(
            b,
            Pending::CmpBranch {
                f: jfgt,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jfle" => two(
            b,
            Pending::CmpBranch {
                f: jfle,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),
        "jfge" => two(
            b,
            Pending::CmpBranch {
                f: jfge,
                a: reg!(0),
                b: reg!(1),
                label: lbl!(2),
                line,
            },
        ),

        // memory
        "load" => one(b, Pending::Word(load(reg!(0), reg!(1), reg!(2)))),
        "store" => one(b, Pending::Word(store(reg!(0), reg!(1), reg!(2)))),
        "store_address" => one(b, Pending::Word(store_address(reg!(0), reg!(1), reg!(2)))),

        // calls / return / unwind
        "call" => one(b, Pending::Call { ret: reg!(0), name: fnm!(1), tail: false, line }),
        "tcall" => one(b, Pending::Call { ret: reg!(0), name: fnm!(1), tail: true, line }),
        "callr" => one(b, Pending::Word(callr(reg!(0), reg!(1)))),
        "tcallr" => one(b, Pending::Word(tcallr(reg!(0), reg!(1)))),
        "throw" => one(b, Pending::Word(throw(reg!(0)))),
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

// ── Resolution ────────────────────────────────────────────────────────────

fn resolve(b: Builder) -> Result<ProgramBuf, AsmError> {
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
            Pending::ArithK { f, dst, src, name, line } => {
                let idx = const_id(name, *line)?;
                if idx > Reg::MAX as usize {
                    return err(*line, format!("const index {idx} exceeds c-field (max {})", Reg::MAX));
                }
                code.push(f(*dst, *src, idx as Reg));
            }
            Pending::JmpAx { label, line } => {
                code.push(jmp(label_pc(label, *line)? as Instruction));
            }
            Pending::JmpT { cond, label, line } => {
                code.push(jmpt(*cond, label_pc(label, *line)? as Instruction));
            }
            Pending::JmpF { cond, label, line } => {
                code.push(jmpf(*cond, label_pc(label, *line)? as Instruction));
            }
            Pending::CmpBranch { f, a, b: bb, label, line } => {
                let target = label_pc(label, *line)? as Instruction;
                let pair = f(*a, *bb, target);
                code.push(pair[0]);
                code.push(pair[1]);
            }
            Pending::Call { ret, name, tail, line } => {
                let fid = b.fn_index.get(name).copied().ok_or_else(|| AsmError {
                    line: *line,
                    msg: format!("unknown function '{name}'"),
                })?;
                if fid > u16::MAX as usize {
                    return err(*line, "too many functions");
                }
                let w = if *tail { tcall(*ret, fid as Instruction) } else { call(*ret, fid as Instruction) };
                code.push(w);
            }
        }
    }

    // resolve handler labels, grouped by function
    let mut per_fn: Vec<Vec<HandlerEntry>> = (0..b.fns.len()).map(|_| Vec::new()).collect();
    for h in &b.handlers {
        let handler_pc = b.labels.get(&h.handler_label).copied().ok_or_else(|| AsmError {
            line: h.line,
            msg: format!("unknown handler label '@{}'", h.handler_label),
        })?;
        per_fn[h.fn_idx].push(HandlerEntry {
            start_pc: h.start_pc,
            end_pc: h.end_pc,
            handler_pc,
            catch_reg: h.catch_reg,
        });
    }
    // outer-first ordering so VM's reverse scan finds innermost handler first
    for entries in per_fn.iter_mut() {
        entries.sort_by(|x, y| x.start_pc.cmp(&y.start_pc).then(y.end_pc.cmp(&x.end_pc)));
    }

    let functions: Vec<Function> = b
        .fns
        .iter()
        .enumerate()
        .map(|(idx, pf)| Function {
            ty: match pf.kind {
                PendingFnKind::Vm { entry_pc } => FnType::Hxvm { entry_pc },
                PendingFnKind::Host { syscode } => FnType::Host { syscode },
            },
            narg: pf.narg,
            nret: pf.nret,
            nreg: pf.nreg,
            handlers: std::mem::take(&mut per_fn[idx]).into_boxed_slice(),
        })
        .collect();

    Ok(ProgramBuf::new(
        code.into_boxed_slice(),
        b.constants.into_boxed_slice(),
        functions.into_boxed_slice(),
        Box::new([]),
    ))
}
