use super::*;
use llvm_sys::{
    core::{LLVMCreateMessage, LLVMDisposeMessage, LLVMSetDataLayout, LLVMSetTarget},
    target::{LLVMCopyStringRepOfTargetData, LLVMDisposeTargetData},
    target_machine::*,
    transforms::pass_builder::*,
};

pub(super) struct Machine {
    pub raw: LLVMTargetMachineRef,
}
impl Drop for Machine {
    fn drop(&mut self) {
        unsafe { LLVMDisposeTargetMachine(self.raw) };
    }
}

impl Machine {
    pub fn new(
        module: LLVMModuleRef,
        mode: BuildMode,
        cpu_target: CpuTarget,
        standalone: bool,
    ) -> Result<Self, Error> {
        unsafe {
            let triple_text = CString::new(env!("NERV_CODEGEN_TARGET"))
                .map_err(|_| Error::new("invalid compiler target triple"))?;
            let triple = LLVMCreateMessage(triple_text.as_ptr());
            if triple.is_null() {
                return Err(Error::new("could not get the native LLVM target triple"));
            }
            let mut target = ptr::null_mut();
            let mut message = ptr::null_mut();
            if LLVMGetTargetFromTriple(triple, &mut target, &mut message) != 0 {
                let text = if message.is_null() {
                    "could not resolve native LLVM target".into()
                } else {
                    let text = CStr::from_ptr(message).to_string_lossy().into_owned();
                    LLVMDisposeMessage(message);
                    text
                };
                LLVMDisposeMessage(triple);
                return Err(Error::new(text));
            }
            let level = match mode {
                BuildMode::Debug => LLVMCodeGenOptLevel::LLVMCodeGenLevelNone,
                BuildMode::Release => LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
            };
            let relocation = if standalone {
                LLVMRelocMode::LLVMRelocPIC
            } else {
                LLVMRelocMode::LLVMRelocDefault
            };
            let model = if standalone {
                LLVMCodeModel::LLVMCodeModelDefault
            } else {
                LLVMCodeModel::LLVMCodeModelJITDefault
            };
            let raw = if cpu_target == CpuTarget::Generic {
                LLVMCreateTargetMachine(
                    target,
                    triple,
                    c"generic".as_ptr(),
                    c"".as_ptr(),
                    level,
                    relocation,
                    model,
                )
            } else {
                let cpu = LLVMGetHostCPUName();
                let features = LLVMGetHostCPUFeatures();
                let raw = LLVMCreateTargetMachine(
                    target, triple, cpu, features, level, relocation, model,
                );
                LLVMDisposeMessage(cpu);
                LLVMDisposeMessage(features);
                raw
            };
            LLVMSetTarget(module, triple);
            LLVMDisposeMessage(triple);
            if raw.is_null() {
                return Err(Error::new("could not create an LLVM target machine"));
            }
            let machine = Self { raw };
            let layout = LLVMCreateTargetDataLayout(raw);
            let text = LLVMCopyStringRepOfTargetData(layout);
            LLVMSetDataLayout(module, text);
            LLVMDisposeMessage(text);
            LLVMDisposeTargetData(layout);
            Ok(machine)
        }
    }
}

pub(super) fn optimize(
    module: LLVMModuleRef,
    mode: BuildMode,
    cpu: CpuTarget,
    standalone: bool,
) -> Result<(), Error> {
    let machine = Machine::new(module, mode, cpu, standalone)?;
    unsafe {
        let options = LLVMCreatePassBuilderOptions();
        let passes = match mode {
            BuildMode::Debug => c"default<O0>",
            BuildMode::Release => c"default<O3>",
        };
        let error = LLVMRunPasses(module, passes.as_ptr(), machine.raw, options);
        LLVMDisposePassBuilderOptions(options);
        if error.is_null() {
            Ok(())
        } else {
            Err(llvm_error(error))
        }
    }
}
