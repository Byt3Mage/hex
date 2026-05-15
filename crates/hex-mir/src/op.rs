/// Binary operations.
///
/// Signedness-sensitive operations have separate signed/unsigned variants
/// (`SDiv`/`UDiv`, `SLt`/`ULt`, etc.) matching the VM's instruction set.
/// Operations that don't care about signedness (`Add`, `Sub`, `Mul`,
/// bitwise ops, equality) have a single variant.
///
/// Float operations are prefixed `F`. The front end picks the right
/// variant based on operand types.
///
/// Short-circuit `&&`/`||` are not represented here; the front end lowers
/// them to control flow. `And`/`Or`/`Xor` on `Bool` are eager bitwise.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum BinOp {
    // Integer arithmetic (signedness-agnostic)
    Add,
    Sub,
    Mul,

    // Integer arithmetic (signed)
    SDiv,
    SRem,

    // Integer arithmetic (unsigned)
    UDiv,
    URem,

    // Bitwise / logical (also used on Bool)
    And,
    Or,
    Xor,
    Shl,
    LShr, // logical shift right (zero-fill)
    AShr, // arithmetic shift right (sign-extend)

    // Integer comparisons (signedness-agnostic)
    Eq,
    Ne,

    // Integer comparisons (signed)
    SLt,
    SLe,
    SGt,
    SGe,

    // Integer comparisons (unsigned)
    ULt,
    ULe,
    UGt,
    UGe,

    // Float arithmetic
    FAdd,
    FSub,
    FMul,
    FDiv,
    FRem,

    // Float comparisons
    FEq,
    FNe,
    FLt,
    FLe,
    FGt,
    FGe,
}

impl BinOp {
    /// Whether this op produces a `Bool` regardless of operand type.
    pub fn is_comparison(self) -> bool {
        use BinOp::*;
        matches!(
            self,
            Eq | Ne
                | SLt
                | SLe
                | SGt
                | SGe
                | ULt
                | ULe
                | UGt
                | UGe
                | FEq
                | FNe
                | FLt
                | FLe
                | FGt
                | FGe
        )
    }
}

/// Unary operations.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum UnOp {
    /// Bitwise NOT on integers.
    Not,
    /// Logical NOT on Bool.
    BNot,
    /// Integer negation (signed). Wraps on overflow per VM semantics.
    INeg,
    /// Float negation.
    FNeg,
}
