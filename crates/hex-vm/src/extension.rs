use crate::{
    VMError, VMResult, VMState,
    instruction::{Instruction, Opcode},
};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DispatchResult {
    Halt,
    Continue,
}

pub trait ExtensionOps {
    fn dispatch(
        &mut self,
        op: Opcode,
        inst: Instruction,
        state: &mut VMState,
    ) -> VMResult<DispatchResult>;
}

/// No-op extension for VMs that don't need custom opcodes.
pub struct NoExtensions;

impl ExtensionOps for NoExtensions {
    fn dispatch(
        &mut self,
        op: Opcode,
        _: Instruction,
        _: &mut VMState,
    ) -> VMResult<DispatchResult> {
        Err(VMError::UnknownOp(op))
    }
}
