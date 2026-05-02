use std::{
    collections::{HashMap, hash_map::Entry},
    rc::Rc,
};

use crate::{
    VMResult, Value,
    instruction::{InstType, Instruction, Opcode, Reg, encode_abx, encode_ax},
};

#[repr(u8)]
pub enum PrimitiveType {
    I64,
    U64,
    F64,
    Bool,
    Handle,
}

/// A single compilation unit (before linking)
pub struct Module {
    /// Module name for linking
    pub name: String,
    /// Bytecode for all functions in this module
    pub bytecode: Box<[Instruction]>,
    /// Constants used by this module
    pub constants: Box<[Value]>,
    /// Functions defined in this module
    pub functions: Box<[Function]>,
    /// Native functions defined in this module
    pub native_functions: Box<[NativeFunction]>,
    /// Items this module exposes to other modules
    pub exports: Box<[Export]>,
    /// Items this module needs from other modules
    pub imports: Box<[Import]>,
}

#[derive(Debug, Clone)]
pub struct Function {
    /// Function name with full path for debugging
    pub name: String,
    /// Entry point in the list of instructions
    pub entry_pc: usize,
    /// Number of registers allocated for this function
    pub nreg: Reg,
    /// Number of argument registers this function expects
    pub narg: Reg,
    /// Number of registers used for return value
    pub nret: Reg,
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
}

#[derive(Clone)]
pub struct NativeFunction {
    /// Function name with full path for debugging
    pub name: String,
    /// Function implementation
    pub func: Rc<dyn Fn(&mut [Value]) -> VMResult<()>>,
    /// Number of registers this function expects
    /// Must be max(nargs, nret)
    pub nreg: u8,
}

pub struct Export {
    pub name: String,
    pub kind: ExportKind,
    pub local_index: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ExportKind {
    Constant,
    Function,
    NativeFunction,
}

/// Import declaration (unresolved)
pub struct Import {
    /// Name of the module to import from (as registered with the linker)
    pub module: String,
    /// Name of the symbol to import from that module
    pub name: String,
    /// Symbols to import from that module
    pub kind: ImportKind,
    /// Index of this import within its module
    pub local_index: u16,
}

pub enum ImportKind {
    Constant,
    Function,
    NativeFunc,
}

/// Global function ID in the linked program
pub type FunctionPtr = u16;

/// Fully linked program ready for execution
pub struct Program {
    /// Merged bytecode from all modules
    pub bytecode: Box<[Instruction]>,
    /// Merged constants from all modules
    pub consts: Box<[Value]>,
    /// All functions, globally indexed
    pub funcs: Box<[CallInfo]>,
    /// All native functions, globally indexed
    pub native_funcs: Box<[NativeFunction]>,
    /// Optional: type signatures for ABI validation
    pub signatures: Box<[FuncSignature]>,
    /// Optional: debug info (function names, source maps)
    pub debug_info: DebugInfo,
}

pub struct FuncSignature {
    pub args: Vec<PrimitiveType>,
    pub rets: Vec<PrimitiveType>,
}

pub struct DebugInfo {
    pub function_names: Vec<String>,
    // TODO: source maps, line tables, etc. later
}

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("unresolved import: {module}::{name}")]
    UnresolvedImport { module: String, name: String },

    #[error("duplicate export: {module}::{name}")]
    DuplicateExport { module: String, name: String },

    #[error("signature mismatch for {module}::{name}")]
    SignatureMismatch { module: String, name: String },
}

struct ResolvedSymbol {
    loc_idx: u16,
    mod_idx: usize,
}

pub fn link(modules: &[Module]) -> Result<Program, LinkError> {
    // 1. Build global export table: "module::name" -> symbol
    let mut exports: HashMap<(&str, &str), ResolvedSymbol> = HashMap::new();
    for (mod_idx, module) in modules.iter().enumerate() {
        for export in &module.exports {
            let key = (module.name.as_str(), export.name.as_str());
            match exports.entry(key) {
                Entry::Vacant(entry) => {
                    entry.insert(ResolvedSymbol {
                        loc_idx: export.local_index,
                        mod_idx,
                    });
                }
                Entry::Occupied(_) => {
                    return Err(LinkError::DuplicateExport {
                        module: module.name.clone(),
                        name: export.name.clone(),
                    });
                }
            }
        }
    }

    // 2. Calculate offsets for merging
    let mut bytecode_offsets = Vec::with_capacity(modules.len());
    let mut const_offsets = Vec::with_capacity(modules.len());
    let mut func_offsets = Vec::with_capacity(modules.len());
    let mut native_func_offsets = Vec::with_capacity(modules.len());

    let mut bytecode_offset = 0usize;
    let mut constant_offset = 0u16;
    let mut function_offset = 0u16;
    let mut native_func_offset = 0u16;

    for module in modules {
        bytecode_offsets.push(bytecode_offset);
        const_offsets.push(constant_offset);
        func_offsets.push(function_offset);
        native_func_offsets.push(native_func_offset);

        bytecode_offset += module.bytecode.len();
        constant_offset += module.constants.len() as u16;
        function_offset += module.functions.len() as u16;
        native_func_offset += module.native_functions.len() as u16;
    }

    // 3. Resolve imports and build remap tables per module
    // local function index -> global function index
    let mut func_remaps: Vec<HashMap<u16, u16>> = vec![HashMap::new(); modules.len()];
    let mut const_remaps: Vec<HashMap<u16, u16>> = vec![HashMap::new(); modules.len()];
    let mut native_func_remaps: Vec<HashMap<u16, u16>> = vec![HashMap::new(); modules.len()];

    for (mod_idx, module) in modules.iter().enumerate() {
        for import in &module.imports {
            let symbol = exports
                .get(&(import.module.as_str(), import.name.as_str()))
                .ok_or_else(|| LinkError::UnresolvedImport {
                    module: import.module.clone(),
                    name: import.name.clone(),
                })?;

            match import.kind {
                ImportKind::Function => {
                    let global_idx = func_offsets[symbol.mod_idx] + symbol.loc_idx;
                    func_remaps[mod_idx].insert(import.local_index, global_idx);
                }
                ImportKind::Constant => {
                    let global_idx = const_offsets[symbol.mod_idx] + symbol.loc_idx;
                    const_remaps[mod_idx].insert(import.local_index, global_idx);
                }
                ImportKind::NativeFunc => {
                    let global_idx = native_func_offsets[symbol.mod_idx] + symbol.loc_idx;
                    native_func_remaps[mod_idx].insert(import.local_index, global_idx);
                }
            };
        }
    }

    // 4. Merge bytecode, patching references
    let total_bc = bytecode_offset as usize;
    let mut bytecode = Vec::with_capacity(total_bc);

    for (mod_idx, module) in modules.iter().enumerate() {
        let bc_base = bytecode_offsets[mod_idx];

        for &inst in &module.bytecode {
            let inst = match inst.op() {
                // Patch jump targets
                Opcode::JMP => encode_ax(Opcode::JMP, inst.ax() + bc_base as InstType),
                Opcode::JMP_T => {
                    encode_abx(Opcode::JMP_T, inst.a(), inst.bx() + bc_base as InstType)
                }
                Opcode::JMP_F => {
                    encode_abx(Opcode::JMP_F, inst.a(), inst.bx() + bc_base as InstType)
                }

                // Patch function calls
                Opcode::CALL => {
                    let local = inst.bx() as u16;
                    let global = func_remaps[mod_idx].get(&local).copied().unwrap();
                    encode_abx(Opcode::CALL, inst.a(), global as InstType)
                }
                Opcode::CALLT => {
                    let local = inst.bx() as u16;
                    let global = func_remaps[mod_idx].get(&local).copied().unwrap();
                    encode_abx(Opcode::CALLT, inst.a(), global as InstType)
                }
                Opcode::CALLN => {
                    let local = inst.bx() as u16;
                    let global = native_func_remaps[mod_idx].get(&local).copied().unwrap();
                    encode_abx(Opcode::CALLN, inst.a(), global as InstType)
                }

                // Patch constant references
                Opcode::CONST => {
                    let local = inst.bx() as u16;
                    let global = const_remaps[mod_idx].get(&local).copied().unwrap();
                    encode_abx(Opcode::CONST, inst.a(), global as InstType)
                }

                // Everything else passes through unchanged
                _ => inst,
            };

            bytecode.push(inst);
        }
    }

    // 5. Merge constants
    let mut consts = Vec::with_capacity(constant_offset as usize);
    for module in modules {
        consts.extend_from_slice(&module.constants);
    }

    // 6. Merge functions
    let mut functions = Vec::with_capacity(function_offset as usize);
    for (mod_idx, module) in modules.iter().enumerate() {
        let bc_base = bytecode_offsets[mod_idx];
        functions.extend(module.functions.iter().map(|f| CallInfo {
            entry_pc: f.entry_pc + bc_base,
            nreg: f.nreg,
            narg: f.narg,
            nret: f.nret,
        }));
    }

    // 7. Merge native functions
    let mut native_funcs = Vec::with_capacity(constant_offset as usize);
    for module in modules {
        native_funcs.extend_from_slice(&module.native_functions);
    }

    // 8. Build debug info
    let mut function_names = Vec::with_capacity(functions.len());
    for module in modules {
        for func in &module.functions {
            function_names.push(format!("{}::{}", module.name, func.name));
        }
    }

    Ok(Program {
        bytecode: bytecode.into(),
        consts: consts.into(),
        funcs: functions.into(),
        native_funcs: native_funcs.into(),
        signatures: Box::new([]),
        debug_info: DebugInfo { function_names },
    })
}
