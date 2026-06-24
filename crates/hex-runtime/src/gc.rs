use hex_vm::{Trap, VM, VMState};

use crate::{
    ABORT_OOM, Runtime,
    allocator::{Allocator, DANGLING_HANDLE, Handle},
    gc::{
        page::{PageManager, SizeClass},
        shape::{DYN_LEN_OFFSET, GcShape, GcShapeId, UNION_PAYLOAD_OFFSET},
    },
    heap::{Heap, HeapBuffer},
    metadata::GcMeta,
};

mod page;
pub mod shape;

pub struct Gc {
    meta: GcMeta,
    pages: PageManager,
    work_list: Vec<Handle>,
    allocated: usize,
    threshold: usize,
}

impl Gc {
    pub fn new(meta: GcMeta, threshold: usize) -> Self {
        Self {
            meta,
            pages: PageManager::new(),
            work_list: Vec::new(),
            allocated: 0,
            threshold,
        }
    }

    #[inline(always)]
    pub fn needs_gc(&self) -> bool {
        self.allocated >= self.threshold
    }

    pub(crate) fn alloc(
        &mut self,
        vm: &mut VM,
        allocator: &mut Allocator,
        size: usize,
        shape: GcShapeId,
    ) -> Result<Handle, Trap> {
        let Some(class) = SizeClass::from_size(size) else {
            return Ok(DANGLING_HANDLE);
        };

        if self.needs_gc() {
            self.collect(vm)?;

            if self.needs_gc() {
                return Err(Trap::Abort(ABORT_OOM));
            }
        }

        let block = self.pages.alloc(&mut vm.heap, allocator, class, shape)?;
        self.allocated += page::block_size(class);
        block.ok_or(Trap::Abort(ABORT_OOM))
    }

    fn collect(&mut self, vm: &mut VM) -> Result<(), Trap> {
        self.work_list.clear();

        let heap = &mut vm.heap;
        let worklist = &mut self.work_list;

        for_each_root(&vm.state, &self.meta, |h| mark_obj(heap, worklist, h))?;
        self.trace(heap)?;
        self.sweep(heap)?;
        Ok(())
    }

    fn trace(&mut self, heap: &mut Heap) -> Result<(), Trap> {
        while let Some(obj) = self.work_list.pop() {
            let shape = page::page(heap, page::base(obj))?.shape_id();
            self.trace_obj(heap, obj, shape)?;
        }
        Ok(())
    }

    fn trace_obj(&mut self, heap: &mut Heap, obj: Handle, obj_shape: GcShapeId) -> Result<(), Trap> {
        match &&self.meta.shapes[obj_shape.idx()] {
            GcShape::Leaf => { /* No handles; nothing to trace */ }

            GcShape::Static { offsets } => {
                for off in offsets {
                    let child = heap.load_value(obj + off)?.get();
                    mark_obj(heap, &mut self.work_list, child)?;
                }
            }

            GcShape::Union { variants } => {
                let tag: u64 = heap.load_value(obj)?.get();
                let vs = variants[tag as usize];
                let payload = obj + UNION_PAYLOAD_OFFSET;
                self.trace_obj(heap, payload, vs)?;
            }

            GcShape::DynArray { elem_shape, elem_stride } => {
                // Header is [buf, len, cap]. Mark buf, then walk len elements
                let buf = heap.load_value(obj)?.get();
                let len = heap.load_value(obj + DYN_LEN_OFFSET)?.get();

                mark_obj(heap, &mut self.work_list, buf)?;

                // Skip element tracing if elements have no handles
                if matches!(&&self.meta.shapes[elem_shape.idx()], GcShape::Leaf) {
                    return Ok(());
                }

                let elem_shape = *elem_shape;
                let elem_stride = *elem_stride;

                for i in 0..len {
                    let elem = buf + (i * elem_stride);
                    self.trace_obj(heap, elem, elem_shape)?;
                }
            }
        }
        Ok(())
    }

    fn sweep(&mut self, heap: &mut Heap) -> Result<(), Trap> {
        self.pages.sweep(heap)
    }
}

#[inline]
fn mark_obj<B: HeapBuffer>(heap: &mut Heap<B>, worklist: &mut Vec<Handle>, obj: Handle) -> Result<(), Trap> {
    if obj != DANGLING_HANDLE {
        let page = page::base(obj);
        let slot = page::page(heap, page)?.slot_of(obj, page);
        if page::mark::try_mark(heap, page, slot)? {
            worklist.push(obj);
        }
    }
    Ok(())
}

/// Walk the live VM state and yield every root handle to `f`.
///
/// Roots come from:
///   - the innermost frame (currently executing function)
///   - every saved frame in the call stack
///
/// Globals, if any, would be added by the runtime separately.
pub fn for_each_root<F>(state: &VMState, metadata: &GcMeta, mut f: F) -> Result<(), Trap>
where
    F: FnMut(Handle) -> Result<(), Trap>,
{
    // Scan current VM function
    scan_frame(state, metadata, state.pc(), state.base(), &mut f)?;

    // Scan entire callstack
    for frame in state.call_stack() {
        scan_frame(state, metadata, frame.ret_pc, frame.ret_base, &mut f)?;
    }

    Ok(())
}

fn scan_frame<F>(state: &VMState, meta: &GcMeta, pc: usize, base: usize, f: &mut F) -> Result<(), Trap>
where
    F: FnMut(Handle) -> Result<(), Trap>,
{
    let fn_id = meta.pc_index.fn_at(pc);
    let map = meta.fn_safepoints(fn_id).map(pc);
    let regs = state.registers();

    for &reg in map.handle_regs.iter() {
        f(regs[base + reg as usize].get())?;
    }

    Ok(())
}

pub mod host {
    use hex_vm::{HostAction, HostCtx, Trap};

    use crate::{Runtime, allocator::Handle, gc::shape::GcShapeId};

    pub fn alloc(mut ctx: HostCtx) -> Result<HostAction, Trap> {
        let size = ctx.arg(0)?;
        let shape = GcShapeId(ctx.arg(1)?);

        let gc = &mut ctx.rt.gc;
        let handle = gc.alloc(ctx.vm, &mut ctx.rt.allocator, size, shape)?;

        ctx.ret(0, handle)?;
        Ok(HostAction::Continue)
    }

    pub fn collect_gc(ctx: HostCtx) -> Result<HostAction, Trap> {
        ctx.rt.gc.collect(ctx.vm)?;
        Ok(HostAction::Continue)
    }

    pub fn write_barrier(
        _heap: &mut Heap,
        _rt: &mut Runtime,
        _parent: Handle,
        _offset: u32,
        _child: Handle,
    ) -> Result<(), Trap> {
        Ok(())
    }
}
