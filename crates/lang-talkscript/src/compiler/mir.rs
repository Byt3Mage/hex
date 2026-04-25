use crate::{
    arena::StrSymbol,
    compiler::op::{BinOp, UnOp},
};

/// A value in the register file. Can span one or more register slots
/// depending on its type. Scalars are size 1, aggregates are size N.
/// Values are mutable, i.e. they can be stored to multiple times.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Value(pub u32);

/// Basic block identifier.
pub type BlockId = usize;

/// Field index within a struct/union.
pub type FieldIdx = usize;

/// Function identifier (indexes into module-level function table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FuncId(pub u32);

/// Type identifier (indexes into module-level type table).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypedValue {
    value: Value,
    type_: TypeId,
}

pub struct ValueAllocator {
    next_value: u32,
}

impl ValueAllocator {
    pub fn new() -> Self {
        Self { next_value: 0 }
    }

    pub fn alloc(&mut self) -> Value {
        let val = self.next_value;
        self.next_value += 1;
        Value(val)
    }
}
/// Scalar types. Used to select the correct
/// VM instruction (e.g. int_add vs float_add)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarType {
    Int,
    UInt,
    Float,
    Bool,
    Pointer,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Int(i64),
    Uint(u64),
    Float(f64),
    Bool(bool),
    Char(char),
    Str(StrSymbol),
}

#[derive(Clone, Debug)]
pub enum Inst {
    /// Copy N (= size(ty)) register slots from src to dst.
    /// Both must have the same type.
    Copy { dst: Value, src: Value, ty: TypeId },

    /// dst = literal
    Const { dst: Value, val: Literal },

    /// dst = lhs (op) rhs
    BinOp {
        dst: Value,
        lhs: Value,
        rhs: Value,
        op: BinOp,
        ty: ScalarType,
    },

    /// dst = (op) src
    UnOp {
        dst: Value,
        src: Value,
        op: UnOp,
        ty: ScalarType,
    },

    /// dst = cast src from `from` to `to`
    Cast {
        dst: Value,
        src: Value,
        from: ScalarType,
        to: ScalarType,
    },

    // ── Memory ─────────────────
    /// Allocate register slots for a value of the given type.
    /// For scalars this is 1 slot, for aggregates it's N slots.
    RegAlloc { dst: Value, ty: TypeId },

    /// dst = base + field offset (compile-time constant).
    /// Result is a Value pointing into the interior of an aggregate.
    FieldAddr {
        dst: Value,
        base: Value,
        field: FieldIdx,
        base_ty: TypeId,
    },

    // ── Tagged union operations ─────────────
    /// Write the field tag into a union value.
    SetTag { dst: Value, field: FieldIdx },

    /// Read the field tag from a union value.
    GetTag { dst: Value, src: Value },

    // ── Calls ───────────────────────────────
    /// Direct function call.
    Call {
        dst: Value,
        func: FuncId,
        args: Vec<Value>,
    },

    /// Indirect call through a function pointer.
    CallIndirect {
        dst: Value,
        func_ptr: Value,
        args: Vec<Value>,
    },

    /// Void direct call.
    CallVoid { func: FuncId, args: Vec<Value> },

    /// Void indirect call.
    CallIndirectVoid { func_ptr: Value, args: Vec<Value> },

    // ── Misc ────────────────────────────────
    /// Get a function pointer as a value.
    FuncAddr { dst: Value, func: FuncId },
}

// ──────────────────────────────────────────────
// Terminators
// ──────────────────────────────────────────────

#[derive(Clone, Debug)]
pub enum Terminator {
    /// Unconditional jump.
    Jump { target: BlockId, args: Vec<Value> },

    /// Conditional branch on a boolean value.
    BranchIf {
        cond: Value,
        then_target: BlockId,
        then_args: Vec<Value>,
        else_target: BlockId,
        else_args: Vec<Value>,
    },

    /// Multi-way branch (for match on tagged unions or integer switch).
    Switch {
        scrutinee: Value,
        cases: Vec<(FieldIdx, BlockId, Vec<Value>)>,
        default: Option<(BlockId, Vec<Value>)>,
    },

    /// Return a value (any size) or void.
    Return { value: Option<Value> },

    /// Unreachable code.
    Unreachable,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub params: Vec<Value>,
    pub insts: Vec<Inst>,
    pub terminator: Terminator,
}

#[derive(Clone, Debug)]
pub struct Function {
    pub id: FuncId,
    pub name: String,
    pub params: Vec<Value>,
    pub param_tys: Vec<TypeId>,
    pub return_ty: Option<TypeId>,
    pub blocks: Vec<Block>,
    pub next_value: u32,
}

#[derive(Clone, Debug)]
pub struct FieldLayout {
    pub name: String,
    pub ty: TypeId,
    pub offset: usize, // register offset from struct base
}

#[derive(Clone, Debug)]
pub struct StructLayout {
    pub name: String,
    pub fields: Vec<FieldLayout>,
    pub size: usize, // total register slots (sum of field sizes)
}

#[derive(Clone, Debug)]
pub struct ArrayLayout {
    pub elem_ty: TypeId,
    pub length: usize,
    pub elem_size: usize,
    pub size: usize, // total register slots (elem_size * length).
}

#[derive(Clone, Debug)]
pub struct UnionLayout {
    pub name: String,
    pub fields: Vec<FieldLayout>,
    pub size: usize, // total register slots 1 (tag) + (max field size)
}

#[derive(Clone, Debug)]
pub enum TypeInfo {
    Scalar(ScalarType),
    Struct(StructLayout),
    Array(ArrayLayout),
    Union(UnionLayout),
    FuncPtr(Vec<TypeId>, Option<TypeId>),
}

#[derive(Clone, Debug)]
pub struct TypeTable {
    pub types: Vec<TypeInfo>,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub functions: Vec<Function>,
    pub types: TypeTable,
}
