//! Liveness analysis for SSA MIR.
//!
//! Computes which `Val`s are live at every block boundary and after every
//! instruction. A `Val` is "live at point P" if it has been defined and
//! will be used on some path from P before any redefinition. In SSA there
//! are no redefinitions, so liveness is "defined and will be used."
//!
//! Algorithm: iterative backward dataflow to fixpoint, with the block-
//! parameter wrinkle: parameters of a successor are *not* live-out of the
//! predecessor — instead, the corresponding jump-arguments are.
//!
//! Output: `Liveness` containing per-block `live_in`/`live_out` sets and
//! per-instruction `live_after` sets. The per-instruction sets are what
//! register allocation reads.

use super::{BlockId, Function, Term, Val};

/// Fixed-capacity bitset over `Val` indices. Allocated once with the number
/// of `Val`s in the function and never resized.
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct ValSet {
    /// One bit per Val. Stored as u64 words.
    words: Vec<u64>,
    /// Number of bits (i.e. number of Vals in the function).
    nbits: usize,
}

impl ValSet {
    pub fn with_capacity(nbits: usize) -> Self {
        let nwords = (nbits + 63) / 64;
        ValSet {
            words: vec![0u64; nwords],
            nbits,
        }
    }

    fn word_bit(idx: usize) -> (usize, u64) {
        (idx / 64, 1u64 << (idx % 64))
    }

    pub fn insert(&mut self, v: Val) -> bool {
        let i = v.idx();
        debug_assert!(i < self.nbits, "Val idx out of bounds");
        let (w, b) = Self::word_bit(i);
        let was = (self.words[w] & b) != 0;
        self.words[w] |= b;
        !was
    }

    pub fn remove(&mut self, v: Val) -> bool {
        let i = v.idx();
        debug_assert!(i < self.nbits, "Val idx out of bounds");
        let (w, b) = Self::word_bit(i);
        let was = (self.words[w] & b) != 0;
        self.words[w] &= !b;
        was
    }

    pub fn contains(&self, v: Val) -> bool {
        let i = v.idx();
        debug_assert!(i < self.nbits, "Val idx out of bounds");
        let (w, b) = Self::word_bit(i);
        (self.words[w] & b) != 0
    }

    /// `self |= other`. Returns true if any bit changed.
    pub fn union_with(&mut self, other: &ValSet) -> bool {
        debug_assert_eq!(self.words.len(), other.words.len());
        let mut changed = false;
        for (a, b) in self.words.iter_mut().zip(other.words.iter()) {
            let new = *a | *b;
            if new != *a {
                changed = true;
                *a = new;
            }
        }
        changed
    }

    /// Iterate set bits in ascending order.
    pub fn iter(&self) -> ValSetIter<'_> {
        ValSetIter {
            words: &self.words,
            word_idx: 0,
            current: self.words.first().copied().unwrap_or(0),
        }
    }
}

pub struct ValSetIter<'a> {
    words: &'a [u64],
    word_idx: usize,
    current: u64,
}

impl<'a> Iterator for ValSetIter<'a> {
    type Item = Val;

    fn next(&mut self) -> Option<Val> {
        loop {
            if self.current != 0 {
                let bit = self.current.trailing_zeros() as usize;
                // Clear the lowest set bit.
                self.current &= self.current - 1;
                let idx = self.word_idx * 64 + bit;
                return Some(val_from_idx(idx));
            }
            self.word_idx += 1;
            if self.word_idx >= self.words.len() {
                return None;
            }
            self.current = self.words[self.word_idx];
        }
    }
}

/// Helper: reconstruct a `Val` from a raw idx. Wraps the same logic
/// the builder uses; defined here to keep the bitset module self-contained.
fn val_from_idx(i: usize) -> Val {
    use std::num::NonZeroU32;
    let n = u32::try_from(i + 1).expect("Val idx overflow");
    // SAFETY: i+1 >= 1, so n is non-zero.
    let nz = NonZeroU32::new(n).expect("non-zero");
    // We need a way to construct Val. Add a pub(super) constructor in the
    // mir module, or use transmute-equivalent via the struct field. The
    // cleanest path is exposing a from_idx_unchecked like we did for
    // BlockId. See the note below the module.
    Val::from_idx_unchecked(nz)
}

/// Liveness analysis result. Indexed by block (per-block sets) and by
/// (block, instruction-index) (per-instruction sets).
pub struct Liveness {
    /// `live_in[block_index]`: Vals live at the entry of the block,
    /// not counting the block's own parameters (those are defined here,
    /// not live-in from elsewhere).
    pub live_in: Vec<ValSet>,
    /// `live_out[block_index]`: Vals live at the exit of the block,
    /// just before the terminator's successor edges are taken.
    pub live_out: Vec<ValSet>,
    /// `live_after[block_index][inst_index]`: Vals live just after the i-th
    /// instruction of the block executes. The set just after the
    /// terminator is `live_out` of one of the successors and is not
    /// stored here.
    pub live_after: Vec<Vec<ValSet>>,
}

impl Liveness {
    /// Compute liveness for `func`.
    pub fn compute(func: &Function) -> Self {
        let nblks = func.blocks.len();
        let nvals = func.vals.len();

        // Step 1: compute use[B] and def[B] for every block.
        //
        // use[B]: Vals read in B before being defined in B.
        // def[B]: Vals defined in B (block params + inst destinations).

        let mut use_set: Vec<ValSet> = (0..nblks).map(|_| ValSet::with_capacity(nvals)).collect();
        let mut def_set: Vec<ValSet> = (0..nblks).map(|_| ValSet::with_capacity(nvals)).collect();

        for block in &func.blocks {
            let bi = block.id.idx();

            // Block params are defined at block entry.
            for &v in &block.params {
                def_set[bi].insert(v);
            }

            // Walk instructions: a use counts as a use[B]
            // if it isn't already defined in B by an earlier instruction.
            for inst in &block.insts {
                for &v in inst.uses() {
                    if !def_set[bi].contains(v) {
                        use_set[bi].insert(v);
                    }
                }
                def_set[bi].insert(inst.def());
            }

            // Terminator uses count too.
            for v in terminator_uses(&block.term) {
                if !def_set[bi].contains(v) {
                    use_set[bi].insert(v);
                }
            }
        }

        // Step 2: iterate to fixpoint.

        let mut live_in: Vec<ValSet> = (0..nblks).map(|_| ValSet::with_capacity(nvals)).collect();
        let mut live_out: Vec<ValSet> = (0..nblks).map(|_| ValSet::with_capacity(nvals)).collect();

        // Process in post-order so successors are usually
        // already updated when we reach a predecessor.
        let post_order = compute_post_order(func);

        let mut changed = true;
        while changed {
            changed = false;
            for &b in &post_order {
                let bi = b.idx();

                // live_out[B] = union over successors S of:
                //   (live_in[S] \ S.params) ∪ args_from_B_to_S
                let mut new_out = ValSet::with_capacity(nvals);
                let block = func.block(b);
                for s in block.term.successors() {
                    let si = s.idx();
                    let succ = func.block(s);

                    // Start from live_in[S] minus S's params.
                    let mut contribution = live_in[si].clone();
                    for &p in &succ.params {
                        contribution.remove(p);
                    }

                    // Add the args this edge passes to S.
                    if let Some(args) = block.term.successor_args(s) {
                        for &v in args {
                            contribution.insert(v);
                        }
                    }

                    new_out.union_with(&contribution);
                }

                if new_out != live_out[bi] {
                    live_out[bi] = new_out;
                    changed = true;
                }

                // live_in[B] = use[B] ∪ (live_out[B] \ def[B])
                let mut new_in = use_set[bi].clone();
                let mut tail = live_out[bi].clone();
                // tail = live_out \ def
                subtract(&mut tail, &def_set[bi]);
                new_in.union_with(&tail);

                if new_in != live_in[bi] {
                    live_in[bi] = new_in;
                    changed = true;
                }
            }
        }

        // Step 3: per-instruction liveness via backward sweep.

        let mut live_after: Vec<Vec<ValSet>> = Vec::with_capacity(nblks);
        for block in &func.blocks {
            let bi = block.id.idx();
            let mut per_inst: Vec<ValSet> = vec![ValSet::with_capacity(nvals); block.insts.len()];

            // Start from the live set just after the last instruction. That
            // set equals live_out plus any Vals used by the terminator that
            // aren't otherwise live-out (e.g. a value used only in the
            // terminator's branch condition or jump-args is live up to and
            // including the moment after the last instruction).
            let mut live = live_out[bi].clone();
            for v in terminator_uses(&block.term) {
                live.insert(v);
            }

            for (i, inst) in block.insts.iter().enumerate().rev() {
                per_inst[i] = live.clone();
                live.remove(inst.def());
                for &v in inst.uses() {
                    live.insert(v);
                }
            }

            // `live` now holds the set live at block entry, *including*
            // block params (since they're defined at entry, anything live
            // here that's a param of B is the same as B's params being
            // immediately needed). We don't store this; live_in[B] already
            // captures it (excluding params).

            live_after.push(per_inst);
        }

        Liveness {
            live_in,
            live_out,
            live_after,
        }
    }
}

/// Subtract `other` from `target` in place: `target &= !other`.
fn subtract(target: &mut ValSet, other: &ValSet) {
    debug_assert_eq!(target.words.len(), other.words.len());
    for (t, o) in target.words.iter_mut().zip(other.words.iter()) {
        *t &= !*o;
    }
}

/// Yield the operand `ValRef`s of a terminator.
fn terminator_uses(term: &Term) -> Vec<Val> {
    // Tiny helper; allocates a Vec but terminators are visited rarely
    // relative to instructions. If this shows up in profiles, hand-roll
    // an iterator like InstUses.
    match term {
        Term::Jump { args, .. } => args.clone(),
        Term::Branch {
            cond,
            then_args,
            else_args,
            ..
        } => {
            let mut v = Vec::with_capacity(1 + then_args.len() + else_args.len());
            v.push(*cond);
            v.extend_from_slice(then_args);
            v.extend_from_slice(else_args);
            v
        }
        Term::Return { vals } => vals.clone(),
        Term::Unreachable => vec![],
    }
}

/// Compute post-order of the CFG starting from entry. A node appears
/// after all its successors. We use this to process blocks predecessor-
/// before-successor when iterating backward dataflow.
fn compute_post_order(func: &Function) -> Vec<BlockId> {
    let n = func.blocks.len();
    let mut visited = vec![false; n];
    let mut order = Vec::with_capacity(n);

    struct Frame {
        block: BlockId,
        succs: Vec<BlockId>,
        next: usize,
    }

    visited[func.entry.idx()] = true;
    let mut stack: Vec<Frame> = vec![Frame {
        block: func.entry,
        succs: func.block(func.entry).term.successors().collect(),
        next: 0,
    }];

    while let Some(top) = stack.last_mut() {
        if top.next < top.succs.len() {
            let s = top.succs[top.next];
            top.next += 1;
            if !visited[s.idx()] {
                visited[s.idx()] = true;
                let succs: Vec<BlockId> = func.block(s).term.successors().collect();
                stack.push(Frame {
                    block: s,
                    succs,
                    next: 0,
                });
            }
        } else {
            order.push(top.block);
            stack.pop();
        }
    }

    order
}
