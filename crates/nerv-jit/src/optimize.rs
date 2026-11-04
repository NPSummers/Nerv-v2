use super::*;
use llvm_sys::{
    core::{LLVMDisposeMessage, LLVMSetDataLayout, LLVMSetTarget},
    target::{LLVMCopyStringRepOfTargetData, LLVMDisposeTargetData},
    target_machine::*,
    transforms::pass_builder::*,
};

pub(super) fn optimize(module: LLVMModuleRef) -> Result<(), Error> {
    unsafe {
        let triple = LLVMGetDefaultTargetTriple();
        if triple.is_null() {
            return Err(Error::new("could not get the native LLVM target triple"));
        }
        let mut target = ptr::null_mut();
        let mut message = ptr::null_mut();
        if LLVMGetTargetFromTriple(triple, &mut target, &mut message) != 0 {
            let message = if message.is_null() {
                "could not resolve the native LLVM target".to_owned()
            } else {
                let text = CStr::from_ptr(message).to_string_lossy().into_owned();
                LLVMDisposeMessage(message);
                text
            };
            LLVMDisposeMessage(triple);
            return Err(Error::new(message));
        }
        let cpu = LLVMGetHostCPUName();
        let features = LLVMGetHostCPUFeatures();
        let machine = LLVMCreateTargetMachine(
            target,
            triple,
            cpu,
            features,
            LLVMCodeGenOptLevel::LLVMCodeGenLevelAggressive,
            LLVMRelocMode::LLVMRelocDefault,
            LLVMCodeModel::LLVMCodeModelJITDefault,
        );
        LLVMDisposeMessage(cpu);
        LLVMDisposeMessage(features);
        LLVMSetTarget(module, triple);
        LLVMDisposeMessage(triple);
        if machine.is_null() {
            return Err(Error::new("could not create an LLVM target machine"));
        }
        let layout = LLVMCreateTargetDataLayout(machine);
        let layout_text = LLVMCopyStringRepOfTargetData(layout);
        LLVMSetDataLayout(module, layout_text);
        LLVMDisposeMessage(layout_text);
        LLVMDisposeTargetData(layout);
        let options = LLVMCreatePassBuilderOptions();
        let error = LLVMRunPasses(module, c"default<O3>".as_ptr(), machine, options);
        LLVMDisposePassBuilderOptions(options);
        LLVMDisposeTargetMachine(machine);
        if error.is_null() {
            Ok(())
        } else {
            Err(llvm_error(error))
        }
    }
}
