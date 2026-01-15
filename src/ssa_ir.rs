//! Mid-level IR for TalkScript
//!
//! Expression-based, SSA form with block parameters.
//! Designed for optimization, not tied to any backend.

// ============================================================================
// Types
// ============================================================================

/// Type reference into module's type table
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

/// Concrete types (monomorphized, no generics)
pub enum Type {
    // === Primitives ===
    Void,
    Never,
    Bool,
    Char,  // u32 unicode scalar
    Int,   // i64
    Uint,  // u64
    Float, // f64

    // === Pointers ===
    Ptr { pointee: TypeId, mutable: bool },

    // === Optional ===
    Optional(TypeId),

    // === Aggregates (register-allocated) ===
    Array { elem: TypeId, len: u64 },
    Tuple(Vec<TypeId>),
    Struct(StructDef),
    Union(UnionDef), // Tagged union, one field active
    Enum(EnumDef),   // C-style enum (int/uint backing)

    // === Functions ===
    Func { params: Vec<TypeId>, ret: TypeId },
}

pub struct StructDef {
    pub name: Option<String>,
    pub fields: Vec<Field>,
}

pub struct Field {
    pub name: String,
    pub ty: TypeId,
}

pub struct UnionDef {
    pub name: Option<String>,
    pub fields: Vec<Field>, // Only one active at runtime
}

pub struct EnumDef {
    pub name: Option<String>,
    pub base_type: TypeId,
    pub variants: Vec<EnumVariant>,
}

pub struct EnumVariant {
    pub name: String,
    pub value: u64, // Discriminant value
}

// ============================================================================
// Values and Blocks
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

pub struct Block {
    pub params: Vec<(Value, TypeId)>,
    pub insts: Vec<Inst>,
    pub term: Term,
}

// ============================================================================
// Instructions
// ============================================================================

pub struct Inst {
    pub dst: Value,
    pub ty: TypeId,
    pub op: Op,
}

pub enum Op {
    // === Constants ===
    Const(Const),

    // === Arithmetic (int) ===
    IAdd(Value, Value),
    ISub(Value, Value),
    IMul(Value, Value),
    IDiv(Value, Value),
    IRem(Value, Value),
    INeg(Value),

    // === Arithmetic (uint) ===
    UAdd(Value, Value),
    USub(Value, Value),
    UMul(Value, Value),
    UDiv(Value, Value),
    URem(Value, Value),

    // === Arithmetic (float) ===
    FAdd(Value, Value),
    FSub(Value, Value),
    FMul(Value, Value),
    FDiv(Value, Value),
    FRem(Value, Value),
    FNeg(Value),

    // === Bitwise ===
    BitAnd(Value, Value),
    BitOr(Value, Value),
    BitXor(Value, Value),
    BitNot(Value),
    Shl(Value, Value),
    Shr(Value, Value),  // Arithmetic (sign-extend)
    UShr(Value, Value), // Logical (zero-extend)

    // === Comparison ===
    Eq(Value, Value),
    Ne(Value, Value),
    // Signed
    ILt(Value, Value),
    ILe(Value, Value),
    IGt(Value, Value),
    IGe(Value, Value),
    // Unsigned
    ULt(Value, Value),
    ULe(Value, Value),
    UGt(Value, Value),
    UGe(Value, Value),
    // Float
    FLt(Value, Value),
    FLe(Value, Value),
    FGt(Value, Value),
    FGe(Value, Value),

    // === Logic ===
    Not(Value),

    // === Aggregate construction ===
    Array(Vec<Value>),
    Tuple(Vec<Value>),
    Struct(Vec<Value>),
    Union {
        tag: u32,
        value: Value,
    },

    // === Aggregate access ===
    GetField {
        val: Value,
        idx: u32,
    },
    SetField {
        val: Value,
        idx: u32,
        new: Value,
    }, // Returns new aggregate
    GetIndex {
        val: Value,
        idx: Value,
    },
    SetIndex {
        val: Value,
        idx: Value,
        new: Value,
    },

    // === Union ===
    GetTag(Value), // Returns uint discriminant
    GetUnionField {
        val: Value,
        idx: u32,
    }, // Unsafe: must check tag first

    // === Optional ===
    Some(Value),
    None,
    IsSome(Value),
    Unwrap(Value), // Unsafe: must check first

    // === Pointers / Memory ===
    Alloc(TypeId), // Heap allocate, returns @mut T
    Load(Value),   // Deref pointer
    Store {
        ptr: Value,
        val: Value,
    }, // Write through pointer (returns Void)

    // === Casts ===
    IntToUint(Value),
    UintToInt(Value),
    IntToFloat(Value),
    UintToFloat(Value),
    FloatToInt(Value),
    FloatToUint(Value),
    CharToInt(Value),
    IntToChar(Value),

    // === Calls ===
    Call {
        func: FuncId,
        args: Vec<Value>,
    },
    CallIndirect {
        func: Value,
        args: Vec<Value>,
    },
    CallNative {
        func: NativeFuncId,
        args: Vec<Value>,
    },

    // === Async (scaffolding) ===
    Spawn {
        func: FuncId,
        args: Vec<Value>,
    },
    Await(Value),
}

pub enum Const {
    Void,
    Bool(bool),
    Char(char),
    Int(i64),
    Uint(u64),
    Float(f64),
    Func(FuncId),
    Null(TypeId), // Null pointer of specific type
}

// ============================================================================
// Terminators
// ============================================================================

pub enum Term {
    /// Return value from function
    Return(Value),

    /// Unconditional jump
    Jump { target: BlockId, args: Vec<Value> },

    /// Conditional branch
    Branch {
        cond: Value,
        then_blk: BlockId,
        then_args: Vec<Value>,
        else_blk: BlockId,
        else_args: Vec<Value>,
    },

    /// Match on integer/enum discriminant
    Switch {
        val: Value,
        cases: Vec<(i64, BlockId, Vec<Value>)>,
        default: BlockId,
        default_args: Vec<Value>,
    },

    /// Tail call (optional optimization)
    TailCall { func: FuncId, args: Vec<Value> },

    /// Unreachable code (after Never-typed expressions)
    Unreachable,
}

// ============================================================================
// Functions and Module
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeFuncId(pub u32);

pub struct Function {
    pub name: String,
    pub params: Vec<(Value, TypeId)>,
    pub ret: TypeId,
    pub entry: BlockId,
    pub blocks: Vec<Block>, // BlockId(n) indexes into this
}

pub struct NativeFunc {
    pub name: String,
    pub params: Vec<TypeId>,
    pub ret: TypeId,
}

pub struct Module {
    pub types: Vec<Type>,     // TypeId indexes here
    pub funcs: Vec<Function>, // FuncId indexes here
    pub native_funcs: Vec<NativeFunc>,
}
