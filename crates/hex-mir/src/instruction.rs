use crate::{
    BlockId, ConstVal, Val,
    op::{BinOp, UnOp},
};

#[derive(Clone, Debug)]
pub enum Inst {
    Const {
        dst: Val,
        val: ConstVal,
    },

    Copy {
        dst: Val,
        src: Val,
    },

    Binary {
        dst: Val,
        op: BinOp,
        lhs: Val,
        rhs: Val,
    },
    Unary {
        dst: Val,
        op: UnOp,
        src: Val,
    },
}

impl Inst {
    /// The SSA value this instruction defines.
    pub fn def(&self) -> Val {
        match self {
            Inst::Const { dst, .. }
            | Inst::Binary { dst, .. }
            | Inst::Unary { dst, .. }
            | Inst::Copy { dst, .. } => *dst,
        }
    }

    /// Operands used by this instruction. Returns at most two operands;
    /// the iterator does not allocate.
    pub fn uses(&self) -> InstUses<'_> {
        match self {
            Inst::Const { .. } => InstUses::empty(),
            Inst::Binary { lhs, rhs, .. } => InstUses::two(lhs, rhs),
            Inst::Unary { src, .. } | Inst::Copy { src, .. } => InstUses::one(src),
        }
    }

    /// Whether the instruction has observable side effects beyond
    /// defining `dst`. All current instructions are pure; this exists
    /// as a hook for future memory ops, calls, etc.
    pub fn is_pure(&self) -> bool {
        true
    }
}

/// Iterator over an instruction's operand `ValRef`s. Yields 0, 1, or 2.
pub struct InstUses<'a> {
    slots: [Option<&'a Val>; 2],
    pos: usize,
}

impl<'a> InstUses<'a> {
    fn empty() -> Self {
        InstUses {
            slots: [None, None],
            pos: 0,
        }
    }

    fn one(a: &'a Val) -> Self {
        InstUses {
            slots: [Some(a), None],
            pos: 0,
        }
    }

    fn two(a: &'a Val, b: &'a Val) -> Self {
        InstUses {
            slots: [Some(a), Some(b)],
            pos: 0,
        }
    }
}

impl<'a> Iterator for InstUses<'a> {
    type Item = &'a Val;

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.slots.len() {
            let i = self.pos;
            self.pos += 1;
            if let Some(r) = self.slots[i] {
                return Some(r);
            }
        }
        None
    }
}

#[derive(Clone, Debug)]
pub enum Term {
    Jump {
        tgt: BlockId,
        args: Vec<Val>,
    },
    Branch {
        cond: Val,
        then_blk: BlockId,
        then_args: Vec<Val>,
        else_blk: BlockId,
        else_args: Vec<Val>,
    },
    Return {
        vals: Vec<Val>,
    },

    Unreachable,
}

impl Term {
    /// Iterate over the successor blocks of this terminator.
    pub fn successors(&self) -> Successors<'_> {
        match self {
            Term::Jump { tgt, .. } => Successors::one(*tgt),
            Term::Branch {
                then_blk, else_blk, ..
            } => Successors::two(*then_blk, *else_blk),
            Term::Return { .. } | Term::Unreachable => Successors::empty(),
        }
    }

    /// The arguments passed to a specific successor block on this edge.
    /// Returns `None` if `succ` is not actually a successor.
    ///
    /// Note: if the same block appears as both successors of a `Branch`
    /// (degenerate case), this returns the `then` args.
    pub fn successor_args(&self, succ: BlockId) -> Option<&[Val]> {
        match self {
            Term::Jump { tgt, args } if *tgt == succ => Some(args),
            Term::Branch {
                then_blk,
                then_args,
                ..
            } if *then_blk == succ => Some(then_args),
            Term::Branch {
                else_blk,
                else_args,
                ..
            } if *else_blk == succ => Some(else_args),
            _ => None,
        }
    }
}

/// Iterator over a terminator's successor blocks. Yields 0, 1, or 2.
pub struct Successors<'a> {
    slots: [Option<BlockId>; 2],
    pos: usize,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> Successors<'a> {
    fn empty() -> Self {
        Successors {
            slots: [None, None],
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }

    fn one(a: BlockId) -> Self {
        Successors {
            slots: [Some(a), None],
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }

    fn two(a: BlockId, b: BlockId) -> Self {
        Successors {
            slots: [Some(a), Some(b)],
            pos: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a> Iterator for Successors<'a> {
    type Item = BlockId;

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.slots.len() {
            let i = self.pos;
            self.pos += 1;
            if let Some(b) = self.slots[i] {
                return Some(b);
            }
        }
        None
    }
}
