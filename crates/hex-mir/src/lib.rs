use std::num::NonZeroU32;

pub(crate) use hex_vm as vm;

pub mod codegen;
mod constants;
mod dominator;
pub mod fmt;
pub mod instruction;
mod liveness;
pub mod op;
mod register_alloc;

pub use instruction::*;
pub use op::*;

pub type RegTy = hex_vm::Reg;

/// SSA value. Defined exactly once, by an instruction or a block parameter.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Val(NonZeroU32);

impl Val {
    fn from_idx(i: usize) -> Self {
        let n = u32::try_from(i + 1).expect("Val idx overflow");
        Self(NonZeroU32::new(n).unwrap())
    }

    fn from_idx_unchecked(n: std::num::NonZeroU32) -> Self {
        Self(n)
    }

    pub fn idx(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

/// Basic block identifier.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct BlockId(NonZeroU32);

impl BlockId {
    fn from_idx(i: usize) -> Self {
        let n = u32::try_from(i + 1).expect("BlockId idx overflow");
        Self(NonZeroU32::new(n).unwrap())
    }

    fn from_idx_unchecked(i: usize) -> Self {
        let n = u32::try_from(i + 1).expect("BlockId idx overflow");
        BlockId(NonZeroU32::new(n).unwrap())
    }

    pub fn idx(self) -> usize {
        (self.0.get() - 1) as usize
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Ty {
    Int,
    Uint,
    Bool,
    Float,
}

#[derive(Copy, Clone, Debug)]
pub enum ConstVal {
    Int(i64),
    Uint(u64),
    Bool(bool),
    Float(f64),
}

impl ConstVal {
    pub fn ty(self) -> Ty {
        match self {
            ConstVal::Int(_) => Ty::Int,
            ConstVal::Uint(_) => Ty::Uint,
            ConstVal::Bool(_) => Ty::Bool,
            ConstVal::Float(_) => Ty::Float,
        }
    }
}

pub struct ValInfo {
    pub ty: Ty,
}

pub struct BasicBlock {
    pub id: BlockId,
    pub params: Vec<Val>,
    pub insts: Vec<Inst>,
    pub term: Term,
}

pub struct Function {
    pub name: String,
    pub ret_tys: Vec<Ty>,
    pub blocks: Vec<BasicBlock>,
    pub vals: Vec<ValInfo>,
    pub entry: BlockId,
}

impl Function {
    pub fn block(&self, id: BlockId) -> &BasicBlock {
        &self.blocks[id.idx()]
    }

    pub fn block_mut(&mut self, id: BlockId) -> &mut BasicBlock {
        &mut self.blocks[id.idx()]
    }

    pub fn val_ty(&self, v: Val) -> Ty {
        self.vals[v.idx()].ty
    }

    /// Iterate over all blocks in id order.
    pub fn iter_blocks(&self) -> impl Iterator<Item = &BasicBlock> {
        self.blocks.iter()
    }

    /// Compute predecessors for every block. Recomputed on demand
    /// rather than stored, so CFG edits don't have to maintain it.
    pub fn predecessors(&self) -> Predecessors {
        let mut preds: Vec<Vec<BlockId>> = vec![Vec::new(); self.blocks.len()];
        for block in &self.blocks {
            for succ in block.term.successors() {
                preds[succ.idx()].push(block.id);
            }
        }
        Predecessors { preds }
    }
}

pub struct Predecessors {
    preds: Vec<Vec<BlockId>>,
}

impl Predecessors {
    pub fn of(&self, b: BlockId) -> &[BlockId] {
        &self.preds[b.idx()]
    }
}

pub struct FunctionBuilder {
    func: Function,
    current: Option<BlockId>,
}

impl FunctionBuilder {
    /// Start building a new function. Creates the entry block but does
    /// not switch to it; call `switch_to(entry)` to begin emitting.
    pub fn new(name: impl Into<String>, ret_tys: Vec<Ty>) -> Self {
        let entry = BlockId::from_idx(0);
        FunctionBuilder {
            func: Function {
                name: name.into(),
                ret_tys,
                blocks: vec![BasicBlock {
                    id: entry,
                    params: Vec::new(),
                    insts: Vec::new(),
                    term: Term::Unreachable,
                }],
                vals: vec![],
                entry,
            },
            current: None,
        }
    }

    pub fn entry(&self) -> BlockId {
        self.func.entry
    }

    pub fn is_terminated(&self) -> bool {
        self.current.is_none()
    }

    /// Allocate a fresh block. The block starts with `Terminator::Unreachable`
    /// as a placeholder; you must overwrite it via `set_terminator` before
    /// finishing.
    pub fn new_block(&mut self) -> BlockId {
        let id = BlockId::from_idx(self.func.blocks.len());
        self.func.blocks.push(BasicBlock {
            id,
            params: Vec::new(),
            insts: Vec::new(),
            term: Term::Unreachable,
        });
        id
    }

    /// Allocate a fresh SSA value with the given type. Not yet attached
    /// to any definition site — the caller is responsible for ensuring
    /// the value is defined exactly once (by an instruction or block param).
    pub fn new_val(&mut self, ty: Ty) -> Val {
        let v = Val::from_idx(self.func.vals.len());
        self.func.vals.push(ValInfo { ty });
        v
    }

    /// Add a parameter of the given type to a block, returning its `Val`.
    pub fn add_param(&mut self, block: BlockId, ty: Ty) -> Val {
        let v = self.new_val(ty);
        self.func.blocks[block.idx()].params.push(v);
        v
    }

    /// Switch the cursor to a block. Subsequent `emit` and `set_terminator`
    /// calls operate on this block.
    pub fn switch_to(&mut self, block: BlockId) {
        self.current = Some(block);
    }

    /// Append an instruction to the current block.
    ///
    /// Panics if no block is current (e.g. after `set_terminator` without
    /// a subsequent `switch_to`).
    pub fn emit(&mut self, inst: Inst) {
        let b = self
            .current
            .expect("FunctionBuilder::emit with no current block");
        self.func.blocks[b.idx()].insts.push(inst);
    }

    /// Set the terminator of the current block and clear the cursor.
    /// You must `switch_to` another block before emitting again.
    pub fn set_term(&mut self, term: Term) {
        let b = self
            .current
            .expect("FunctionBuilder::set_terminator with no current block");
        self.func.blocks[b.idx()].term = term;
        self.current = None;
    }

    /// Finish construction and return the built function.
    ///
    /// Panics if any block still has `Terminator::Unreachable` as a
    /// placeholder when the caller didn't intend that — actually, we
    /// can't tell intent, so we don't check here. Run `validate` for
    /// real diagnostics.
    pub fn finish(self) -> Function {
        self.func
    }
}

pub struct Module {
    pub name: String,
    pub functions: Vec<Function>,
}
