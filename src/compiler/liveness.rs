use ahash::AHashMap;
use smallvec::SmallVec;

use crate::compiler::mir::{Block, BlockId, Inst, Terminator, Value};

/// A global instruction position in the linearized function.
pub type InstIdx = usize;

/// For each Value, tracks where it's defined and where it's last used.
struct LiveRange {
    defined_at: InstIdx,
    last_use: InstIdx,
}

pub struct LivenessInfo {
    ranges: AHashMap<Value, LiveRange>,
}

impl LivenessInfo {
    pub fn analyze(blocks: &[Block], block_order: &[BlockId]) -> Self {
        let mut ranges: AHashMap<Value, LiveRange> = AHashMap::new();
        let mut idx: InstIdx = 0;

        for &block_id in block_order {
            let block = &blocks[block_id];

            // Block params are defined at the start of the block.
            for &param in &block.params {
                ranges.entry(param).or_insert(LiveRange {
                    defined_at: idx,
                    last_use: idx,
                });
            }

            for inst in &block.insts {
                // Record definitions.
                for def in inst_defs(inst) {
                    ranges.entry(def).or_insert(LiveRange {
                        defined_at: idx,
                        last_use: idx,
                    });
                }

                // Record uses and extend last_use.
                for used in inst_uses(inst) {
                    if let Some(range) = ranges.get_mut(&used) {
                        range.last_use = range.last_use.max(idx);
                    }
                }

                idx += 1;
            }

            // Terminator uses.
            for used in terminator_uses(&block.terminator) {
                if let Some(range) = ranges.get_mut(&used) {
                    range.last_use = range.last_use.max(idx);
                }
            }
            idx += 1; // terminator occupies a slot
        }

        Self { ranges }
    }
}

/// Values defined (written) by an instruction.
fn inst_defs(inst: &Inst) -> SmallVec<[Value; 2]> {
    let mut defs = SmallVec::new();
    match inst {
        Inst::Const { dst, .. }
        | Inst::BinOp { dst: dst, .. }
        | Inst::UnOp { dst: dst, .. }
        | Inst::Cast { dst: dst, .. }
        | Inst::FuncAddr { dst, .. }
        | Inst::RegAlloc { dst, .. }
        | Inst::FieldAddr { dst, .. }
        | Inst::Copy { dst, .. }
        | Inst::SetTag { dst, .. }
        | Inst::GetTag { dst, .. }
        | Inst::VariantPayload { dst, .. }
        | Inst::Call { dst, .. }
        | Inst::CallIndirect { dst, .. } => {
            defs.push(*dst);
        }
        Inst::CallVoid { .. } | Inst::CallIndirectVoid { .. } => {}
    }
    defs
}

/// Values used (read) by an instruction.
fn inst_uses(inst: &Inst) -> SmallVec<[Value; 4]> {
    let mut uses = SmallVec::new();
    match inst {
        Inst::Copy { dst, src, .. } => {
            uses.push(*dst);
            uses.push(*src);
        }
        Inst::Const { .. } => {}
        Inst::BinOp { lhs, rhs, .. } => {
            uses.push(*lhs);
            uses.push(*rhs);
        }
        Inst::UnOp { src, .. } => {
            uses.push(*src);
        }
        Inst::Cast { src, .. } => {
            uses.push(*src);
        }

        Inst::FuncAddr { .. } => {}

        Inst::RegAlloc { .. } => {}

        Inst::FieldAddr { base, .. } => {
            uses.push(*base);
        }

        Inst::SetTag { dst, .. } => {
            uses.push(*dst);
        }
        Inst::GetTag { src, .. } => {
            uses.push(*src);
        }
        Inst::VariantPayload { base, .. } => {
            uses.push(*base);
        }

        Inst::Call { args, .. } => {
            uses.extend_from_slice(args);
        }
        Inst::CallIndirect { func_ptr, args, .. } => {
            uses.push(*func_ptr);
            uses.extend_from_slice(args);
        }
        Inst::CallVoid { args, .. } => {
            uses.extend_from_slice(args);
        }
        Inst::CallIndirectVoid { func_ptr, args, .. } => {
            uses.push(*func_ptr);
            uses.extend_from_slice(args);
        }
    }
    uses
}

/// Values used by a terminator.
fn terminator_uses(term: &Terminator) -> SmallVec<[Value; 4]> {
    let mut uses = SmallVec::new();
    match term {
        Terminator::Jump { args, .. } => {
            uses.extend_from_slice(args);
        }
        Terminator::BranchIf {
            cond,
            then_args,
            else_args,
            ..
        } => {
            uses.push(*cond);
            uses.extend_from_slice(then_args);
            uses.extend_from_slice(else_args);
        }
        Terminator::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            uses.push(*scrutinee);
            for (_, _, args) in cases {
                uses.extend_from_slice(args);
            }
            if let Some((_, args)) = default {
                uses.extend_from_slice(args);
            }
        }
        Terminator::Return { value } => {
            if let Some(v) = value {
                uses.push(*v);
            }
        }
        Terminator::Unreachable => {}
    }
    uses
}
