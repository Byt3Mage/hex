use crate::{
    Error, Value,
    host::Syscode,
    instruction::{Instruction, Reg},
};

/// Global function ID in the linked program
pub type FunctionId = u16;
pub type ConstantId = u16;

#[derive(Debug, Clone, Copy)]
pub enum FnType {
    Hxvm { entry_pc: usize },
    Host { syscode: Syscode },
}

impl FnType {
    pub fn entry_pc(&self) -> Result<usize, Error> {
        match self {
            Self::Hxvm { entry_pc } => Ok(*entry_pc),
            _ => Err(Error::FunctionIsHost),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HandlerEntry {
    pub start_pc: usize,
    pub end_pc: usize,
    pub handler_pc: usize,
    pub catch_reg: Reg,
}

#[derive(Debug, Clone)]
pub struct Function {
    /// Function callable type
    pub ty: FnType,
    /// Number of arguments the function expects
    pub narg: Reg,
    /// Number of values the function returns
    pub nret: Reg,
    /// Number of registers the function uses
    pub nreg: Reg,
    /// Trap handler entries for the function
    pub handlers: Box<[HandlerEntry]>,
}

impl Function {
    #[inline]
    pub fn handler_for(&self, pc: usize) -> Option<&HandlerEntry> {
        // ranges are emitted nested, so the LAST matching entry
        // is the most deeply nested. Iterate reversed to catch inner try.
        self.handlers.iter().rev().find(|h| h.start_pc <= pc && pc < h.end_pc)
    }
}

/// Fully compiled program ready for execution
pub struct Program {
    instructions: Box<[Instruction]>,
    constants: Box<[Value]>,
    functions: Box<[Function]>,
}

impl Program {
    pub fn new(instructions: Box<[Instruction]>, constants: Box<[Value]>, functions: Box<[Function]>) -> Self {
        Self { instructions, constants, functions }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    #[inline(always)]
    pub fn constants(&self) -> &[Value] {
        &self.constants
    }

    #[inline(always)]
    pub fn functions(&self) -> &[Function] {
        &self.functions
    }

    #[inline(always)]
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    #[inline(always)]
    pub fn constant(&self, idx: usize) -> Value {
        self.constants[idx]
    }

    #[inline(always)]
    pub fn function(&self, id: FunctionId) -> &Function {
        &self.functions[id as usize]
    }

    #[inline(always)]
    pub fn instruction(&self, pc: usize) -> Instruction {
        self.instructions[pc]
    }
}
