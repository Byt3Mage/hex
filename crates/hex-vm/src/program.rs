use crate::{
    VMResult, Value,
    instruction::{Instruction, Reg},
};

#[derive(Debug, Clone, Copy)]
pub struct Function {
    /// Entry point in the list of instructions
    pub entry_pc: usize,
    /// Number of registers allocated for this function
    pub nreg: Reg,
    /// Number of argument registers this function expects
    pub narg: Reg,
    /// Number of registers used for return value
    pub nret: Reg,
}

#[derive(Debug, Clone, Copy)]
pub struct HostFunction {
    /// Function implementation
    pub func: fn(&mut [Value]) -> VMResult<()>,
    /// Number of registers this function expects
    pub nreg: Reg,
}

/// Global function ID in the linked program
pub type FunctionPtr = u16;

/// Fully compiled program ready for execution
pub struct Program {
    pub instructions: Box<[Instruction]>,
    pub constants: Box<[Value]>,
    pub functions: Box<[Function]>,
    pub host_functions: Box<[HostFunction]>,
}
