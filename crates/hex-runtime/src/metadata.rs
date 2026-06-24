use std::collections::HashMap;

use hex_vm::{FunctionId, HeapBuf, Program, Reg};

use crate::gc::shape::GcShape;

/// Maps any PC to the function it belongs to.
/// Built by sorting functions by their entry_pc.
pub struct PcIndex {
    entries: Vec<(usize, FunctionId)>,
}

impl PcIndex {
    pub fn build<B: HeapBuf, R>(program: &Program<B, R>) -> Self {
        let mut entries: Vec<(usize, FunctionId)> = (0..program.functions().len())
            .filter_map(|i| {
                let f = program.function(i);
                f.ty.entry_pc().ok().map(|pc| (pc, i as FunctionId))
            })
            .collect();
        entries.sort_by_key(|(pc, _)| *pc);
        Self { entries }
    }

    /// Function containing `pc`. Returns the function whose entry_pc
    /// is the largest one <= pc.
    pub fn fn_at(&self, pc: usize) -> FunctionId {
        // partition_point returns the first index where pred is false.
        // We want the last entry with start <= pc, so that's
        // partition_point(start <= pc) - 1.
        let i = self.entries.partition_point(|(s, _)| *s <= pc) - 1;
        self.entries[i].1
    }
}

/// Stack map at one safepoint: the registers that hold handles at
/// this PC. PC here is the PC *after* the call instruction (the
/// return PC), matching the VM's convention.
#[derive(Clone)]
pub struct StackMap {
    pub handle_regs: Box<[Reg]>,
}

/// All safepoints for one function.
#[derive(Default, Clone)]
pub struct Safepoints {
    // Return-PC -> map. Sparse; only call sites have entries.
    pub maps: HashMap<usize, StackMap>,
}

impl Safepoints {
    #[inline]
    pub fn map(&self, pc: usize) -> &StackMap {
        self.maps
            .get(&pc)
            .expect("compiler bug: Frame PC is not a registered safepoint")
    }
}

/// GC metadata, owned by the runtime.
pub struct GcMeta {
    pub shapes: Box<[GcShape]>,
    pub pc_index: PcIndex,
    pub safepoints: HashMap<FunctionId, Safepoints>,
}

impl GcMeta {
    /// Build from a program plus per-function safepoint info supplied
    /// by the compiler. `compiler_safepoints` is keyed by FunctionId.
    pub fn build<B: HeapBuf, R>(
        program: &Program<B, R>,
        shapes: Box<[GcShape]>,
        safepoints: HashMap<FunctionId, Safepoints>,
    ) -> Self {
        Self {
            shapes,
            pc_index: PcIndex::build(program),
            safepoints,
        }
    }

    #[inline]
    pub fn fn_safepoints(&self, fn_id: FunctionId) -> &Safepoints {
        self.safepoints
            .get(&fn_id)
            .expect("compiler bug: Function has no safepoint table")
    }
}
