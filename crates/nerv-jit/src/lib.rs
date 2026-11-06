use std::{
    ffi::{CStr, CString},
    fmt, mem, ptr,
    sync::Mutex,
};

use llvm_sys::{
    core::{LLVMContextCreate, LLVMContextDispose, LLVMDisposeModule},
    error::{LLVMDisposeErrorMessage, LLVMErrorRef, LLVMGetErrorMessage},
    orc2::{
        LLVMJITEvaluatedSymbol, LLVMJITSymbolFlags, LLVMJITSymbolGenericFlags,
        LLVMOrcAbsoluteSymbols, LLVMOrcCSymbolMapPair,
        LLVMOrcCreateNewThreadSafeContextFromLLVMContext, LLVMOrcCreateNewThreadSafeModule,
        LLVMOrcDisposeThreadSafeContext, LLVMOrcJITDylibDefine, lljit::*,
    },
    prelude::{LLVMContextRef, LLVMModuleRef},
    target::{LLVM_InitializeNativeAsmPrinter, LLVM_InitializeNativeTarget},
};
use nerv_sema::ir::{CheckedProgram, Span, SymbolId, Type};
use std::path::Path;

mod compiler;
mod optimize;
mod range;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub message: String,
    pub span: Option<Span>,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span: None,
        }
    }
    fn at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Error {}

static JIT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildMode {
    Debug,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuTarget {
    Generic,
    Native,
}

/// Emit a host-native, position-independent object without creating a JIT.
/// The object includes a C main adapter and links against runtime ABI version 2.
pub fn emit_object(program: &CheckedProgram, output: &Path, mode: BuildMode) -> Result<(), Error> {
    emit_object_for_cpu(program, output, mode, CpuTarget::Generic)
}

/// Native uses the same host CPU/features as the JIT. Such objects are not
/// guaranteed to run on older CPUs; Generic remains the distribution default.
pub fn emit_object_for_cpu(
    program: &CheckedProgram,
    output: &Path,
    mode: BuildMode,
    cpu: CpuTarget,
) -> Result<(), Error> {
    use llvm_sys::target_machine::{LLVMCodeGenFileType, LLVMTargetMachineEmitToFile};
    let _lock = JIT_LOCK
        .lock()
        .map_err(|_| Error::new("LLVM lock poisoned"))?;
    initialize_native()?;
    if program.main().is_none() {
        return Err(Error::new("missing `main` function"));
    }
    let generated = compiler::generate_for_cpu(program, mode, true, cpu)?;
    let machine = optimize::Machine::new(generated.module, mode, cpu, true)?;
    let path = CString::new(output.as_os_str().as_encoded_bytes())
        .map_err(|_| Error::new("NUL byte in object path"))?;
    let mut message = ptr::null_mut();
    let failed = unsafe {
        LLVMTargetMachineEmitToFile(
            machine.raw,
            generated.module,
            path.as_ptr().cast_mut(),
            LLVMCodeGenFileType::LLVMObjectFile,
            &mut message,
        )
    };
    if failed != 0 {
        let error = if message.is_null() {
            "could not emit object file".into()
        } else {
            let error = unsafe { CStr::from_ptr(message) }
                .to_string_lossy()
                .into_owned();
            unsafe { llvm_sys::core::LLVMDisposeMessage(message) };
            error
        };
        Err(Error::new(error))
    } else {
        Ok(())
    }
}

fn initialize_native() -> Result<(), Error> {
    if unsafe { LLVM_InitializeNativeTarget() != 0 || LLVM_InitializeNativeAsmPrinter() != 0 } {
        return Err(Error::new("could not initialize the native LLVM target"));
    }
    Ok(())
}

pub fn host_cpu_identity() -> Result<String, String> {
    unsafe {
        let name = llvm_sys::target_machine::LLVMGetHostCPUName();
        let features = llvm_sys::target_machine::LLVMGetHostCPUFeatures();
        if name.is_null() || features.is_null() {
            if !name.is_null() {
                llvm_sys::core::LLVMDisposeMessage(name);
            }
            if !features.is_null() {
                llvm_sys::core::LLVMDisposeMessage(features);
            }
            return Err("could not identify host CPU".into());
        }
        let identity = format!(
            "{}|{}",
            CStr::from_ptr(name).to_string_lossy(),
            CStr::from_ptr(features).to_string_lossy()
        );
        llvm_sys::core::LLVMDisposeMessage(name);
        llvm_sys::core::LLVMDisposeMessage(features);
        Ok(identity)
    }
}

pub fn run_main(program: &CheckedProgram) -> Result<i64, Error> {
    let _lock = JIT_LOCK
        .lock()
        .map_err(|_| Error::new("JIT lock poisoned"))?;
    let _execution = nerv_runtime::Execution::new();
    let main = program
        .main()
        .ok_or_else(|| Error::new("missing `main` function"))?;
    let mut jit = Jit::new()?;
    jit.add_module(compiler::generate(program, BuildMode::Release, false)?)?;
    jit.run(&symbol(main), program.function(main).result == Type::Int)
}

pub fn run_tests(program: &CheckedProgram) -> Result<usize, Error> {
    run_group(program, program.tests())
}

pub fn run_tests_in_source(
    program: &CheckedProgram,
    source: nerv_sema::ir::SourceId,
) -> Result<usize, Error> {
    let functions = program
        .tests()
        .iter()
        .copied()
        .filter(|id| program.function(*id).span.source == source)
        .collect::<Vec<_>>();
    run_group(program, &functions)
}

pub fn run_benches(program: &CheckedProgram) -> Result<usize, Error> {
    run_group(program, program.benches())
}

fn run_group(program: &CheckedProgram, functions: &[SymbolId]) -> Result<usize, Error> {
    let _lock = JIT_LOCK
        .lock()
        .map_err(|_| Error::new("JIT lock poisoned"))?;
    let _execution = nerv_runtime::Execution::new();
    let mut jit = Jit::new()?;
    jit.add_module(compiler::generate(program, BuildMode::Release, false)?)?;
    for id in functions {
        jit.run(&symbol(*id), false)?;
    }
    Ok(functions.len())
}

// Debug/source names never participate in linkage or lookup. Runtime ABI symbols
// use a separate namespace, so user functions cannot accidentally replace them.
fn symbol(id: SymbolId) -> String {
    format!("__nerv_fn_{}", id.0)
}

pub struct Context {
    raw: LLVMContextRef,
}
impl Context {
    pub fn new() -> Self {
        let raw = unsafe { LLVMContextCreate() };
        assert!(!raw.is_null(), "LLVMContextCreate returned null");
        Self { raw }
    }
    pub fn as_raw(&self) -> LLVMContextRef {
        self.raw
    }
}
impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe { LLVMContextDispose(self.raw) };
    }
}

struct GeneratedModule {
    module: LLVMModuleRef,
    context: LLVMContextRef,
}
impl Drop for GeneratedModule {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeModule(self.module);
            LLVMContextDispose(self.context);
        }
    }
}

struct Jit {
    raw: LLVMOrcLLJITRef,
}
impl Jit {
    fn new() -> Result<Self, Error> {
        unsafe {
            initialize_native()?;
            let builder = LLVMOrcCreateLLJITBuilder();
            if builder.is_null() {
                return Err(Error::new("could not create an LLJIT builder"));
            }
            let mut raw = ptr::null_mut();
            let error = LLVMOrcCreateLLJIT(&mut raw, builder);
            if !error.is_null() {
                return Err(llvm_error(error));
            }
            let jit = Self { raw };
            jit.install_runtime()?;
            Ok(jit)
        }
    }

    fn install_runtime(&self) -> Result<(), Error> {
        unsafe {
            let runtime = nerv_runtime::symbols();
            let mut symbols = Vec::with_capacity(runtime.len());
            for symbol in &runtime {
                let name = cstring(symbol.name)?;
                let name = LLVMOrcLLJITMangleAndIntern(self.raw, name.as_ptr());
                if name.is_null() {
                    return Err(Error::new("could not intern a runtime symbol"));
                }
                symbols.push(LLVMOrcCSymbolMapPair {
                    Name: name,
                    Sym: LLVMJITEvaluatedSymbol {
                        Address: symbol.address as usize as u64,
                        Flags: LLVMJITSymbolFlags {
                            GenericFlags:
                                (LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsExported as u8)
                                    | (LLVMJITSymbolGenericFlags::LLVMJITSymbolGenericFlagsCallable
                                        as u8),
                            TargetFlags: 0,
                        },
                    },
                });
            }
            let unit = LLVMOrcAbsoluteSymbols(symbols.as_mut_ptr(), symbols.len());
            if unit.is_null() {
                return Err(Error::new("could not create runtime symbols"));
            }
            let error = LLVMOrcJITDylibDefine(LLVMOrcLLJITGetMainJITDylib(self.raw), unit);
            if error.is_null() {
                Ok(())
            } else {
                Err(llvm_error(error))
            }
        }
    }

    fn add_module(&mut self, module: GeneratedModule) -> Result<(), Error> {
        let module = mem::ManuallyDrop::new(module);
        unsafe {
            let context = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(module.context);
            if context.is_null() {
                LLVMDisposeModule(module.module);
                LLVMContextDispose(module.context);
                return Err(Error::new("could not create an LLVM thread-safe context"));
            }
            let thread_safe = LLVMOrcCreateNewThreadSafeModule(module.module, context);
            if thread_safe.is_null() {
                LLVMDisposeModule(module.module);
                LLVMOrcDisposeThreadSafeContext(context);
                return Err(Error::new("could not create an LLVM thread-safe module"));
            }
            let error = LLVMOrcLLJITAddLLVMIRModule(
                self.raw,
                LLVMOrcLLJITGetMainJITDylib(self.raw),
                thread_safe,
            );
            LLVMOrcDisposeThreadSafeContext(context);
            if error.is_null() {
                Ok(())
            } else {
                // LLJITAddLLVMIRModule consumes the module even on error.
                // Disposing it here would double-free LLVM-owned memory.
                Err(llvm_error(error))
            }
        }
    }

    fn run(&self, name: &str, returns_int: bool) -> Result<i64, Error> {
        let name = cstring(name)?;
        let mut address = 0;
        unsafe {
            let error = LLVMOrcLLJITLookup(self.raw, &mut address, name.as_ptr());
            if !error.is_null() {
                return Err(llvm_error(error));
            }
            if returns_int {
                let function: unsafe extern "C" fn() -> i64 = mem::transmute(address);
                Ok(function())
            } else {
                let function: unsafe extern "C" fn() = mem::transmute(address);
                function();
                Ok(0)
            }
        }
    }
}
impl Drop for Jit {
    fn drop(&mut self) {
        unsafe {
            let error = LLVMOrcDisposeLLJIT(self.raw);
            if !error.is_null() {
                llvm_error(error);
            }
        }
    }
}

fn cstring(value: &str) -> Result<CString, Error> {
    CString::new(value).map_err(|_| Error::new("NUL byte in LLVM identifier"))
}
fn llvm_error(error: LLVMErrorRef) -> Error {
    unsafe {
        let message = LLVMGetErrorMessage(error);
        if message.is_null() {
            Error::new("unknown LLVM error")
        } else {
            let text = CStr::from_ptr(message).to_string_lossy().into_owned();
            LLVMDisposeErrorMessage(message);
            Error::new(text)
        }
    }
}
