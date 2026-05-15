use hex_vm::{Callable, Frame, FunctionId, HostFn, VM, VMError, Value, memory::Buffer};

pub mod heap;
pub mod memory;
pub mod object;

#[derive(thiserror::Error, Clone, Copy, Debug)]
pub enum RuntimeError {
    #[error("Invalid argument count: expected {exp}, got {got}")]
    InvalidArgCount { exp: usize, got: usize },
    #[error("VM error: {0}")]
    VMError(#[from] VMError),
}

pub struct Runtime<'v, B: Buffer> {
    vm: VM<'v, B>,
}

impl<'v, B: Buffer> Runtime<'v, B> {
    pub fn execute(&mut self, func: FunctionId, args: &[Value]) -> Result<&[Value], RuntimeError> {
        let func = self.vm.program.functions[func as usize];
        let narg = func.narg as usize;
        let argc = args.len();

        if argc != narg {
            return Err(RuntimeError::InvalidArgCount {
                exp: narg,
                got: argc,
            });
        }

        self.vm.reset();
        self.vm.registers.resize(func.nreg as usize, Value::ZERO);
        self.vm.registers[..narg].copy_from_slice(args);

        match func.callable {
            Callable::Vm(entry) => self.execute_vm(entry)?,
            Callable::Host(func) => self.execute_host(func)?,
        }

        Ok(&self.vm.registers[..func.nret as usize])
    }

    fn execute_vm(&mut self, entry: usize) -> Result<(), RuntimeError> {
        self.vm.base = 0;
        self.vm.pc = entry;
        self.vm.call_stack.push(Frame {
            ret_pc: 0,
            ret_base: 0,
        });

        while !self.vm.call_stack.is_empty() {
            // TODO: handle interrupt

            // TODO: Check gc

            // Dispatch instruction
            let instruction = self.vm.fetch()?;
            self.vm.pc += 1;
            self.vm.execute(instruction)?;
        }

        Ok(())
    }

    #[inline(always)]
    fn execute_host(&mut self, func: HostFn) -> Result<(), VMError> {
        func(&mut self.vm.registers)
    }
}
