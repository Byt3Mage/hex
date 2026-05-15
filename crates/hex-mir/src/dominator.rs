//! Dominator tree computation using the Cooper-Harvey-Kennedy algorithm.
//!
//! Reference: Cooper, Harvey, Kennedy, "A Simple, Fast Dominance Algorithm"
//! (2001). The algorithm is O(N^2) worst case but in practice converges in
//! a small constant number of iterations on reducible CFGs.
//!
//! The result is a `DomTree` idxed by `BlockId`, supporting:
//!   - `idom(b)`: immediate dominator of `b` (or `None` for the entry).
//!   - `dominates(a, b)`: does `a` dominate `b`?
//!   - `pre_order()`: iterate the tree in pre-order (the order chordal
//!     coloring needs).

use super::{BlockId, Function};

/// Compute the reverse post-order of the CFG starting from `entry`.
///
/// Post-order: a node is emitted after all its successors have been visited.
/// Reverse post-order: that order, reversed. RPO has the property that every
/// block appears before all of its successors, except for back-edges.
///
/// This is the canonical traversal order for forward dataflow analyses
/// and for the CHK dominator algorithm.
pub fn reverse_post_order(func: &Function) -> Vec<BlockId> {
    let n = func.blocks.len();
    let mut visited = vec![false; n];
    let mut post_order = Vec::with_capacity(n);

    // Iterative DFS with an explicit stack. Each stack frame tracks how
    // many of its successors have been processed, so we know when to
    // emit the node into post-order.
    struct Frame {
        block: BlockId,
        // Successor list, captured up-front so we don't re-borrow func.
        succs: Vec<BlockId>,
        next: usize,
    }

    let entry = func.entry;
    visited[entry.idx()] = true;
    let mut stack: Vec<Frame> = vec![Frame {
        block: entry,
        succs: func.block(entry).term.successors().collect(),
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
            // All successors processed; emit and pop.
            post_order.push(top.block);
            stack.pop();
        }
    }

    post_order.reverse();
    post_order
}

/// The dominator tree of a function.
///
/// Stored as a flat `idom` array idxed by `BlockId`. The entry block's
/// idom is itself by convention (so the tree is rooted at entry, but
/// `idom_of(entry)` is reported as `None` to callers).
pub struct DomTree {
    /// `idom[b.idx()] = Some(parent)` for every reachable non-entry
    /// block; `Some(entry)` for the entry; `None` for unreachable blocks.
    idom: Vec<Option<BlockId>>,
    /// RPO numbering used during construction; kept around for `dominates`.
    rpo_idx: Vec<Option<u32>>,
    /// The RPO sequence, useful for callers who want to walk it.
    rpo: Vec<BlockId>,
    entry: BlockId,
}

impl DomTree {
    /// Compute the dominator tree of `func`.
    pub fn compute(func: &Function) -> Self {
        let n = func.blocks.len();
        let entry = func.entry;

        let rpo = reverse_post_order(func);
        let mut rpo_idx: Vec<Option<u32>> = vec![None; n];
        for (i, b) in rpo.iter().enumerate() {
            rpo_idx[b.idx()] = Some(i as u32);
        }

        let preds = func.predecessors();

        // idom array: None until set. Entry's idom is itself.
        let mut idom: Vec<Option<BlockId>> = vec![None; n];
        idom[entry.idx()] = Some(entry);

        // Iterate over RPO (excluding entry) until no changes.
        let mut changed = true;
        while changed {
            changed = false;
            for &b in rpo.iter().skip(1) {
                // Pick the first already-processed predecessor as a starting
                // point, then intersect with each other processed predecessor.
                let mut new_idom: Option<BlockId> = None;
                for &p in preds.of(b) {
                    if idom[p.idx()].is_none() {
                        // Predecessor not yet processed in this iteration.
                        continue;
                    }
                    new_idom = Some(match new_idom {
                        None => p,
                        Some(cur) => intersect(p, cur, &idom, &rpo_idx),
                    });
                }

                // If b has no processed predecessors, it's unreachable from
                // entry — leave idom[b] as None.
                if let Some(ni) = new_idom {
                    if idom[b.idx()] != Some(ni) {
                        idom[b.idx()] = Some(ni);
                        changed = true;
                    }
                }
            }
        }

        DomTree {
            idom,
            rpo_idx,
            rpo,
            entry,
        }
    }

    /// The immediate dominator of `b`, or `None` if `b` is the entry
    /// block or unreachable.
    pub fn idom_of(&self, b: BlockId) -> Option<BlockId> {
        let raw = self.idom[b.idx()]?;
        if raw == b {
            // Entry's idom is itself in storage; report None to callers.
            None
        } else {
            Some(raw)
        }
    }

    /// Whether `b` is reachable from the entry block.
    pub fn is_reachable(&self, b: BlockId) -> bool {
        self.idom[b.idx()].is_some()
    }

    /// Does `a` dominate `b`? A block always dominates itself.
    ///
    /// Implemented by walking up the dominator tree from `b`. O(depth).
    pub fn dominates(&self, a: BlockId, b: BlockId) -> bool {
        if !self.is_reachable(a) || !self.is_reachable(b) {
            return false;
        }
        let mut cur = b;
        loop {
            if cur == a {
                return true;
            }
            // Walk to parent. Stop when we reach the entry (whose stored
            // idom is itself).
            let parent = self.idom[cur.idx()].expect("reachable");
            if parent == cur {
                // Reached entry without finding a.
                return a == cur;
            }
            cur = parent;
        }
    }

    /// Iterate blocks in dominator-tree pre-order. This is the order
    /// required by chordal-graph register coloring.
    ///
    /// "Pre-order" here means: each block appears before all blocks it
    /// dominates. RPO is one valid pre-order for the dominator tree
    /// (since `idom(b)` always appears before `b` in RPO), so we reuse it.
    pub fn pre_order(&self) -> impl Iterator<Item = BlockId> + '_ {
        self.rpo.iter().copied().filter(|b| self.is_reachable(*b))
    }

    /// The reverse post-order used during construction. Useful for
    /// callers that want a deterministic block ordering.
    pub fn rpo(&self) -> &[BlockId] {
        &self.rpo
    }

    /// Build a children-list view of the dominator tree. Useful for
    /// recursive walks. idxed by `BlockId`.
    pub fn children(&self) -> DomChildren {
        let n = self.idom.len();
        let mut children: Vec<Vec<BlockId>> = vec![Vec::new(); n];
        for (i, slot) in self.idom.iter().enumerate() {
            if let Some(parent) = slot {
                let child = BlockId::from_idx_unchecked(i);
                if *parent != child {
                    children[parent.idx()].push(child);
                }
            }
        }
        DomChildren { children }
    }
}

/// Walk two blocks up the partially-built dominator tree until they meet.
/// This is the "intersect" routine from CHK. Both `a` and `b` must have
/// `idom` set (i.e. be processed in this or a prior iteration).
///
/// The walk uses RPO numbers as a depth proxy: a higher RPO number means
/// "deeper" (further from the root in the dominator tree, since dominators
/// come earlier in RPO). Always advance the deeper finger.
fn intersect(
    mut a: BlockId,
    mut b: BlockId,
    idom: &[Option<BlockId>],
    rpo_idx: &[Option<u32>],
) -> BlockId {
    while a != b {
        // RPO indices: lower = closer to root.
        let ra = rpo_idx[a.idx()].expect("processed block has rpo idx");
        let rb = rpo_idx[b.idx()].expect("processed block has rpo idx");
        if ra > rb {
            // a is deeper; walk it up.
            a = idom[a.idx()].expect("processed block has idom");
        } else {
            b = idom[b.idx()].expect("processed block has idom");
        }
    }
    a
}

pub struct DomChildren {
    children: Vec<Vec<BlockId>>,
}

impl DomChildren {
    pub fn of(&self, b: BlockId) -> &[BlockId] {
        &self.children[b.idx()]
    }
}
