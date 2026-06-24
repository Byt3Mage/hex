use crate::{Opcode, Reg, Syscode};

#[derive(thiserror::Error, Copy, Clone, Debug)]
pub enum Fault {
    #[error("division by zero")]
    DivisionByZero,
    #[error("memory access out of bounds")]
    MemoryOOB,
    #[error("aborted with code {0}")]
    Abort(u8),
}

#[derive(thiserror::Error, Copy, Clone, Debug)]
pub enum Error {
    #[error("unknown opcode: {0}")]
    UnknownOp(Opcode),
    #[error("unknown syscode: {0}")]
    UnknownSys(Syscode),
    #[error("program counter out of bounds")]
    PcOOB,
    #[error("expected VM function but found host function")]
    FunctionIsHost,
    #[error("argument count mismatch: expected {exp}, got {got}")]
    ArgcMismatch { exp: Reg, got: Reg },
    #[error("argument index '{index}' out of bounds (nargs: {narg})")]
    ArgOutOfBounds { index: Reg, narg: Reg },
    #[error("return value index '{index}' out of bounds (nret: {nret})")]
    RetOutOfBounds { index: Reg, nret: Reg },
}
