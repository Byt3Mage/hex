//! Pretty-printer for MIR. Produces human-readable output suitable for
//! debugging, golden-file tests, and `Display` impls.

use std::fmt::{self, Display, Formatter, Write};

use crate::{
    BasicBlock, BlockId, ConstVal, Function, Inst, Term, Ty, Val,
    op::{BinOp, UnOp},
};

impl Display for Val {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.idx())
    }
}

impl Display for BlockId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "block{}", self.idx())
    }
}

impl Display for Ty {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = match self {
            Ty::Int => "int",
            Ty::Uint => "uint",
            Ty::Bool => "bool",
            Ty::Float => "float",
        };
        f.write_str(s)
    }
}

impl Display for ConstVal {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            ConstVal::Int(i) => write!(f, "{}", i),
            ConstVal::Uint(u) => write!(f, "{}", u),
            ConstVal::Bool(b) => write!(f, "{}", b),
            ConstVal::Float(x) => {
                // Use a representation that round-trips: always include
                // a decimal point or exponent so it's visually distinct
                // from an integer.
                if x.is_nan() {
                    f.write_str("nan")
                } else if x.is_infinite() {
                    if *x < 0.0 {
                        f.write_str("-inf")
                    } else {
                        f.write_str("inf")
                    }
                } else if x.fract() == 0.0 && x.abs() < 1e16 {
                    write!(f, "{:.1}", x)
                } else {
                    write!(f, "{}", x)
                }
            }
        }
    }
}

impl Display for BinOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = match self {
            BinOp::Add => "add",
            BinOp::Sub => "sub",
            BinOp::Mul => "mul",
            BinOp::SDiv => "sdiv",
            BinOp::SRem => "srem",
            BinOp::UDiv => "udiv",
            BinOp::URem => "urem",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Xor => "xor",
            BinOp::Shl => "shl",
            BinOp::LShr => "lshr",
            BinOp::AShr => "ashr",
            BinOp::Eq => "eq",
            BinOp::Ne => "ne",
            BinOp::SLt => "slt",
            BinOp::SLe => "sle",
            BinOp::SGt => "sgt",
            BinOp::SGe => "sge",
            BinOp::ULt => "ult",
            BinOp::ULe => "ule",
            BinOp::UGt => "ugt",
            BinOp::UGe => "uge",
            BinOp::FAdd => "fadd",
            BinOp::FSub => "fsub",
            BinOp::FMul => "fmul",
            BinOp::FDiv => "fdiv",
            BinOp::FRem => "frem",
            BinOp::FEq => "feq",
            BinOp::FNe => "fne",
            BinOp::FLt => "flt",
            BinOp::FLe => "fle",
            BinOp::FGt => "fgt",
            BinOp::FGe => "fge",
        };
        f.write_str(s)
    }
}

impl Display for UnOp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let s = match self {
            UnOp::Not => "not",
            UnOp::BNot => "bnot",
            UnOp::INeg => "ineg",
            UnOp::FNeg => "fneg",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// FunctionPrinter: formats a Function in the context where Val types are
// known. We can't impl Display directly on Inst because Inst alone doesn't
// know its operand types — and we want to print result types on defs.
// ---------------------------------------------------------------------------

/// Adapter that lets a `Function` be printed via `{}`. Holds a reference
/// so it doesn't take ownership.
pub struct FunctionPrinter<'a> {
    func: &'a Function,
}

impl<'a> FunctionPrinter<'a> {
    pub fn new(func: &'a Function) -> Self {
        FunctionPrinter { func }
    }
}

impl<'a> Display for FunctionPrinter<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let func = self.func;

        // Signature line: `fn name(p0: t0, p1: t1, ...) -> ret {`
        write!(f, "fn {}(", func.name)?;
        let entry = func.block(func.entry);
        for (i, &p) in entry.params.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}: {}", p, func.val_ty(p))?;
        }
        f.write_str(")")?;

        if !func.ret_tys.is_empty() {
            f.write_str(" : [")?;
            for (i, v) in func.ret_tys.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{}", v)?;
            }
            f.write_str("]")?;
        }

        f.write_str(" {\n")?;
        // Blocks. The entry block is printed first; the rest in id order.
        // Since blocks are stored in id order and entry is always block0
        // by construction, a simple iteration suffices. If that invariant
        // ever changes, swap this for an explicit reachable-blocks walk.
        for (i, block) in func.blocks.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            print_block(f, func, block)?;
        }
        f.write_str("}\n")
    }
}

fn print_block(f: &mut Formatter<'_>, func: &Function, block: &BasicBlock) -> fmt::Result {
    // Header: `  blockN(p0: t0, ...):`
    write!(f, "  {}", block.id)?;
    f.write_str("(")?;
    for (i, &p) in block.params.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}: {}", p, func.val_ty(p))?;
    }
    f.write_str("):\n")?;

    // Instructions, indented further.
    for inst in &block.insts {
        f.write_str("    ")?;
        print_inst(f, func, inst)?;
        f.write_str("\n")?;
    }

    // Terminator.
    f.write_str("    ")?;
    print_terminator(f, &block.term)?;
    f.write_str("\n")
}

fn print_inst(f: &mut Formatter<'_>, func: &Function, inst: &Inst) -> fmt::Result {
    let dst = inst.def();
    let dst_ty = func.val_ty(dst);

    match inst {
        Inst::Const { val, .. } => {
            write!(f, "{} = const {}: {}", dst, val, dst_ty)
        }
        Inst::Binary { op, lhs, rhs, .. } => {
            // Result type of comparisons is always Bool, of arithmetic is
            // the operand type. Either way we annotate dst's type for
            // unambiguous reading.
            write!(f, "{} = {} {}, {}: {}", dst, op, lhs, rhs, dst_ty)
        }
        Inst::Unary { op, src, .. } => {
            write!(f, "{} = {} {}: {}", dst, op, src, dst_ty)
        }
        Inst::Copy { src, .. } => {
            write!(f, "{} = copy {}: {}", dst, src, dst_ty)
        }
    }
}

fn print_terminator(f: &mut Formatter<'_>, term: &Term) -> fmt::Result {
    match term {
        Term::Jump { tgt, args } => {
            write!(f, "jump {}", tgt)?;
            print_args(f, args)
        }
        Term::Branch {
            cond,
            then_blk,
            then_args,
            else_blk,
            else_args,
        } => {
            write!(f, "branch {}, {}", cond, then_blk)?;
            print_args(f, then_args)?;
            write!(f, ", {}", else_blk)?;
            print_args(f, else_args)
        }
        Term::Return { vals } => print_return(f, vals),
        Term::Unreachable => f.write_str("unreachable"),
    }
}

fn print_args(f: &mut Formatter<'_>, args: &[Val]) -> fmt::Result {
    f.write_str("(")?;
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", a)?;
    }
    f.write_str(")")
}

fn print_return(f: &mut Formatter<'_>, vals: &[Val]) -> fmt::Result {
    f.write_str("return [")?;
    for (i, v) in vals.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", v)?;
    }
    f.write_str("]")
}

// ---------------------------------------------------------------------------
// Convenience: Function::display() and Display impl forwarding through it.
// ---------------------------------------------------------------------------

impl Function {
    /// Return a `Display`-able view of this function.
    ///
    /// Use as: `println!("{}", func.display());`
    pub fn display(&self) -> FunctionPrinter<'_> {
        FunctionPrinter::new(self)
    }
}

impl Display for Function {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        FunctionPrinter::new(self).fmt(f)
    }
}

// ---------------------------------------------------------------------------
// to_string helper that doesn't require a Formatter — useful for tests
// that want to compare against a literal string.
// ---------------------------------------------------------------------------

impl Function {
    /// Render the function to a `String`. Equivalent to `format!("{}", self)`
    /// but avoids the import boilerplate at call sites.
    pub fn to_pretty_string(&self) -> String {
        let mut s = String::new();
        write!(&mut s, "{}", self).expect("writing to String never fails");
        s
    }
}
