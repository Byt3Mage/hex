use std::collections::VecDeque;

use crate::{FuncDef, Inst, Term, Val};

/// Tracks value liveness across blocks for register freeing decisions.
///
/// A value V is "live" at a program point if there exists some path
/// from that point to a use of V. We can only free V's register
/// when it is no longer live.
pub struct Liveness {
    /// Per-block, per-value: the index of the last use within that block.
    /// Outer vec indexed by block, inner is (Val id, inst index) pairs,
    /// sorted by val id for binary search.
    last_use_in_block: Vec<Vec<(u32, usize)>>,

    /// Per-value: set of block indices where this value is live-out
    /// (needed by at least one successor).
    /// Stored as a flat bitset per value would be ideal, but for
    /// clarity we use a Vec<bool> per value, indexed by block.
    /// live_out[val_id][block_idx] = true means V is live-out of that block.
    live_out: Vec<Vec<bool>>,
}

impl Liveness {
    pub fn compute(func: &FuncDef) -> Self {
        let num_val = func.next_val as usize;
        let num_blk = func.blocks.len();

        // ---- Step 1: Find definitions and uses ----
        //
        // def_block[val_id] = which block defines this value.
        // For block params, the defining block is the block they belong to.
        // For instructions, the defining block is the block containing them.

        let mut def_blk: Vec<usize> = vec![0; num_val];

        for (blk_idx, blk) in func.blocks.iter().enumerate() {
            for &param in &blk.params {
                def_blk[param.0 as usize] = blk_idx;
            }
            for inst_def in &blk.insts {
                def_blk[inst_def.val.0 as usize] = blk_idx;
            }
        }

        // ---- Step 2: Compute per-block last use for each value ----
        //
        // For each block, scan instructions and the terminator.
        // Record the highest instruction index at which each value is used.
        // The terminator counts as index = block.insts.len().

        let mut last_use_in_block: Vec<Vec<(u32, usize)>> = Vec::with_capacity(num_blk);

        // Temporary map reused per block to avoid allocations.
        // val_id -> last inst index in current block.
        let mut block_uses: Vec<Option<usize>> = vec![None; num_val];

        for blk in &func.blocks {
            // Scan instructions
            for (inst_idx, inst_def) in blk.insts.iter().enumerate() {
                Self::for_each_operand(&inst_def.inst, |val| {
                    block_uses[val.0 as usize] = Some(inst_idx);
                });
            }

            // Scan terminator
            let term_idx = blk.insts.len();
            Self::for_each_term_operand(&blk.term, |val| {
                block_uses[val.0 as usize] = Some(term_idx);
            });

            // Collect into sorted vec and reset
            let mut entries = Vec::new();
            for val_id in 0..num_val {
                if let Some(idx) = block_uses[val_id].take() {
                    entries.push((val_id as u32, idx));
                }
            }
            last_use_in_block.push(entries);
        }

        // ---- Step 3: Compute upward-exposed uses per block ----
        //
        // A value is "upward exposed" in block B if it is used in B
        // and is NOT defined in B. This means B needs the value to
        // already be in a register when B starts executing.
        // These are the values that are live-in to B.

        // live_in[val_id] as a set of block indices.
        // We use a flat vec of bools per value.
        let mut live_in: Vec<Vec<bool>> = vec![vec![false; num_blk]; num_val];

        for (block_idx, uses) in last_use_in_block.iter().enumerate() {
            for &(val_id, _) in uses {
                // If this value is not defined in this block, it's upward exposed
                if def_blk[val_id as usize] != block_idx {
                    live_in[val_id as usize][block_idx] = true;
                }
            }
        }

        // ---- Step 4: Build predecessor map ----

        let mut predecessors: Vec<Vec<usize>> = vec![vec![]; num_blk];
        for (block_idx, block) in func.blocks.iter().enumerate() {
            Self::for_each_successor(&block.term, |succ| {
                predecessors[succ].push(block_idx);
            });
        }

        // ---- Step 5: Propagate liveness backwards ----
        //
        // If value V is live-in to block B, then V is live-out of
        // every predecessor P of B, UNLESS P is where V is defined.
        // And if V is live-out of P and not defined in P, then V
        // is also live-in to P (propagate further).
        //
        // We use a worklist of (val_id, block_idx) pairs meaning
        // "val_id is live-in to block_idx, propagate to predecessors."

        let mut worklist: VecDeque<(u32, usize)> = VecDeque::new();

        // Seed worklist with initial upward-exposed uses
        for val_id in 0..num_val {
            for block_idx in 0..num_blk {
                if live_in[val_id][block_idx] {
                    worklist.push_back((val_id as u32, block_idx));
                }
            }
        }

        // live_out is what we're computing
        let mut live_out: Vec<Vec<bool>> = vec![vec![false; num_blk]; num_val];

        while let Some((val_id, block_idx)) = worklist.pop_front() {
            for &pred in &predecessors[block_idx] {
                // V is live-out of pred (pred flows into block_idx which needs V)
                if live_out[val_id as usize][pred] {
                    // Already processed
                    continue;
                }

                live_out[val_id as usize][pred] = true;

                // If pred does NOT define V, then V is also live-in to pred
                if def_blk[val_id as usize] != pred {
                    if !live_in[val_id as usize][pred] {
                        live_in[val_id as usize][pred] = true;
                        worklist.push_back((val_id, pred));
                    }
                }
            }
        }

        Self {
            last_use_in_block,
            live_out,
        }
    }

    /// Can we free the register for `val` after instruction `inst_idx` in block `block_idx`?
    ///
    /// Yes if:
    /// 1. There is no later use of val in this block
    /// 2. val is not live-out of this block
    pub fn is_last_use(&self, val: Val, block_idx: usize, inst_idx: usize) -> bool {
        let val_id = val.0 as usize;

        // Check for a later use in this same block
        for &(vid, last_idx) in &self.last_use_in_block[block_idx] {
            if vid == val_id as u32 {
                if last_idx > inst_idx {
                    return false;
                }
                break;
            }
        }

        // Check if value is needed by any successor
        !self.live_out[val_id][block_idx]
    }

    pub fn for_each_operand(inst: &Inst, mut f: impl FnMut(Val)) {
        match inst {
            Inst::Const(_, _) => {}
            Inst::BinOp(_, a, b) => {
                f(*a);
                f(*b);
            }
            Inst::UnOp(_, a) | Inst::Conv(_, a) => f(*a),
            Inst::Call(_, args) | Inst::CallNative(_, args) => {
                args.iter().for_each(|&v| f(v));
            }
            Inst::CallIndirect(fv, args) => {
                f(*fv);
                args.iter().for_each(|&v| f(v));
            }
            Inst::Result(call, _) => f(*call),
        }
    }

    pub fn any_inst_operand<R>(inst: &Inst, mut f: impl FnMut(Val) -> Option<R>) -> Option<R> {
        match inst {
            Inst::Const(_, _) => None,
            Inst::BinOp(_, a, b) => f(*a).or_else(|| f(*b)),
            Inst::UnOp(_, a) | Inst::Conv(_, a) => f(*a),
            Inst::Call(_, args) | Inst::CallNative(_, args) => {
                args.iter().copied().find_map(|v| f(v))
            }
            Inst::CallIndirect(fv, args) => {
                f(*fv).or_else(|| args.iter().copied().find_map(|v| f(v)))
            }
            Inst::Result(call, _) => f(*call),
        }
    }

    pub fn for_each_term_operand(term: &Term, mut f: impl FnMut(Val)) {
        match term {
            Term::Br(_, args) => args.iter().for_each(|&v| f(v)),
            Term::BrIf(cond, _, t_args, _, f_args) => {
                f(*cond);
                t_args.iter().for_each(|&v| f(v));
                f_args.iter().for_each(|&v| f(v));
            }
            Term::Ret(vals) => vals.iter().for_each(|&v| f(v)),
        }
    }

    fn for_each_successor(term: &Term, mut f: impl FnMut(usize)) {
        match term {
            Term::Br(b, _) => f(b.0 as usize),
            Term::BrIf(_, t, _, fb, _) => {
                f(t.0 as usize);
                f(fb.0 as usize);
            }
            Term::Ret(_) => {}
        }
    }
}
