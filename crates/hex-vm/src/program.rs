use crate::{
    Error,
    host::Syscode,
    instruction::{Instruction, Reg},
    value::word,
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

/// A function's handler table is a contiguous span of the program-wide
/// [`HandlerEntry`] table, addressed by index. Keeps [`Function`] `Copy`
/// and free of owned storage.
#[derive(Debug, Clone, Copy, Default)]
pub struct HandlerSpan {
    pub start: u32,
    pub len: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Function {
    /// Function callable type
    pub ty: FnType,
    /// Number of arguments the function expects
    pub narg: Reg,
    /// Number of values the function returns
    pub nret: Reg,
    /// Number of registers the function uses
    pub nreg: Reg,
    /// Trap handlers, as a span into the program's handler table.
    pub handlers: HandlerSpan,
}

/// A borrowed, read-only view of a fully linked program.
///
/// Four independent slices sharing one lifetime. Point them at `&'static`
/// data (flash/ROM), stack/`static` arrays, or borrow from an owned
/// [`ProgramBuf`] (the `alloc` feature). `Copy` and zero-cost; pass it by
/// value into the interpreter so it lives in registers across the hot loop.
#[derive(Debug, Clone, Copy)]
pub struct Program<'p> {
    instructions: &'p [Instruction],
    constants: &'p [word],
    functions: &'p [Function],
    handlers: &'p [HandlerEntry],
}

impl<'p> Program<'p> {
    pub fn new(
        instructions: &'p [Instruction],
        constants: &'p [word],
        functions: &'p [Function],
        handlers: &'p [HandlerEntry],
    ) -> Self {
        Self { instructions, constants, functions, handlers }
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    #[inline(always)]
    pub fn constants(&self) -> &'p [word] {
        self.constants
    }

    #[inline(always)]
    pub fn functions(&self) -> &'p [Function] {
        self.functions
    }

    #[inline(always)]
    pub fn instructions(&self) -> &'p [Instruction] {
        self.instructions
    }

    #[inline(always)]
    pub fn handlers(&self) -> &'p [HandlerEntry] {
        self.handlers
    }

    #[inline(always)]
    pub fn constant(&self, idx: ConstantId) -> word {
        self.constants[idx as usize]
    }

    #[inline(always)]
    pub fn function(&self, id: FunctionId) -> &'p Function {
        &self.functions[id as usize]
    }

    #[inline(always)]
    pub fn instruction(&self, pc: usize) -> Instruction {
        self.instructions[pc]
    }

    /// Most-deeply-nested handler covering `pc` within `func`, if any.
    #[inline]
    pub fn handler_for(&self, func: FunctionId, pc: usize) -> Option<&'p HandlerEntry> {
        let HandlerSpan { start, len } = self.functions[func as usize].handlers;
        let span = &self.handlers[start as usize..start as usize + len as usize];
        // ranges are emitted nested, so the LAST matching entry is the most
        // deeply nested. Iterate reversed to catch the inner try.
        span.iter().rev().find(|h| h.start_pc <= pc && pc < h.end_pc)
    }
}

/// Owned backing storage for a [`Program`]. Build once, then call
/// [`ProgramBuf::program`] for cheap borrowed views to execute.
#[cfg(feature = "alloc")]
#[derive(Debug, Clone)]
pub struct ProgramBuf {
    instructions: alloc::boxed::Box<[Instruction]>,
    constants: alloc::boxed::Box<[word]>,
    functions: alloc::boxed::Box<[Function]>,
    handlers: alloc::boxed::Box<[HandlerEntry]>,
}

#[cfg(feature = "alloc")]
impl ProgramBuf {
    pub fn new(
        instructions: alloc::boxed::Box<[Instruction]>,
        constants: alloc::boxed::Box<[word]>,
        functions: alloc::boxed::Box<[Function]>,
        handlers: alloc::boxed::Box<[HandlerEntry]>,
    ) -> Self {
        Self { instructions, constants, functions, handlers }
    }

    #[inline]
    pub fn program(&self) -> Program<'_> {
        Program {
            instructions: &self.instructions,
            constants: &self.constants,
            functions: &self.functions,
            handlers: &self.handlers,
        }
    }
}
