//! Register allocation for SSA MIR.
//!
//! Walks each block, assigns each `Val` a register such that no two
//! simultaneously-live `Val`s share a register. Uses `live_after` from
//! the liveness pass to know when values die.

use std::collections::HashMap;

use crate::{BlockId, Function, RegTy, Val, instruction::Term, liveness::Liveness};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum RegAllocError {
    /// More than 256 simultaneously-live Vals at some point.
    #[error("out of registers at block {0}")]
    OutOfRegisters(BlockId),
    #[error("scratch register allocation failed")]
    ScratchAllocFailed,
}

/// The result of register allocation.
pub struct RegAlloc {
    map: HashMap<Val, RegTy>,
    max_reg: RegTy,
    scratch: RegTy,
    used_scratch: bool,
}

impl RegAlloc {
    /// Look up the register assigned to a `Val`. Panics if unassigned.
    pub fn reg_of(&self, v: Val) -> RegTy {
        match self.map.get(&v) {
            Some(&r) => r,
            None => panic!("Val {v} has no register assignment"),
        }
    }

    pub fn scratch(&mut self) -> Result<RegTy, RegAllocError> {
        if !self.used_scratch {
            self.used_scratch = true;
            self.scratch = self
                .max_reg
                .checked_add(1)
                .ok_or(RegAllocError::ScratchAllocFailed)?;
        }

        Ok(self.scratch)
    }

    pub fn nreg(&self, nret: RegTy) -> RegTy {
        (self.max_reg + 1).max(nret) + if self.used_scratch { 1 } else { 0 }
    }

    pub fn compute(func: &Function, live: &Liveness) -> Result<Self, RegAllocError> {
        let mut map: HashMap<Val, RegTy> = HashMap::new();
        let mut max_reg: RegTy = 0;

        for blk in &func.blocks {
            let bi = blk.id.idx();

            // Step 1: figure out which registers are taken at block entry.
            //
            // Anything live-in must already have been assigned by an
            // earlier block's processing. Mark its register as taken.
            let mut occupied = [false; (RegTy::MAX as usize) + 1];

            for v in live.live_in[bi].iter() {
                if let Some(&r) = map.get(&v) {
                    occupied[r as usize] = true;
                }
            }

            // Step 2: assign registers to this block's parameters.
            for &p in &blk.params {
                if !map.contains_key(&p) {
                    let r = first_free(&occupied).ok_or(RegAllocError::OutOfRegisters(blk.id))?;
                    map.insert(p, r);
                    occupied[r as usize] = true;
                    max_reg = max_reg.max(r);
                }
            }

            // Step 3: walk instructions top to bottom.
            //
            // Before each instruction, free registers of values that just
            // died. A value dies at instruction i if it was alive going in
            // but isn't in live_after[i]. Then assign a register to dst.

            // The "alive going in" set for the first instruction is
            // live_in plus block params. For later instructions, it's the
            // previous instruction's live_after.
            let mut live_before = live.live_in[bi].clone();
            for &p in &blk.params {
                live_before.insert(p);
            }

            for (i, inst) in blk.insts.iter().enumerate() {
                let live_after = &live.live_after[bi][i];

                // Free registers of values that died at this instruction.
                for v in live_before.iter() {
                    if !live_after.contains(v) {
                        if let Some(&r) = map.get(&v) {
                            occupied[r as usize] = false;
                        }
                    }
                }

                // Assign a register to this instruction's destination.
                let dst = inst.def();
                let r = first_free(&occupied).ok_or(RegAllocError::OutOfRegisters(blk.id))?;
                map.insert(dst, r);
                occupied[r as usize] = true;
                max_reg = max_reg.max(r);

                live_before = live_after.clone();
            }
        }

        Ok(Self {
            map,
            max_reg,
            scratch: RegTy::MAX,
            used_scratch: false,
        })
    }
}

/// Return the lowest-numbered free register, or `None` if all 256 are taken.
fn first_free(occupied: &[bool]) -> Option<RegTy> {
    occupied.iter().position(|b| !b).map(|i| i as RegTy)
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Move {
    pub src: RegTy,
    pub dst: RegTy,
}

/// Sequence a set of parallel moves into a list of single moves.
///
/// Inputs:
///   - `moves`: the parallel-move set. Each `to` register must appear at
///     most once (a destination can't have two sources). The same `from`
///     may appear multiple times (one source feeding many destinations).
///   - `scratch`: a register guaranteed not to be used as any `from` or
///     `to` in `moves`. Used to break cycles. May be unused if no cycles.
///
/// Output: a list of single moves to execute in order. Executing them
/// produces the same effect as if all input moves happened simultaneously.
pub fn resolve_parallel_moves(
    moves: &[Move],
    alloc: &mut RegAlloc,
) -> Result<Vec<Move>, RegAllocError> {
    let mut output: Vec<Move> = Vec::new();

    // Filter trivial self-moves (from == to) up front; they do nothing.
    // Also build a working set: map from dst -> src.
    let mut pending: HashMap<RegTy, RegTy> = HashMap::new();

    for m in moves {
        if m.src == m.dst {
            continue;
        }
        debug_assert!(
            !pending.contains_key(&m.dst),
            "destination {} used by two moves",
            m.dst
        );
        pending.insert(m.dst, m.src);
    }

    while !pending.is_empty() {
        // Try to find a "ready" move: one whose destination is not used
        // as anyone else's source. Emitting it can't clobber a value
        // that's still needed.
        let ready = pending
            .iter()
            .find(|(dst, _)| !pending.values().any(|src| src == *dst))
            .map(|(&dst, &src)| (dst, src));

        if let Some((dst, src)) = ready {
            output.push(Move { src, dst });
            pending.remove(&dst);
            continue;
        }

        // No ready move means every remaining destination is also a
        // source — i.e. we're entirely inside cycles. Pick any move,
        // copy its source to scratch, and rewrite any move that reads
        // from that source to read from scratch instead. This breaks
        // exactly one cycle.
        let (_, &cycle_src) = pending.iter().next().expect("non-empty");
        output.push(Move {
            src: cycle_src,
            dst: alloc.scratch()?,
        });

        // Anyone who was reading from cycle_src now reads from scratch.
        for src in pending.values_mut() {
            if *src == cycle_src {
                *src = alloc.scratch()?;
            }
        }

        // The move (cycle_src -> cycle_dst) is now (scratch -> cycle_dst);
        // that update happened in the loop above since cycle_dst's source
        // was cycle_src. The cycle is broken: cycle_dst is no longer the
        // source of anyone (we'll find it ready on the next iteration).
    }

    Ok(output)
}

pub struct EdgeMoves {
    pub on_jump: HashMap<BlockId, Vec<Move>>,
    pub on_branch_then: HashMap<BlockId, Vec<Move>>,
    pub on_branch_else: HashMap<BlockId, Vec<Move>>,
}

impl EdgeMoves {
    pub fn compute(func: &Function, alloc: &mut RegAlloc) -> Result<Self, RegAllocError> {
        let mut on_jump: HashMap<BlockId, Vec<Move>> = HashMap::new();
        let mut on_branch_then: HashMap<BlockId, Vec<Move>> = HashMap::new();
        let mut on_branch_else: HashMap<BlockId, Vec<Move>> = HashMap::new();

        for block in &func.blocks {
            match &block.term {
                Term::Jump { tgt, args } => {
                    let moves = build_edge_moves(func, alloc, *tgt, args)?;
                    if !moves.is_empty() {
                        on_jump.insert(block.id, moves);
                    }
                }
                Term::Branch {
                    then_blk,
                    then_args,
                    else_blk,
                    else_args,
                    ..
                } => {
                    let then_moves = build_edge_moves(func, alloc, *then_blk, then_args)?;
                    if !then_moves.is_empty() {
                        on_branch_then.insert(block.id, then_moves);
                    }
                    let else_moves = build_edge_moves(func, alloc, *else_blk, else_args)?;
                    if !else_moves.is_empty() {
                        on_branch_else.insert(block.id, else_moves);
                    }
                }
                Term::Return { .. } | Term::Unreachable => {}
            }
        }

        Ok(Self {
            on_jump,
            on_branch_then,
            on_branch_else,
        })
    }
}

/// Build the move sequence for one edge from a predecessor to `target`,
/// passing `args` to `target`'s block parameters.
fn build_edge_moves(
    func: &Function,
    alloc: &mut RegAlloc,
    tgt: BlockId,
    args: &[Val],
) -> Result<Vec<Move>, RegAllocError> {
    let tgt_blk = func.block(tgt);
    let params = &tgt_blk.params;

    debug_assert_eq!(args.len(), params.len());

    let moves: Vec<Move> = args
        .iter()
        .zip(params.iter())
        .map(|(&a, &p)| Move {
            src: alloc.reg_of(a),
            dst: alloc.reg_of(p),
        })
        .collect();

    resolve_parallel_moves(&moves, alloc)
}
