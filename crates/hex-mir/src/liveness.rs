use std::collections::HashSet;

use crate::{Function, Val};

pub struct Liveness {
    pub live_in: Vec<HashSet<Val>>,
    pub live_out: Vec<HashSet<Val>>,
}

pub fn compute_liveness(func: &Function) -> Liveness {
    let n = func.blocks.len();
    let mut live_in = vec![HashSet::new(); n];
    let mut live_out = vec![HashSet::new(); n];

    let mut changed = true;
    while changed {
        changed = false;

        for b in (0..n).rev() {
            let block = &func.blocks[b];

            // live_out = union of live_in of all successors
            let mut out = HashSet::new();
            block.term.for_each_successor(|succ| {
                out.extend(&live_in[succ]);
            });

            // Walk backward through block to compute live_in
            let mut live = out.clone();

            block.term.for_each_use(|v| {
                live.insert(*v);
            });

            // Instructions in reverse
            for inst in block.insts.iter().rev() {
                inst.for_each_def(|v| {
                    live.remove(v);
                });
                inst.for_each_use(|v| {
                    live.insert(*v);
                });
            }

            if live != live_in[b] {
                live_in[b] = live;
                live_out[b] = out;
                changed = true;
            } else if out != live_out[b] {
                live_out[b] = out;
                changed = true;
            }
        }
    }

    Liveness { live_in, live_out }
}
