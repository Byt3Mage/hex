use hex_vm::Value;

/// Index into the GC shape table (which block offsets hold handles).
/// Pure GC metadata; the VM never reads it. A page is locked to one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct GcShapeId(pub u32);

impl GcShapeId {
    pub const fn idx(self) -> usize {
        self.0 as usize
    }
}

pub const VALUE_SIZE: u32 = size_of::<Value>() as u32;

// Union layout: [tag, variant_1 | variant_2 | variant_n]
pub const UNION_TAG_OFFSET: u32 = 0;
pub const UNION_PAYLOAD_OFFSET: u32 = VALUE_SIZE;

// DynArray header layout: [buf, len, cap]
pub const DYN_BUF_OFFSET: u32 = 0;
pub const DYN_LEN_OFFSET: u32 = VALUE_SIZE;
pub const DYN_CAP_OFFSET: u32 = VALUE_SIZE * 2;

pub enum GcShape {
    /// No memory handles. Tracer skips in O(1).
    Leaf,

    /// Fixed handle offsets, compile-time known.
    Static { offsets: Box<[u32]> },

    /// Tag-dependent. Each variant is itself a shape.
    Union { variants: Box<[GcShapeId]> },

    /// Fixed element shape repeated a runtime number of times.
    DynArray {
        elem_shape: GcShapeId,
        elem_stride: u32,
    },
}
