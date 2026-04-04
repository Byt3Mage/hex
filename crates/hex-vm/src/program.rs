use std::{collections::HashMap, rc::Rc, sync::Arc};

use crate::{
    instruction::{Instruction, Reg},
    name::Name,
    object::Value,
    vm::VMResult,
};

/// A single compilation unit (before linking)
pub struct Unit {
    /// Imports that need to be resolved during linking
    pub imports: Vec<Import>,

    /// Items this unit exposes to other units
    pub exports: ExportTable,

    /// Bytecode for all functions in this unit (local indices)
    pub bytecode: Vec<Instruction>,

    /// All constants used in this unit (local indices)
    pub constants: Vec<Value>,

    /// All bytecode functions defined in this unit (local indices)
    pub functions: Vec<FunctionInfo>,

    /// All native functions defined in this unit (local indices)
    pub native_functions: Vec<NativeFunc>,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct CallInfo {
    /// Entry point in the list of instructions
    pub entry_pc: usize,

    /// Number of registers allocated for this function
    pub nreg: Reg,

    /// Number of argument registers this function expects
    pub narg: Reg,

    /// Number of registers used for return value
    pub nret: Reg,

    /// Number of captured values (0 for regular functions)
    pub ncap: Reg,
}

#[derive(Debug, Clone)]
pub struct FunctionInfo {
    /// Function name with full path for debugging
    pub name: String,

    /// Call metadata for the function
    pub call_info: CallInfo,
}

pub struct NativeFunc {
    /// Function name with full path for debugging
    pub name: Name,

    /// Function implementation
    pub func: Rc<dyn Fn(&[Value], &mut [Value]) -> VMResult<()>>,

    /// Number of argument registers this function expects
    pub narg: u8,

    /// Number of return registers this function uses
    pub nret: u8,
}

/// Import declaration (unresolved)
pub struct Import {
    /// Name of the module to import from (as registered with the linker)
    pub module_name: Name,

    /// Symbols to import from that module
    pub symbols: ImportSymbols,
}

/// Symbols imported from another unit
pub struct ImportSymbols {
    /// Function name -> local function ID (before linking)
    pub functions: HashMap<Name, LocalFunctionId>,

    /// Native function name -> local function ID (before linking)
    pub native_functions: HashMap<Name, LocalFunctionId>,
}

/// Symbols exported by a unit
pub struct ExportTable {
    /// Top-level exported functions (name -> local ID)
    pub functions: HashMap<Name, LocalFunctionId>,

    /// Top-level exported native functions (name -> local ID)
    pub native_functions: HashMap<Name, LocalFunctionId>,

    /// Nested namespaces from mod blocks
    pub namespaces: HashMap<Name, ExportTable>,
}

/// Local function ID within a unit (before linking)
pub type LocalFunctionId = usize;

/// Module identifier
pub type UnitId = Name;

/// Global function ID in the linked program
pub type FunctionPtr = u16;

/// Global native function ID in the linked program
pub type NativeFunctionId = usize;

/// Fully linked program ready for execution
pub struct Program {
    /// All bytecode from all units
    pub bytecode: Arc<[Instruction]>,

    /// All constants from all units
    pub constants: Arc<[Value]>,

    /// All functions from all units, in global order
    pub funcs: Arc<[FunctionInfo]>,

    /// All native functions from all units, in global order
    pub native_funcs: Arc<[NativeFunc]>,
}

/// Mapping information used during linking
pub struct LinkContext {
    /// Map from (module_id, local_function_id) -> global_function_id
    pub function_map: HashMap<(UnitId, LocalFunctionId), FunctionPtr>,

    /// Map from (module_id, local_native_function_id) -> global_native_function_id
    pub native_function_map: HashMap<(UnitId, LocalFunctionId), NativeFunctionId>,

    /// Map from (module_id, local_constant_index) -> global_constant_index
    pub constant_map: HashMap<(UnitId, usize), usize>,

    /// Map from module_id to its bytecode offset in the merged program
    pub bytecode_offsets: HashMap<UnitId, usize>,
}
