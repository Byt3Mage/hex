use crate::{
    VMResult, Value,
    instruction::{Instruction, Reg},
};

/// Host function type for calling external functions from the VM
pub type HostFn = fn(&mut [Value]) -> VMResult<()>;

/// Global function ID in the linked program
pub type FunctionId = u16;

/// Fully compiled program ready for execution
pub struct Program {
    pub instructions: Box<[Instruction]>,
    pub constants: Box<[Value]>,
    pub functions: Box<[Function]>,
}

#[derive(Debug, Clone, Copy)]
pub struct Function {
    /// Function callable type
    pub callable: Callable,
    /// Number of arguments the function expects
    pub narg: Reg,
    /// Number of values the function returns
    pub nret: Reg,
    /// Number of registers the function uses
    pub nreg: Reg,
}

#[derive(Debug, Clone, Copy)]
pub enum Callable {
    Vm(usize),
    Host(HostFn),
}
