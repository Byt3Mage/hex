#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Void,
    Never,
    Bool,
    Int,
    Uint,
    Float,
    Char,
    Ptr { pointee: TypeId, mutable: bool },
    Array { elem: TypeId, len: u64 },
    Tuple(Vec<TypeId>),
    Struct(Vec<TypeId>),
    Union(Vec<TypeId>),
    Enum { backing: TypeId },
    Func { params: Vec<TypeId>, ret: TypeId },
}

pub type Reg = u8;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Const {
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Inst {
    /// dst = const
    LoadConst {
        dst: Reg,
        val: Const,
    },

    /// dst = src
    Copy {
        dst: Reg,
        src: Reg,
    },

    Not {
        dst: Reg,
    },

    INeg {
        dst: Reg,
    },

    FNeg {
        dst: Reg,
    },

    // --- Arithmetic (int) ---
    IAdd {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    ISub {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    IMul {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    IDiv {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    IRem {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Arithmetic (uint) ---
    UAdd {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    USub {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UMul {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UDiv {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    URem {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Arithmetic (float) ---
    FAdd {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FSub {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FMul {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FDiv {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FRem {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Bitwise ---
    BitAnd {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    BitOr {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    BitXor {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    BitNot {
        dst: Reg,
        src: Reg,
    },
    Shl {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Shr {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Comparison ---
    IEq {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    INe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    ILt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    ILe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    IGt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    IGe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UEq {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UNe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    ULt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    ULe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UGt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    UGe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FLt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FLe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FGt {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    FGe {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Logic ---
    And {
        dst: Reg,
        a: Reg,
        b: Reg,
    },
    Or {
        dst: Reg,
        a: Reg,
        b: Reg,
    },

    // --- Aggregates (struct/tuple/array) ---
    /// Build aggregate in dst from consecutive registers starting at src
    BuildAggregate {
        dst: Reg,
        src: Reg,
        len: u32,
    },

    /// dst = base[field]
    GetField {
        dst: Reg,
        base: Reg,
        field: u32,
    },

    /// base[field] = src (mutates in place)
    SetField {
        base: Reg,
        field: u32,
        src: Reg,
    },

    /// dst = base[index]
    GetIndex {
        dst: Reg,
        base: Reg,
        index: Reg,
    },

    /// base[index] = src (mutates in place)
    SetIndex {
        base: Reg,
        index: Reg,
        src: Reg,
    },

    // --- Union ---
    /// Build union: sets tag and payload
    BuildUnion {
        dst: Reg,
        variant: u32,
        src: Reg,
    },

    /// dst = tag of union at base
    GetTag {
        dst: Reg,
        base: Reg,
    },

    /// dst = payload of union at base (assumes tag checked)
    GetVariant {
        dst: Reg,
        base: Reg,
        variant: u32,
    },

    /// Set union: updates tag and payload in place
    SetVariant {
        base: Reg,
        variant: u32,
        src: Reg,
    },

    // --- Memory (heap) ---
    /// dst = alloc(size)
    Alloc {
        dst: Reg,
        size: Reg,
    },

    /// dst = *ptr
    Load {
        dst: Reg,
        ptr: Reg,
    },

    /// *ptr = src
    Store {
        ptr: Reg,
        src: Reg,
    },

    // --- Calls ---
    /// dst = func(args...)
    Call {
        dst: Reg,
        func: FuncId,
    },

    /// dst = func_ptr(args...) - indirect call
    CallIndirect {
        dst: Reg,
        ptr: Reg,
    },

    /// dst = native(args...)
    CallNative {
        dst: Reg,
        func: NativeFuncId,
    },

    /// dst = func_ptr(args...)
    CallNativeIndirect {
        dst: Reg,
        func: NativeFuncId,
    },
}

// ============================================================================
// Terminators
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    /// Return value from register
    Return(Reg),

    /// Return void
    ReturnVoid,

    /// Unconditional jump
    Jump(BlockId),

    /// Conditional branch
    Branch {
        cond: Reg,
        then_blk: BlockId,
        else_blk: BlockId,
    },

    /// Multi-way branch on integer value
    Switch {
        cond: Reg,
        cases: Vec<(i64, BlockId)>,
        default: BlockId,
    },

    /// Unreachable (after never-returning calls, etc.)
    Unreachable,
}

// ============================================================================
// Blocks and Functions
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub usize);

#[derive(Debug, Clone)]
pub struct Block {
    pub insts: Vec<Inst>,
    pub term: Term,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeFuncId(pub u16);

#[derive(Debug, Clone)]
pub struct Func {
    pub name: String,
    pub params: Vec<TypeId>,
    pub ret: TypeId,
    pub nregs: u8,
    pub blocks: Vec<Block>,
}

impl Func {
    pub fn entry(&self) -> BlockId {
        BlockId(0)
    }
}

// ============================================================================
// Module
// ============================================================================

#[derive(Debug, Clone)]
pub struct Module {
    pub types: Vec<Type>,
    pub funcs: Vec<Func>,
    pub native_funcs: Vec<NativeFunc>,
}

#[derive(Debug, Clone)]
pub struct NativeFunc {
    pub name: String,
    pub params: Vec<TypeId>,
    pub ret: TypeId,
}

impl Module {
    pub fn new() -> Self {
        Self {
            types: Vec::new(),
            funcs: Vec::new(),
            native_funcs: Vec::new(),
        }
    }

    pub fn add_type(&mut self, ty: Type) -> TypeId {
        let id = TypeId(self.types.len() as u64);
        self.types.push(ty);
        id
    }

    pub fn add_func(&mut self, func: Func) -> FuncId {
        let id = FuncId(self.funcs.len() as u16);
        self.funcs.push(func);
        id
    }

    pub fn add_native_func(&mut self, func: NativeFunc) -> NativeFuncId {
        let id = NativeFuncId(self.native_funcs.len() as u16);
        self.native_funcs.push(func);
        id
    }

    pub fn get_type(&self, id: TypeId) -> &Type {
        &self.types[id.0 as usize]
    }

    pub fn get_func(&self, id: FuncId) -> &Func {
        &self.funcs[id.0 as usize]
    }

    pub fn get_func_mut(&mut self, id: FuncId) -> &mut Func {
        &mut self.funcs[id.0 as usize]
    }
}
