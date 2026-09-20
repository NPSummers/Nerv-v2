use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    fmt, mem, ptr,
    sync::Mutex,
};

use llvm_sys::{
    LLVMIntPredicate, LLVMRealPredicate,
    analysis::{LLVMVerifierFailureAction, LLVMVerifyModule},
    core::*,
    error::{LLVMDisposeErrorMessage, LLVMErrorRef, LLVMGetErrorMessage},
    orc2::{
        LLVMJITEvaluatedSymbol, LLVMJITSymbolFlags, LLVMJITSymbolGenericFlags,
        LLVMOrcAbsoluteSymbols, LLVMOrcCSymbolMapPair,
        LLVMOrcCreateNewThreadSafeContextFromLLVMContext, LLVMOrcCreateNewThreadSafeModule,
        LLVMOrcDisposeThreadSafeContext, LLVMOrcDisposeThreadSafeModule, LLVMOrcJITDylibDefine,
        LLVMOrcThreadSafeContextRef, lljit::*,
    },
    prelude::{
        LLVMBasicBlockRef, LLVMBuilderRef, LLVMContextRef, LLVMModuleRef, LLVMTypeRef, LLVMValueRef,
    },
    target::{
        LLVM_InitializeNativeAsmPrinter, LLVM_InitializeNativeTarget,
        LLVMCopyStringRepOfTargetData, LLVMDisposeTargetData,
    },
    target_machine::{
        LLVMCodeGenOptLevel, LLVMCodeModel, LLVMCreateTargetDataLayout, LLVMCreateTargetMachine,
        LLVMDisposeTargetMachine, LLVMGetDefaultTargetTriple, LLVMGetHostCPUFeatures,
        LLVMGetHostCPUName, LLVMGetTargetFromTriple, LLVMRelocMode,
    },
    transforms::pass_builder::{
        LLVMCreatePassBuilderOptions, LLVMDisposePassBuilderOptions, LLVMRunPasses,
    },
};
use nerv_syntax::{
    BinaryOp, Expr, Function, Module, Statement, Struct as StructDecl, Type, UnaryOp,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

static JIT_LOCK: Mutex<()> = Mutex::new(());

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

pub fn run_main(modules: &[Module]) -> Result<i64, Error> {
    let _lock = JIT_LOCK
        .lock()
        .map_err(|_| Error::new("JIT lock poisoned"))?;
    let main = all_functions(modules)
        .find(|function| function.name == "main")
        .ok_or_else(|| Error::new("missing `main` function"))?;
    if !main.params.is_empty() || !matches!(main.result, Type::Int | Type::Void) {
        return Err(Error::new(
            "`main` must have type fn() -> i64 or fn() -> void",
        ));
    }
    let mut jit = Jit::new()?;
    jit.add_module(lower(modules)?)?;
    jit.run_main(main.result == Type::Int)
}

pub fn run_tests(modules: &[Module]) -> Result<usize, Error> {
    run_prefixed(modules, "__nerv_test_")
}

pub fn run_benches(modules: &[Module]) -> Result<usize, Error> {
    run_prefixed(modules, "__nerv_bench_")
}

fn run_prefixed(modules: &[Module], prefix: &str) -> Result<usize, Error> {
    let _lock = JIT_LOCK
        .lock()
        .map_err(|_| Error::new("JIT lock poisoned"))?;
    let names = all_functions(modules)
        .filter(|function| function.name.starts_with(prefix))
        .map(|function| function.name.clone())
        .collect::<Vec<_>>();
    let mut jit = Jit::new()?;
    jit.add_module(lower(modules)?)?;
    for name in &names {
        jit.run_void(name)?;
    }
    Ok(names.len())
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

struct LoweredModule {
    module: LLVMModuleRef,
    context: LLVMOrcThreadSafeContextRef,
}

impl Drop for LoweredModule {
    fn drop(&mut self) {
        unsafe {
            LLVMDisposeModule(self.module);
            LLVMOrcDisposeThreadSafeContext(self.context);
        }
    }
}

struct Jit {
    raw: LLVMOrcLLJITRef,
}

impl Jit {
    fn new() -> Result<Self, Error> {
        unsafe {
            if LLVM_InitializeNativeTarget() != 0 || LLVM_InitializeNativeAsmPrinter() != 0 {
                return Err(Error::new("could not initialize the native LLVM target"));
            }
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
            let runtime_symbols = nerv_runtime::symbols();
            let mut symbols = Vec::with_capacity(runtime_symbols.len());
            for symbol in &runtime_symbols {
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

    fn add_module(&mut self, module: LoweredModule) -> Result<(), Error> {
        let module = mem::ManuallyDrop::new(module);
        unsafe {
            let thread_safe_module =
                LLVMOrcCreateNewThreadSafeModule(module.module, module.context);
            if thread_safe_module.is_null() {
                LLVMDisposeModule(module.module);
                LLVMOrcDisposeThreadSafeContext(module.context);
                return Err(Error::new("could not create an LLVM thread-safe module"));
            }
            let error = LLVMOrcLLJITAddLLVMIRModule(
                self.raw,
                LLVMOrcLLJITGetMainJITDylib(self.raw),
                thread_safe_module,
            );
            LLVMOrcDisposeThreadSafeContext(module.context);
            if error.is_null() {
                Ok(())
            } else {
                LLVMOrcDisposeThreadSafeModule(thread_safe_module);
                Err(llvm_error(error))
            }
        }
    }

    fn run_main(&self, returns_int: bool) -> Result<i64, Error> {
        let name = cstring("main")?;
        let mut address = 0;
        unsafe {
            let error = LLVMOrcLLJITLookup(self.raw, &mut address, name.as_ptr());
            if !error.is_null() {
                return Err(llvm_error(error));
            }
            if returns_int {
                let main: unsafe extern "C" fn() -> i64 = mem::transmute(address);
                Ok(main())
            } else {
                let main: unsafe extern "C" fn() = mem::transmute(address);
                main();
                Ok(0)
            }
        }
    }

    fn run_void(&self, name: &str) -> Result<(), Error> {
        let name = cstring(name)?;
        let mut address = 0;
        unsafe {
            let error = LLVMOrcLLJITLookup(self.raw, &mut address, name.as_ptr());
            if !error.is_null() {
                return Err(llvm_error(error));
            }
            let function: unsafe extern "C" fn() = mem::transmute(address);
            function();
        }
        Ok(())
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

struct Compiler {
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    structs: HashMap<String, StructValue>,
    enums: HashMap<String, EnumValue>,
    functions: HashMap<String, FunctionValue>,
    function_slots: HashMap<usize, LLVMTypeRef>,
    lambda_index: usize,
}

#[derive(Clone, Copy)]
struct FunctionValue {
    value: LLVMValueRef,
    ty: LLVMTypeRef,
}

#[derive(Clone)]
struct StructValue {
    ty: LLVMTypeRef,
    fields: HashMap<String, (u32, LLVMTypeRef)>,
}

#[derive(Clone)]
struct EnumValue {
    ty: LLVMTypeRef,
    variants: HashMap<String, (i64, bool)>,
    has_payload: bool,
}

#[derive(Clone, Copy)]
struct Value {
    value: LLVMValueRef,
    ty: LLVMTypeRef,
}

fn all_functions(source: &[Module]) -> impl Iterator<Item = &Function> {
    source.iter().flat_map(|module| {
        module
            .functions
            .iter()
            .chain(
                module
                    .structs
                    .iter()
                    .flat_map(|structure| structure.methods.iter()),
            )
            .chain(
                module
                    .impls
                    .iter()
                    .flat_map(|implementation| implementation.methods.iter()),
            )
    })
}

fn lower(source: &[Module]) -> Result<LoweredModule, Error> {
    unsafe {
        let context = LLVMContextCreate();
        if context.is_null() {
            return Err(Error::new("could not create an LLVM context"));
        }
        let thread_safe_context = LLVMOrcCreateNewThreadSafeContextFromLLVMContext(context);
        if thread_safe_context.is_null() {
            LLVMContextDispose(context);
            return Err(Error::new("could not create an LLVM thread-safe context"));
        }
        let name = cstring("nerv")?;
        let module = LLVMModuleCreateWithNameInContext(name.as_ptr(), context);
        let builder = LLVMCreateBuilderInContext(context);
        let mut compiler = Compiler {
            context,
            module,
            builder,
            structs: HashMap::new(),
            enums: HashMap::new(),
            functions: HashMap::new(),
            function_slots: HashMap::new(),
            lambda_index: 0,
        };
        let result = compiler
            .compile(source)
            .and_then(|()| optimize(module))
            .map(|()| LoweredModule {
                module,
                context: thread_safe_context,
            });
        LLVMDisposeBuilder(builder);
        if result.is_err() {
            LLVMDisposeModule(module);
            LLVMOrcDisposeThreadSafeContext(thread_safe_context);
        }
        result
    }
}

fn optimize(module: LLVMModuleRef) -> Result<(), Error> {
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

impl Compiler {
    fn compile(&mut self, source: &[Module]) -> Result<(), Error> {
        self.declare_structs(source)?;
        self.declare_enums(source)?;
        for function in all_functions(source) {
            let mut params = function
                .params
                .iter()
                .map(|param| self.ty(&param.ty))
                .collect::<Result<Vec<_>, _>>()?;
            let result = self.ty(&function.result)?;
            let function_ty =
                unsafe { LLVMFunctionType(result, params.as_mut_ptr(), params.len() as u32, 0) };
            let name = cstring(&function.name)?;
            let value = unsafe { LLVMAddFunction(self.module, name.as_ptr(), function_ty) };
            if self
                .functions
                .insert(
                    function.name.clone(),
                    FunctionValue {
                        value,
                        ty: function_ty,
                    },
                )
                .is_some()
            {
                return Err(Error::new(format!(
                    "duplicate function `{}`",
                    function.name
                )));
            }
        }
        for function in all_functions(source) {
            self.function(function)?;
        }
        unsafe {
            let mut message = ptr::null_mut();
            if LLVMVerifyModule(
                self.module,
                LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut message,
            ) != 0
            {
                let message = if message.is_null() {
                    "invalid LLVM module".to_owned()
                } else {
                    let text = CStr::from_ptr(message).to_string_lossy().into_owned();
                    LLVMDisposeMessage(message);
                    text
                };
                return Err(Error::new(message));
            }
        }
        Ok(())
    }

    fn declare_structs(&mut self, source: &[Module]) -> Result<(), Error> {
        for structure in source.iter().flat_map(|module| &module.structs) {
            let name = cstring(&structure.name)?;
            let ty = unsafe { LLVMStructCreateNamed(self.context, name.as_ptr()) };
            if self
                .structs
                .insert(
                    structure.name.clone(),
                    StructValue {
                        ty,
                        fields: HashMap::new(),
                    },
                )
                .is_some()
            {
                return Err(Error::new(format!("duplicate struct `{}`", structure.name)));
            }
        }
        for structure in source.iter().flat_map(|module| &module.structs) {
            self.define_struct(structure)?;
        }
        Ok(())
    }

    fn define_struct(&mut self, structure: &StructDecl) -> Result<(), Error> {
        let mut fields = Vec::with_capacity(structure.fields.len());
        let mut field_types = Vec::with_capacity(structure.fields.len());
        for (index, field) in structure.fields.iter().enumerate() {
            let ty = self.ty(&field.ty)?;
            fields.push((field.name.clone(), (index as u32, ty)));
            field_types.push(ty);
        }
        let structure = self.structs.get_mut(&structure.name).unwrap();
        unsafe {
            LLVMStructSetBody(
                structure.ty,
                field_types.as_mut_ptr(),
                field_types.len() as u32,
                0,
            )
        };
        structure.fields.extend(fields);
        Ok(())
    }

    fn declare_enums(&mut self, source: &[Module]) -> Result<(), Error> {
        for enumeration in source.iter().flat_map(|module| &module.enums) {
            let mut variants = HashMap::new();
            let has_payload = enumeration
                .variants
                .iter()
                .any(|variant| !variant.fields.is_empty());
            for (index, variant) in enumeration.variants.iter().enumerate() {
                if variant.fields.len() > 1
                    || variant.fields.first().is_some_and(|ty| *ty != Type::Int)
                {
                    return Err(Error::new(
                        "enum payloads currently require at most one i64 value",
                    ));
                }
                if variants
                    .insert(
                        variant.name.clone(),
                        (index as i64, !variant.fields.is_empty()),
                    )
                    .is_some()
                {
                    return Err(Error::new(format!(
                        "duplicate enum variant `{}`",
                        variant.name
                    )));
                }
            }
            if self
                .enums
                .insert(
                    enumeration.name.clone(),
                    EnumValue {
                        ty: if has_payload {
                            unsafe {
                                LLVMStructTypeInContext(
                                    self.context,
                                    &mut [self.int_ty(), self.int_ty()] as *mut LLVMTypeRef,
                                    2,
                                    0,
                                )
                            }
                        } else {
                            self.int_ty()
                        },
                        variants,
                        has_payload,
                    },
                )
                .is_some()
            {
                return Err(Error::new(format!("duplicate enum `{}`", enumeration.name)));
            }
        }
        Ok(())
    }

    fn function(&mut self, source: &Function) -> Result<(), Error> {
        let function = self.functions[&source.name];
        let entry = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function.value, c"entry".as_ptr())
        };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, entry) };
        let mut values = HashMap::new();
        for (index, param) in source.params.iter().enumerate() {
            let ty = self.ty(&param.ty)?;
            let name = cstring(&param.name)?;
            let slot = unsafe { LLVMBuildAlloca(self.builder, ty, name.as_ptr()) };
            unsafe {
                LLVMBuildStore(
                    self.builder,
                    LLVMGetParam(function.value, index as u32),
                    slot,
                )
            };
            values.insert(param.name.clone(), Value { value: slot, ty });
            if matches!(param.ty, Type::Function { .. }) {
                self.function_slots
                    .insert(slot as usize, self.function_ty(&param.ty)?);
            }
        }

        let mut last = None;
        for statement in &source.body {
            match statement {
                Statement::Let {
                    name, ty, value, ..
                } => {
                    let value = self.expr(value, &values)?;
                    if let Some(ty) = ty {
                        if let Ok(ty) = self.ty(ty) {
                            self.require_type(value, ty)?;
                        }
                    }
                    let llvm_name = cstring(name)?;
                    let slot =
                        unsafe { LLVMBuildAlloca(self.builder, value.ty, llvm_name.as_ptr()) };
                    unsafe { LLVMBuildStore(self.builder, value.value, slot) };
                    values.insert(
                        name.clone(),
                        Value {
                            value: slot,
                            ty: value.ty,
                        },
                    );
                    if let Some(ty @ Type::Function { .. }) = ty {
                        self.function_slots
                            .insert(slot as usize, self.function_ty(ty)?);
                    }
                    last = None;
                }
                Statement::Assign { name, value } => {
                    self.assign(name, value, &values)?;
                    last = None;
                }
                Statement::AssignIndex { base, index, value } => {
                    self.assign_index(base, index, value, &values)?;
                    last = None;
                }
                Statement::AssignField { base, name, value } => {
                    self.assign_field(base, name, value, &values)?;
                    last = None;
                }
                Statement::While { condition, body } => {
                    self.while_loop(condition, body, &values)?;
                    last = None;
                }
                Statement::For {
                    name,
                    start,
                    end,
                    inclusive,
                    body,
                } => {
                    self.for_loop(name, start, end, *inclusive, body, &values)?;
                    last = None;
                }
                Statement::Return(value) => {
                    let value = self.expr(value, &values)?;
                    self.require_type(value, self.ty(&source.result)?)?;
                    unsafe { LLVMBuildRet(self.builder, value.value) };
                    return Ok(());
                }
                Statement::Expr(expr) => last = Some(self.expr(expr, &values)?),
            }
        }
        match source.result {
            Type::Void => unsafe { LLVMBuildRetVoid(self.builder) },
            _ => {
                let value = last.ok_or_else(|| {
                    Error::new(format!("function `{}` must return a value", source.name))
                })?;
                self.require_type(value, self.ty(&source.result)?)?;
                unsafe { LLVMBuildRet(self.builder, value.value) }
            }
        };
        Ok(())
    }

    fn expr(&mut self, expr: &Expr, values: &HashMap<String, Value>) -> Result<Value, Error> {
        match expr {
            Expr::Int(value) => Ok(Value {
                value: unsafe { LLVMConstInt(self.int_ty(), *value as u64, 1) },
                ty: self.int_ty(),
            }),
            Expr::Float(value) => Ok(Value {
                value: unsafe { LLVMConstReal(self.float_ty(), *value) },
                ty: self.float_ty(),
            }),
            Expr::String(value) => {
                let text = cstring(value)?;
                let bytes = unsafe {
                    LLVMBuildGlobalString(self.builder, text.as_ptr(), c"string".as_ptr())
                };
                let length = Value {
                    value: unsafe { LLVMConstInt(self.int_ty(), value.len() as u64, 0) },
                    ty: self.int_ty(),
                };
                self.call_external(
                    "nerv_string_new",
                    self.string_ty(),
                    &[
                        Value {
                            value: bytes,
                            ty: self.string_ty(),
                        },
                        length,
                    ],
                )
            }
            Expr::Bool(value) => Ok(Value {
                value: unsafe { LLVMConstInt(self.bool_ty(), *value as u64, 0) },
                ty: self.bool_ty(),
            }),
            Expr::Array(elements) => {
                let array = self.call_external(
                    "nerv_array_i64_new",
                    self.array_ty(),
                    &[
                        Value {
                            value: unsafe { LLVMConstInt(self.int_ty(), elements.len() as u64, 0) },
                            ty: self.int_ty(),
                        },
                        Value {
                            value: unsafe { LLVMConstInt(self.int_ty(), 0, 0) },
                            ty: self.int_ty(),
                        },
                    ],
                )?;
                let array = self.call_external("nerv_array_i64_data", self.array_ty(), &[array])?;
                for (index, element) in elements.iter().enumerate() {
                    let element = self.expr(element, values)?;
                    self.require_type(element, self.int_ty())?;
                    self.array_store(
                        array,
                        Value {
                            value: unsafe { LLVMConstInt(self.int_ty(), index as u64, 0) },
                            ty: self.int_ty(),
                        },
                        element,
                    )?;
                }
                Ok(array)
            }
            Expr::ArrayRepeat { value, length } => {
                let value = self.expr(value, values)?;
                let length = self.expr(length, values)?;
                self.require_type(value, self.int_ty())?;
                self.require_type(length, self.int_ty())?;
                let array =
                    self.call_external("nerv_array_i64_new", self.array_ty(), &[length, value])?;
                self.call_external("nerv_array_i64_data", self.array_ty(), &[array])
            }
            Expr::Match { value, arms } => self.match_expr(value, arms, values),
            Expr::Method { base, name, args } => self.method_call(base, name, args, values),
            Expr::Lambda {
                params,
                result,
                body,
            } => {
                let name = format!("__lambda_{}", self.lambda_index);
                self.lambda_index += 1;
                let function = Function {
                    name: name.clone(),
                    params: params.clone(),
                    result: result.clone(),
                    body: vec![Statement::Expr((**body).clone())],
                    line: 0,
                    column: 0,
                };
                let mut params = function
                    .params
                    .iter()
                    .map(|param| self.ty(&param.ty))
                    .collect::<Result<Vec<_>, _>>()?;
                let result = self.ty(&function.result)?;
                let ty = unsafe {
                    LLVMFunctionType(result, params.as_mut_ptr(), params.len() as u32, 0)
                };
                let llvm_name = cstring(&name)?;
                let value = unsafe { LLVMAddFunction(self.module, llvm_name.as_ptr(), ty) };
                self.functions.insert(name, FunctionValue { value, ty });
                let block = unsafe { LLVMGetInsertBlock(self.builder) };
                self.function(&function)?;
                unsafe { LLVMPositionBuilderAtEnd(self.builder, block) };
                Ok(Value {
                    value,
                    ty: self.array_ty(),
                })
            }
            Expr::Name(name) => match values.get(name.as_str()) {
                Some(value) => Ok(Value {
                    value: unsafe {
                        LLVMBuildLoad2(self.builder, value.ty, value.value, c"load".as_ptr())
                    },
                    ty: value.ty,
                }),
                None => self
                    .functions
                    .get(name)
                    .map(|function| Value {
                        value: function.value,
                        ty: self.array_ty(),
                    })
                    .ok_or_else(|| Error::new(format!("unknown value `{name}`"))),
            },
            Expr::Struct { name, fields } => {
                let structure = self
                    .structs
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Error::new(format!("unknown struct `{name}`")))?;
                let mut value = unsafe { LLVMGetUndef(structure.ty) };
                if fields.len() != structure.fields.len() {
                    return Err(Error::new("wrong struct field count"));
                }
                for (name, field_value) in fields {
                    let (index, ty) = structure
                        .fields
                        .get(name)
                        .copied()
                        .ok_or_else(|| Error::new(format!("unknown field `{name}`")))?;
                    let field_value = self.expr(field_value, values)?;
                    self.require_type(field_value, ty)?;
                    value = unsafe {
                        LLVMBuildInsertValue(
                            self.builder,
                            value,
                            field_value.value,
                            index,
                            c"field".as_ptr(),
                        )
                    };
                }
                Ok(Value {
                    value,
                    ty: structure.ty,
                })
            }
            Expr::If {
                condition,
                then_body,
                else_body,
            } => self.if_expr(condition, then_body, else_body, values),
            Expr::Unary { op, value } => {
                let value = self.expr(value, values)?;
                match op {
                    UnaryOp::Neg if value.ty == self.int_ty() => Ok(Value {
                        value: unsafe { LLVMBuildNeg(self.builder, value.value, c"neg".as_ptr()) },
                        ty: value.ty,
                    }),
                    UnaryOp::Neg if value.ty == self.float_ty() => Ok(Value {
                        value: unsafe { LLVMBuildFNeg(self.builder, value.value, c"neg".as_ptr()) },
                        ty: value.ty,
                    }),
                    UnaryOp::Not if value.ty == self.bool_ty() => Ok(Value {
                        value: unsafe { LLVMBuildNot(self.builder, value.value, c"not".as_ptr()) },
                        ty: value.ty,
                    }),
                    _ => Err(Error::new("invalid unary operand")),
                }
            }
            Expr::Binary { left, op, right } => self.binary(*op, left, right, values),
            Expr::Call { callee, args } => self.call(callee, args, values),
            Expr::Field { base, name } => {
                let base = self.expr(base, values)?;
                let structure = self
                    .structs
                    .values()
                    .find(|structure| structure.ty == base.ty)
                    .cloned()
                    .ok_or_else(|| Error::new("field access requires a struct"))?;
                let (index, ty) = structure
                    .fields
                    .get(name)
                    .copied()
                    .ok_or_else(|| Error::new(format!("unknown field `{name}`")))?;
                Ok(Value {
                    value: unsafe {
                        LLVMBuildExtractValue(self.builder, base.value, index, c"field".as_ptr())
                    },
                    ty,
                })
            }
            Expr::Index { base, index } => {
                let base = self.expr(base, values)?;
                let index = self.expr(index, values)?;
                self.require_type(index, self.int_ty())?;
                let pointer = self.array_element(base, index)?;
                Ok(Value {
                    value: unsafe {
                        LLVMBuildLoad2(self.builder, self.int_ty(), pointer, c"array.load".as_ptr())
                    },
                    ty: self.int_ty(),
                })
            }
        }
    }

    fn if_expr(
        &mut self,
        condition: &Expr,
        then_body: &[Statement],
        else_body: &[Statement],
        values: &HashMap<String, Value>,
    ) -> Result<Value, Error> {
        let condition = self.expr(condition, values)?;
        self.require_type(condition, self.bool_ty())?;
        let function = unsafe { LLVMGetBasicBlockParent(LLVMGetInsertBlock(self.builder)) };
        let then_block =
            unsafe { LLVMAppendBasicBlockInContext(self.context, function, c"if.then".as_ptr()) };
        let else_block =
            unsafe { LLVMAppendBasicBlockInContext(self.context, function, c"if.else".as_ptr()) };
        let merge_block =
            unsafe { LLVMAppendBasicBlockInContext(self.context, function, c"if.merge".as_ptr()) };
        unsafe { LLVMBuildCondBr(self.builder, condition.value, then_block, else_block) };

        unsafe { LLVMPositionBuilderAtEnd(self.builder, then_block) };
        let then_value = self.block(then_body, values)?;
        let then_end = unsafe { LLVMGetInsertBlock(self.builder) };
        unsafe { LLVMBuildBr(self.builder, merge_block) };

        unsafe { LLVMPositionBuilderAtEnd(self.builder, else_block) };
        let else_value = self.block(else_body, values)?;
        self.require_type(else_value, then_value.ty)?;
        let else_end = unsafe { LLVMGetInsertBlock(self.builder) };
        unsafe { LLVMBuildBr(self.builder, merge_block) };

        unsafe { LLVMPositionBuilderAtEnd(self.builder, merge_block) };
        if then_value.ty == self.void_ty() {
            return Ok(then_value);
        }
        let phi = unsafe { LLVMBuildPhi(self.builder, then_value.ty, c"if.value".as_ptr()) };
        let mut incoming_values = [then_value.value, else_value.value];
        let mut incoming_blocks: [LLVMBasicBlockRef; 2] = [then_end, else_end];
        unsafe {
            LLVMAddIncoming(
                phi,
                incoming_values.as_mut_ptr(),
                incoming_blocks.as_mut_ptr(),
                incoming_values.len() as u32,
            )
        };
        Ok(Value {
            value: phi,
            ty: then_value.ty,
        })
    }

    fn method_call(
        &mut self,
        base: &Expr,
        name: &str,
        args: &[Expr],
        values: &HashMap<String, Value>,
    ) -> Result<Value, Error> {
        let base = self.expr(base, values)?;
        let type_name = self
            .structs
            .iter()
            .find(|(_, structure)| structure.ty == base.ty)
            .map(|(name, _)| name.clone())
            .or_else(|| {
                self.enums
                    .iter()
                    .find(|(_, enumeration)| enumeration.ty == base.ty)
                    .map(|(name, _)| name.clone())
            })
            .ok_or_else(|| Error::new("method call requires a concrete value"))?;
        let function = self
            .functions
            .get(&format!("{type_name}__{name}"))
            .copied()
            .ok_or_else(|| Error::new(format!("unknown method `{name}`")))?;
        let mut lowered = Vec::with_capacity(args.len() + 1);
        lowered.push(base);
        lowered.extend(
            args.iter()
                .map(|arg| self.expr(arg, values))
                .collect::<Result<Vec<_>, _>>()?,
        );
        let mut args = lowered.iter().map(|value| value.value).collect::<Vec<_>>();
        let value = unsafe {
            LLVMBuildCall2(
                self.builder,
                function.ty,
                function.value,
                args.as_mut_ptr(),
                args.len() as u32,
                c"method.call".as_ptr(),
            )
        };
        Ok(Value {
            value,
            ty: unsafe { LLVMGetReturnType(function.ty) },
        })
    }

    fn match_expr(
        &mut self,
        value: &Expr,
        arms: &[nerv_syntax::MatchArm],
        values: &HashMap<String, Value>,
    ) -> Result<Value, Error> {
        let value = self.expr(value, values)?;
        let enumeration = self
            .enums
            .values()
            .find(|enumeration| enumeration.ty == value.ty)
            .cloned()
            .ok_or_else(|| Error::new("match requires an enum value"))?;
        let tag = if enumeration.has_payload {
            unsafe { LLVMBuildExtractValue(self.builder, value.value, 0, c"enum.tag".as_ptr()) }
        } else {
            value.value
        };
        let payload = if enumeration.has_payload {
            Some(unsafe {
                LLVMBuildExtractValue(self.builder, value.value, 1, c"enum.payload".as_ptr())
            })
        } else {
            None
        };
        let function = unsafe { LLVMGetBasicBlockParent(LLVMGetInsertBlock(self.builder)) };
        let merge = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function, c"match.merge".as_ptr())
        };
        let fallback = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function, c"match.invalid".as_ptr())
        };
        let blocks = (0..arms.len())
            .map(|_| unsafe {
                LLVMAppendBasicBlockInContext(self.context, function, c"match.arm".as_ptr())
            })
            .collect::<Vec<_>>();
        let switch = unsafe { LLVMBuildSwitch(self.builder, tag, fallback, arms.len() as u32) };
        for (arm, block) in arms.iter().zip(&blocks) {
            let (tag, _) = enumeration
                .variants
                .get(&arm.variant)
                .copied()
                .ok_or_else(|| Error::new(format!("unknown enum variant `{}`", arm.variant)))?;
            unsafe { LLVMAddCase(switch, LLVMConstInt(self.int_ty(), tag as u64, 0), *block) };
        }

        let mut incoming = Vec::with_capacity(arms.len());
        let mut result_ty = None;
        for (arm, block) in arms.iter().zip(blocks) {
            unsafe { LLVMPositionBuilderAtEnd(self.builder, block) };
            let mut arm_values = values.clone();
            if let Some(name) = &arm.binding {
                let payload = payload.ok_or_else(|| Error::new("match variant has no payload"))?;
                let slot = unsafe {
                    LLVMBuildAlloca(self.builder, self.int_ty(), c"match.payload".as_ptr())
                };
                unsafe { LLVMBuildStore(self.builder, payload, slot) };
                arm_values.insert(
                    name.clone(),
                    Value {
                        value: slot,
                        ty: self.int_ty(),
                    },
                );
            }
            let result = self.expr(&arm.body, &arm_values)?;
            if let Some(expected) = result_ty {
                self.require_type(result, expected)?;
            } else {
                result_ty = Some(result.ty);
            }
            let end = unsafe { LLVMGetInsertBlock(self.builder) };
            unsafe { LLVMBuildBr(self.builder, merge) };
            incoming.push((result.value, end));
        }
        unsafe { LLVMPositionBuilderAtEnd(self.builder, fallback) };
        unsafe { LLVMBuildUnreachable(self.builder) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, merge) };
        let ty = result_ty.ok_or_else(|| Error::new("match needs at least one arm"))?;
        if ty == self.void_ty() {
            return Ok(Value {
                value: unsafe { LLVMGetUndef(ty) },
                ty,
            });
        }
        let phi = unsafe { LLVMBuildPhi(self.builder, ty, c"match.value".as_ptr()) };
        let mut values = incoming.iter().map(|(value, _)| *value).collect::<Vec<_>>();
        let mut blocks = incoming.iter().map(|(_, block)| *block).collect::<Vec<_>>();
        unsafe {
            LLVMAddIncoming(
                phi,
                values.as_mut_ptr(),
                blocks.as_mut_ptr(),
                values.len() as u32,
            )
        };
        Ok(Value { value: phi, ty })
    }

    fn block(
        &mut self,
        body: &[Statement],
        values: &HashMap<String, Value>,
    ) -> Result<Value, Error> {
        let mut values = values.clone();
        let mut last = None;
        for statement in body {
            match statement {
                Statement::Let {
                    name, ty, value, ..
                } => {
                    let value = self.expr(value, &values)?;
                    if let Some(ty) = ty {
                        if let Ok(ty) = self.ty(ty) {
                            self.require_type(value, ty)?;
                        }
                    }
                    let llvm_name = cstring(name)?;
                    let slot =
                        unsafe { LLVMBuildAlloca(self.builder, value.ty, llvm_name.as_ptr()) };
                    unsafe { LLVMBuildStore(self.builder, value.value, slot) };
                    values.insert(
                        name.clone(),
                        Value {
                            value: slot,
                            ty: value.ty,
                        },
                    );
                    last = None;
                }
                Statement::Assign { name, value } => {
                    self.assign(name, value, &values)?;
                    last = None;
                }
                Statement::AssignIndex { base, index, value } => {
                    self.assign_index(base, index, value, &values)?;
                    last = None;
                }
                Statement::AssignField { base, name, value } => {
                    self.assign_field(base, name, value, &values)?;
                    last = None;
                }
                Statement::While { condition, body } => {
                    self.while_loop(condition, body, &values)?;
                    last = None;
                }
                Statement::For {
                    name,
                    start,
                    end,
                    inclusive,
                    body,
                } => {
                    self.for_loop(name, start, end, *inclusive, body, &values)?;
                    last = None;
                }
                Statement::Expr(expr) => last = Some(self.expr(expr, &values)?),
                Statement::Return(_) => {
                    return Err(Error::new(
                        "return is not supported inside an if expression",
                    ));
                }
            }
        }
        last.ok_or_else(|| Error::new("if branch must end with a value"))
    }

    fn assign(
        &mut self,
        name: &str,
        expr: &Expr,
        values: &HashMap<String, Value>,
    ) -> Result<(), Error> {
        let slot = values
            .get(name)
            .copied()
            .ok_or_else(|| Error::new(format!("unknown value `{name}`")))?;
        let value = self.expr(expr, values)?;
        self.require_type(value, slot.ty)?;
        unsafe { LLVMBuildStore(self.builder, value.value, slot.value) };
        Ok(())
    }

    fn assign_index(
        &mut self,
        base: &Expr,
        index: &Expr,
        value: &Expr,
        values: &HashMap<String, Value>,
    ) -> Result<(), Error> {
        let base = self.expr(base, values)?;
        let index = self.expr(index, values)?;
        let value = self.expr(value, values)?;
        self.require_type(index, self.int_ty())?;
        self.require_type(value, self.int_ty())?;
        self.array_store(base, index, value)?;
        Ok(())
    }

    fn assign_field(
        &mut self,
        base: &Expr,
        name: &str,
        value: &Expr,
        values: &HashMap<String, Value>,
    ) -> Result<(), Error> {
        let Expr::Name(base_name) = base else {
            return Err(Error::new("field assignment requires a named value"));
        };
        let slot = values
            .get(base_name)
            .copied()
            .ok_or_else(|| Error::new(format!("unknown value `{base_name}`")))?;
        let structure = self
            .structs
            .values()
            .find(|structure| structure.ty == slot.ty)
            .cloned()
            .ok_or_else(|| Error::new("field assignment requires a struct"))?;
        let (index, ty) = structure
            .fields
            .get(name)
            .copied()
            .ok_or_else(|| Error::new(format!("unknown field `{name}`")))?;
        let value = self.expr(value, values)?;
        self.require_type(value, ty)?;
        let current =
            unsafe { LLVMBuildLoad2(self.builder, slot.ty, slot.value, c"load.struct".as_ptr()) };
        let updated = unsafe {
            LLVMBuildInsertValue(
                self.builder,
                current,
                value.value,
                index,
                c"field.set".as_ptr(),
            )
        };
        unsafe { LLVMBuildStore(self.builder, updated, slot.value) };
        Ok(())
    }

    fn array_element(&mut self, array: Value, index: Value) -> Result<LLVMValueRef, Error> {
        self.require_type(index, self.int_ty())?;
        Ok(unsafe {
            LLVMBuildInBoundsGEP2(
                self.builder,
                self.int_ty(),
                array.value,
                &mut [index.value] as *mut LLVMValueRef,
                1,
                c"array.element".as_ptr(),
            )
        })
    }

    fn array_store(&mut self, array: Value, index: Value, value: Value) -> Result<(), Error> {
        self.require_type(value, self.int_ty())?;
        let pointer = self.array_element(array, index)?;
        unsafe { LLVMBuildStore(self.builder, value.value, pointer) };
        Ok(())
    }

    fn while_loop(
        &mut self,
        condition: &Expr,
        body: &[Statement],
        values: &HashMap<String, Value>,
    ) -> Result<(), Error> {
        let function = unsafe { LLVMGetBasicBlockParent(LLVMGetInsertBlock(self.builder)) };
        let condition_block = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function, c"while.condition".as_ptr())
        };
        let body_block = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function, c"while.body".as_ptr())
        };
        let exit_block = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function, c"while.exit".as_ptr())
        };
        unsafe { LLVMBuildBr(self.builder, condition_block) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, condition_block) };
        let condition = self.expr(condition, values)?;
        self.require_type(condition, self.bool_ty())?;
        unsafe { LLVMBuildCondBr(self.builder, condition.value, body_block, exit_block) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, body_block) };
        self.loop_body(body, &mut values.clone())?;
        unsafe { LLVMBuildBr(self.builder, condition_block) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, exit_block) };
        Ok(())
    }

    fn for_loop(
        &mut self,
        name: &str,
        start: &Expr,
        end: &Expr,
        inclusive: bool,
        body: &[Statement],
        values: &HashMap<String, Value>,
    ) -> Result<(), Error> {
        let start = self.expr(start, values)?;
        let end = self.expr(end, values)?;
        self.require_type(start, self.int_ty())?;
        self.require_type(end, self.int_ty())?;
        let llvm_name = cstring(name)?;
        let slot = unsafe { LLVMBuildAlloca(self.builder, self.int_ty(), llvm_name.as_ptr()) };
        unsafe { LLVMBuildStore(self.builder, start.value, slot) };
        let function = unsafe { LLVMGetBasicBlockParent(LLVMGetInsertBlock(self.builder)) };
        let condition_block = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function, c"for.condition".as_ptr())
        };
        let body_block =
            unsafe { LLVMAppendBasicBlockInContext(self.context, function, c"for.body".as_ptr()) };
        let exit_block =
            unsafe { LLVMAppendBasicBlockInContext(self.context, function, c"for.exit".as_ptr()) };
        unsafe { LLVMBuildBr(self.builder, condition_block) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, condition_block) };
        let index =
            unsafe { LLVMBuildLoad2(self.builder, self.int_ty(), slot, c"for.index".as_ptr()) };
        let predicate = if inclusive {
            LLVMIntPredicate::LLVMIntSLE
        } else {
            LLVMIntPredicate::LLVMIntSLT
        };
        let condition = unsafe {
            LLVMBuildICmp(
                self.builder,
                predicate,
                index,
                end.value,
                c"for.test".as_ptr(),
            )
        };
        unsafe { LLVMBuildCondBr(self.builder, condition, body_block, exit_block) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, body_block) };
        let mut body_values = values.clone();
        body_values.insert(
            name.to_owned(),
            Value {
                value: slot,
                ty: self.int_ty(),
            },
        );
        self.loop_body(body, &mut body_values)?;
        let index =
            unsafe { LLVMBuildLoad2(self.builder, self.int_ty(), slot, c"for.next".as_ptr()) };
        let next = unsafe {
            LLVMBuildAdd(
                self.builder,
                index,
                LLVMConstInt(self.int_ty(), 1, 0),
                c"for.increment".as_ptr(),
            )
        };
        unsafe { LLVMBuildStore(self.builder, next, slot) };
        unsafe { LLVMBuildBr(self.builder, condition_block) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, exit_block) };
        Ok(())
    }

    fn loop_body(
        &mut self,
        body: &[Statement],
        values: &mut HashMap<String, Value>,
    ) -> Result<(), Error> {
        for statement in body {
            match statement {
                Statement::Let {
                    name, ty, value, ..
                } => {
                    let value = self.expr(value, values)?;
                    if let Some(ty) = ty {
                        if let Ok(ty) = self.ty(ty) {
                            self.require_type(value, ty)?;
                        }
                    }
                    let llvm_name = cstring(name)?;
                    let slot =
                        unsafe { LLVMBuildAlloca(self.builder, value.ty, llvm_name.as_ptr()) };
                    unsafe { LLVMBuildStore(self.builder, value.value, slot) };
                    values.insert(
                        name.clone(),
                        Value {
                            value: slot,
                            ty: value.ty,
                        },
                    );
                }
                Statement::Assign { name, value } => self.assign(name, value, values)?,
                Statement::AssignIndex { base, index, value } => {
                    self.assign_index(base, index, value, values)?
                }
                Statement::AssignField { base, name, value } => {
                    self.assign_field(base, name, value, values)?
                }
                Statement::While { condition, body } => self.while_loop(condition, body, values)?,
                Statement::For {
                    name,
                    start,
                    end,
                    inclusive,
                    body,
                } => self.for_loop(name, start, end, *inclusive, body, values)?,
                Statement::Expr(expr) => {
                    self.expr(expr, values)?;
                }
                Statement::Return(_) => {
                    return Err(Error::new("return is not supported inside a loop"));
                }
            }
        }
        Ok(())
    }

    fn binary(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        values: &HashMap<String, Value>,
    ) -> Result<Value, Error> {
        let left = self.expr(left, values)?;
        let right = self.expr(right, values)?;
        self.require_type(right, left.ty)?;
        let name = c"binary".as_ptr();
        let value = unsafe {
            match (op, left.ty == self.float_ty()) {
                (BinaryOp::Add, false) => LLVMBuildAdd(self.builder, left.value, right.value, name),
                (BinaryOp::Sub, false) => LLVMBuildSub(self.builder, left.value, right.value, name),
                (BinaryOp::Mul, false) => LLVMBuildMul(self.builder, left.value, right.value, name),
                (BinaryOp::Div, false) => {
                    LLVMBuildSDiv(self.builder, left.value, right.value, name)
                }
                (BinaryOp::Rem, false) => {
                    LLVMBuildSRem(self.builder, left.value, right.value, name)
                }
                (BinaryOp::Add, true) => LLVMBuildFAdd(self.builder, left.value, right.value, name),
                (BinaryOp::Sub, true) => LLVMBuildFSub(self.builder, left.value, right.value, name),
                (BinaryOp::Mul, true) => LLVMBuildFMul(self.builder, left.value, right.value, name),
                (BinaryOp::Div, true) => LLVMBuildFDiv(self.builder, left.value, right.value, name),
                (BinaryOp::Rem, true) => LLVMBuildFRem(self.builder, left.value, right.value, name),
                (BinaryOp::And, false) if left.ty == self.bool_ty() => {
                    LLVMBuildAnd(self.builder, left.value, right.value, name)
                }
                (BinaryOp::Or, false) if left.ty == self.bool_ty() => {
                    LLVMBuildOr(self.builder, left.value, right.value, name)
                }
                (BinaryOp::Eq, false) => LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntEQ,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::NotEq, false) => LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntNE,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::Lt, false) => LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntSLT,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::LtEq, false) => LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntSLE,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::Gt, false) => LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntSGT,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::GtEq, false) => LLVMBuildICmp(
                    self.builder,
                    LLVMIntPredicate::LLVMIntSGE,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::Eq, true) => LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealOEQ,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::NotEq, true) => LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealONE,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::Lt, true) => LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealOLT,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::LtEq, true) => LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealOLE,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::Gt, true) => LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealOGT,
                    left.value,
                    right.value,
                    name,
                ),
                (BinaryOp::GtEq, true) => LLVMBuildFCmp(
                    self.builder,
                    LLVMRealPredicate::LLVMRealOGE,
                    left.value,
                    right.value,
                    name,
                ),
                _ => return Err(Error::new("invalid binary operands")),
            }
        };
        let ty = match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq => self.bool_ty(),
            _ => left.ty,
        };
        Ok(Value { value, ty })
    }

    fn call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        values: &HashMap<String, Value>,
    ) -> Result<Value, Error> {
        if let Expr::Lambda { params, result, .. } = callee {
            if args.len() != params.len() {
                return Err(Error::new("wrong argument count"));
            }
            let lambda = self.expr(callee, values)?;
            let mut parameter_types = params
                .iter()
                .map(|param| self.ty(&param.ty))
                .collect::<Result<Vec<_>, _>>()?;
            let result_ty = self.ty(result)?;
            let function_ty = unsafe {
                LLVMFunctionType(
                    result_ty,
                    parameter_types.as_mut_ptr(),
                    parameter_types.len() as u32,
                    0,
                )
            };
            let args = args
                .iter()
                .map(|arg| self.expr(arg, values))
                .collect::<Result<Vec<_>, _>>()?;
            for (value, expected) in args.iter().zip(parameter_types) {
                self.require_type(*value, expected)?;
            }
            let mut values = args.iter().map(|value| value.value).collect::<Vec<_>>();
            let value = unsafe {
                LLVMBuildCall2(
                    self.builder,
                    function_ty,
                    lambda.value,
                    values.as_mut_ptr(),
                    values.len() as u32,
                    c"lambda.call".as_ptr(),
                )
            };
            return Ok(Value {
                value,
                ty: result_ty,
            });
        }
        let Some(name) = callee_name(callee) else {
            return Err(Error::new("only direct function calls can be lowered"));
        };
        if matches!(name.as_str(), "print" | "println") {
            if args.len() != 1 {
                return Err(Error::new("print expects one argument"));
            }
            let value = self.expr(&args[0], values)?;
            let suffix = if name == "print" { "print" } else { "println" };
            let runtime = if value.ty == self.int_ty() {
                format!("nerv_{suffix}_i64")
            } else if value.ty == self.float_ty() {
                format!("nerv_{suffix}_f64")
            } else if value.ty == self.bool_ty() {
                format!("nerv_{suffix}_bool")
            } else if value.ty == self.string_ty() {
                format!("nerv_{suffix}_string")
            } else {
                return Err(Error::new("unsupported print value"));
            };
            return self.call_external(&runtime, self.void_ty(), &[value]);
        }
        let lowered = args
            .iter()
            .map(|arg| self.expr(arg, values))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(value) = self.enum_variant(&name, &lowered)? {
            return Ok(value);
        }
        if let Some(result) = self.runtime_call(&name, &lowered)? {
            return Ok(result);
        }
        if let Expr::Name(name) = callee
            && let Some(callee) = values.get(name)
            && let Some(function_ty) = self.function_slots.get(&(callee.value as usize)).copied()
        {
            let mut args = lowered.iter().map(|value| value.value).collect::<Vec<_>>();
            let value = unsafe {
                LLVMBuildCall2(
                    self.builder,
                    function_ty,
                    LLVMBuildLoad2(self.builder, callee.ty, callee.value, c"load.fn".as_ptr()),
                    args.as_mut_ptr(),
                    args.len() as u32,
                    c"indirect.call".as_ptr(),
                )
            };
            return Ok(Value {
                value,
                ty: unsafe { LLVMGetReturnType(function_ty) },
            });
        }
        let function = self
            .functions
            .get(&name)
            .copied()
            .ok_or_else(|| Error::new(format!("unknown function `{name}`")))?;
        let mut lowered = lowered
            .into_iter()
            .map(|value| value.value)
            .collect::<Vec<_>>();
        let value = unsafe {
            LLVMBuildCall2(
                self.builder,
                function.ty,
                function.value,
                lowered.as_mut_ptr(),
                lowered.len() as u32,
                c"call".as_ptr(),
            )
        };
        let ty = unsafe { LLVMGetReturnType(function.ty) };
        Ok(Value { value, ty })
    }

    fn runtime_call(&mut self, name: &str, args: &[Value]) -> Result<Option<Value>, Error> {
        let result = match (name, args) {
            ("nerv.math.sqrt", [value]) if value.ty == self.float_ty() => {
                self.call_external("nerv_sqrt", self.float_ty(), args)?
            }
            ("nerv.math.pow", [left, right])
                if left.ty == self.float_ty() && right.ty == self.float_ty() =>
            {
                self.call_external("nerv_pow", self.float_ty(), args)?
            }
            ("nerv.math.abs", [value]) if value.ty == self.int_ty() => {
                self.call_external("nerv_abs_i64", self.int_ty(), args)?
            }
            ("nerv.math.abs", [value]) if value.ty == self.float_ty() => {
                self.call_external("nerv_abs_f64", self.float_ty(), args)?
            }
            ("nerv.string.len", [value]) if value.ty == self.string_ty() => {
                self.call_external("nerv_string_len", self.int_ty(), args)?
            }
            ("nerv.string.concat", [left, right])
                if left.ty == self.string_ty() && right.ty == self.string_ty() =>
            {
                self.call_external("nerv_string_concat", self.string_ty(), args)?
            }
            ("nerv.string.is_empty", [value]) if value.ty == self.string_ty() => {
                self.call_external("nerv_str_is_empty", self.int_ty(), args)?
            }
            ("nerv.string.char_count", [value]) if value.ty == self.string_ty() => {
                self.call_external("nerv_str_char_count", self.int_ty(), args)?
            }
            ("nerv.string.to_upper", [value]) if value.ty == self.string_ty() => {
                self.call_external("nerv_str_to_upper", self.string_ty(), args)?
            }
            ("nerv.string.to_lower", [value]) if value.ty == self.string_ty() => {
                self.call_external("nerv_str_to_lower", self.string_ty(), args)?
            }
            ("nerv.string.trim", [value]) if value.ty == self.string_ty() => {
                self.call_external("nerv_str_trim", self.string_ty(), args)?
            }
            ("nerv.string.contains", [value, needle])
                if value.ty == self.string_ty() && needle.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_contains", self.int_ty(), args)?
            }
            ("nerv.string.starts_with", [value, prefix])
                if value.ty == self.string_ty() && prefix.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_starts_with", self.int_ty(), args)?
            }
            ("nerv.string.ends_with", [value, suffix])
                if value.ty == self.string_ty() && suffix.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_ends_with", self.int_ty(), args)?
            }
            ("nerv.string.find", [value, needle])
                if value.ty == self.string_ty() && needle.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_find", self.int_ty(), args)?
            }
            ("nerv.string.split", [value, separator])
                if value.ty == self.string_ty() && separator.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_split", self.string_ty(), args)?
            }
            ("nerv.string.split_once", [value, separator])
                if value.ty == self.string_ty() && separator.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_split_once", self.string_ty(), args)?
            }
            ("nerv.string.char_at", [value, index])
                if value.ty == self.string_ty() && index.ty == self.int_ty() =>
            {
                self.call_external("nerv_str_char_at", self.int_ty(), args)?
            }
            ("nerv.string.repeat", [value, count])
                if value.ty == self.string_ty() && count.ty == self.int_ty() =>
            {
                self.call_external("nerv_str_repeat", self.string_ty(), args)?
            }
            ("nerv.string.replace", [value, from, to])
                if value.ty == self.string_ty()
                    && from.ty == self.string_ty()
                    && to.ty == self.string_ty() =>
            {
                self.call_external("nerv_str_replace", self.string_ty(), args)?
            }
            ("nerv.string.slice", [value, start, end])
                if value.ty == self.string_ty()
                    && start.ty == self.int_ty()
                    && end.ty == self.int_ty() =>
            {
                self.call_external("nerv_str_slice", self.string_ty(), args)?
            }
            ("nerv.arrays.map", [array, length, callback])
                if array.ty == self.array_ty()
                    && length.ty == self.int_ty()
                    && callback.ty == self.array_ty() =>
            {
                self.call_external("nerv_array_i64_map", self.array_ty(), args)?
            }
            ("nerv.arrays.reduce", [array, length, initial, callback])
                if array.ty == self.array_ty()
                    && length.ty == self.int_ty()
                    && initial.ty == self.int_ty()
                    && callback.ty == self.array_ty() =>
            {
                self.call_external("nerv_array_i64_reduce", self.int_ty(), args)?
            }
            ("nerv.arrays.new", [length, initial])
                if length.ty == self.int_ty() && initial.ty == self.int_ty() =>
            {
                let array = self.call_external("nerv_array_i64_new", self.array_ty(), args)?;
                self.call_external("nerv_array_i64_data", self.array_ty(), &[array])?
            }
            ("nerv.random", [maximum]) if maximum.ty == self.int_ty() => {
                self.call_external("nerv_random_i64", self.int_ty(), args)?
            }
            ("nerv.collections.vec_new", []) => {
                self.call_external("nerv_vec_new", self.int_ty(), args)?
            }
            ("nerv.collections.vec_push", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_push", self.void_ty(), args)?
            }
            ("nerv.collections.vec_pop", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_vec_pop", self.int_ty(), args)?
            }
            ("nerv.collections.vec_get", [handle, index])
                if handle.ty == self.int_ty() && index.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_get", self.int_ty(), args)?
            }
            ("nerv.collections.vec_set", [handle, index, value])
                if handle.ty == self.int_ty()
                    && index.ty == self.int_ty()
                    && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_set", self.void_ty(), args)?
            }
            ("nerv.collections.vec_len", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_vec_len", self.int_ty(), args)?
            }
            ("nerv.collections.vec_is_empty", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_vec_is_empty", self.int_ty(), args)?
            }
            ("nerv.collections.vec_insert", [handle, index, value])
                if handle.ty == self.int_ty()
                    && index.ty == self.int_ty()
                    && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_insert", self.void_ty(), args)?
            }
            ("nerv.collections.vec_remove", [handle, index])
                if handle.ty == self.int_ty() && index.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_remove", self.int_ty(), args)?
            }
            ("nerv.collections.vec_clear", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_vec_clear", self.void_ty(), args)?
            }
            ("nerv.collections.vec_free", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_vec_free", self.void_ty(), args)?
            }
            ("nerv.collections.vec_contains", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_contains", self.int_ty(), args)?
            }
            ("nerv.collections.vec_index_of", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_vec_index_of", self.int_ty(), args)?
            }
            ("nerv.collections.vec_as_ptr", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_vec_as_ptr", self.int_ty(), args)?
            }
            ("nerv.collections.map_new", []) => {
                self.call_external("nerv_map_new", self.int_ty(), args)?
            }
            ("nerv.collections.map_insert", [handle, key, value])
                if handle.ty == self.int_ty()
                    && key.ty == self.string_ty()
                    && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_map_insert", self.int_ty(), args)?
            }
            ("nerv.collections.map_get", [handle, key])
                if handle.ty == self.int_ty() && key.ty == self.string_ty() =>
            {
                self.call_external("nerv_map_get", self.int_ty(), args)?
            }
            ("nerv.collections.map_remove", [handle, key])
                if handle.ty == self.int_ty() && key.ty == self.string_ty() =>
            {
                self.call_external("nerv_map_remove", self.int_ty(), args)?
            }
            ("nerv.collections.map_contains_key", [handle, key])
                if handle.ty == self.int_ty() && key.ty == self.string_ty() =>
            {
                self.call_external("nerv_map_contains_key", self.int_ty(), args)?
            }
            ("nerv.collections.map_len", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_map_len", self.int_ty(), args)?
            }
            ("nerv.collections.map_free", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_map_free", self.void_ty(), args)?
            }
            ("nerv.collections.set_new", []) => {
                self.call_external("nerv_set_new", self.int_ty(), args)?
            }
            ("nerv.collections.set_insert", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_set_insert", self.int_ty(), args)?
            }
            ("nerv.collections.set_contains", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_set_contains", self.int_ty(), args)?
            }
            ("nerv.collections.set_remove", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_set_remove", self.int_ty(), args)?
            }
            ("nerv.collections.set_len", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_set_len", self.int_ty(), args)?
            }
            ("nerv.collections.set_free", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_set_free", self.void_ty(), args)?
            }
            ("nerv.collections.deque_new", []) => {
                self.call_external("nerv_deque_new", self.int_ty(), args)?
            }
            ("nerv.collections.deque_push_back", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_deque_push_back", self.void_ty(), args)?
            }
            ("nerv.collections.deque_push_front", [handle, value])
                if handle.ty == self.int_ty() && value.ty == self.int_ty() =>
            {
                self.call_external("nerv_deque_push_front", self.void_ty(), args)?
            }
            ("nerv.collections.deque_pop_back", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_deque_pop_back", self.int_ty(), args)?
            }
            ("nerv.collections.deque_pop_front", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_deque_pop_front", self.int_ty(), args)?
            }
            ("nerv.collections.deque_len", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_deque_len", self.int_ty(), args)?
            }
            ("nerv.collections.deque_free", [handle]) if handle.ty == self.int_ty() => {
                self.call_external("nerv_deque_free", self.void_ty(), args)?
            }
            (
                "nerv.math.sqrt"
                | "nerv.math.pow"
                | "nerv.math.abs"
                | "nerv.string.len"
                | "nerv.string.concat"
                | "nerv.arrays.new"
                | "nerv.random"
                | "nerv.collections.vec_new"
                | "nerv.collections.vec_push"
                | "nerv.collections.vec_pop"
                | "nerv.collections.vec_get"
                | "nerv.collections.vec_set"
                | "nerv.collections.vec_len"
                | "nerv.collections.vec_is_empty"
                | "nerv.collections.vec_insert"
                | "nerv.collections.vec_remove"
                | "nerv.collections.vec_clear"
                | "nerv.collections.vec_free"
                | "nerv.collections.vec_contains"
                | "nerv.collections.vec_index_of"
                | "nerv.collections.vec_as_ptr"
                | "nerv.collections.map_new"
                | "nerv.collections.map_insert"
                | "nerv.collections.map_get"
                | "nerv.collections.map_remove"
                | "nerv.collections.map_contains_key"
                | "nerv.collections.map_len"
                | "nerv.collections.map_free"
                | "nerv.collections.set_new"
                | "nerv.collections.set_insert"
                | "nerv.collections.set_contains"
                | "nerv.collections.set_remove"
                | "nerv.collections.set_len"
                | "nerv.collections.set_free"
                | "nerv.collections.deque_new"
                | "nerv.collections.deque_push_back"
                | "nerv.collections.deque_push_front"
                | "nerv.collections.deque_pop_back"
                | "nerv.collections.deque_pop_front"
                | "nerv.collections.deque_len"
                | "nerv.collections.deque_free",
                _,
            ) => {
                return Err(Error::new("invalid runtime arguments"));
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }

    fn enum_variant(&self, name: &str, args: &[Value]) -> Result<Option<Value>, Error> {
        let (enum_name, variant_name) = name.rsplit_once('.').unwrap_or(("", name));
        let enumeration = if enum_name.is_empty() {
            let Some(enumeration) = self
                .enums
                .values()
                .find(|enumeration| enumeration.variants.contains_key(variant_name))
            else {
                return Ok(None);
            };
            enumeration
        } else {
            let Some(enumeration) = self.enums.get(enum_name) else {
                return Ok(None);
            };
            enumeration
        };
        let Some((tag, payload)) = enumeration.variants.get(variant_name).copied() else {
            return Ok(None);
        };
        if !enumeration.has_payload {
            if !args.is_empty() {
                return Err(Error::new("enum variant does not accept arguments"));
            }
            return Ok(Some(Value {
                value: unsafe { LLVMConstInt(enumeration.ty, tag as u64, 0) },
                ty: enumeration.ty,
            }));
        }
        if args.len() != payload as usize || args.iter().any(|value| value.ty != self.int_ty()) {
            return Err(Error::new("invalid enum payload"));
        }
        let mut value = unsafe { LLVMGetUndef(enumeration.ty) };
        value = unsafe {
            LLVMBuildInsertValue(
                self.builder,
                value,
                LLVMConstInt(self.int_ty(), tag as u64, 0),
                0,
                c"enum.tag".as_ptr(),
            )
        };
        let payload = args
            .first()
            .map(|value| value.value)
            .unwrap_or_else(|| unsafe { LLVMConstInt(self.int_ty(), 0, 0) });
        value = unsafe {
            LLVMBuildInsertValue(self.builder, value, payload, 1, c"enum.payload".as_ptr())
        };
        Ok(Some(Value {
            value,
            ty: enumeration.ty,
        }))
    }

    fn call_external(
        &mut self,
        name: &str,
        result: LLVMTypeRef,
        args: &[Value],
    ) -> Result<Value, Error> {
        let function = if let Some(function) = self.functions.get(name).copied() {
            function
        } else {
            let mut params = args.iter().map(|value| value.ty).collect::<Vec<_>>();
            let ty =
                unsafe { LLVMFunctionType(result, params.as_mut_ptr(), params.len() as u32, 0) };
            let llvm_name = cstring(name)?;
            let value = unsafe { LLVMAddFunction(self.module, llvm_name.as_ptr(), ty) };
            let function = FunctionValue { value, ty };
            self.functions.insert(name.to_owned(), function);
            function
        };
        let mut args = args.iter().map(|value| value.value).collect::<Vec<_>>();
        let call_name = if result == self.void_ty() {
            c"".as_ptr()
        } else {
            c"runtime.call".as_ptr()
        };
        let value = unsafe {
            LLVMBuildCall2(
                self.builder,
                function.ty,
                function.value,
                args.as_mut_ptr(),
                args.len() as u32,
                call_name,
            )
        };
        Ok(Value { value, ty: result })
    }

    fn require_type(&self, value: Value, ty: LLVMTypeRef) -> Result<(), Error> {
        if value.ty == ty {
            Ok(())
        } else {
            Err(Error::new("type mismatch during LLVM lowering"))
        }
    }

    fn ty(&self, ty: &Type) -> Result<LLVMTypeRef, Error> {
        Ok(match ty {
            Type::Void => self.void_ty(),
            Type::Bool => self.bool_ty(),
            Type::Int => self.int_ty(),
            Type::Float => self.float_ty(),
            Type::String => self.string_ty(),
            Type::Array(element) if **element == Type::Int => self.array_ty(),
            Type::Array(_) => return Err(Error::new("only i64 arrays are supported")),
            Type::Function { .. } => self.array_ty(),
            Type::Named(name) => self
                .structs
                .get(name)
                .map(|structure| structure.ty)
                .or_else(|| self.enums.get(name).map(|enumeration| enumeration.ty))
                .ok_or_else(|| Error::new(format!("unknown type `{name}`")))?,
        })
    }

    fn function_ty(&self, ty: &Type) -> Result<LLVMTypeRef, Error> {
        let Type::Function { params, result } = ty else {
            return Err(Error::new("expected a function type"));
        };
        let mut params = params
            .iter()
            .map(|param| self.ty(param))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(unsafe {
            LLVMFunctionType(
                self.ty(result)?,
                params.as_mut_ptr(),
                params.len() as u32,
                0,
            )
        })
    }

    fn bool_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMInt1TypeInContext(self.context) }
    }

    fn void_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMVoidTypeInContext(self.context) }
    }

    fn int_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMInt64TypeInContext(self.context) }
    }

    fn float_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMDoubleTypeInContext(self.context) }
    }

    fn string_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMPointerTypeInContext(self.context, 0) }
    }

    fn array_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMPointerTypeInContext(self.context, 0) }
    }
}

fn cstring(value: &str) -> Result<CString, Error> {
    CString::new(value).map_err(|_| Error::new("NUL byte in LLVM identifier"))
}

fn callee_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.clone()),
        Expr::Field { base, name } => Some(format!("{}.{}", callee_name(base)?, name)),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use nerv_syntax::parse;

    #[test]
    fn creates_a_context() {
        let context = Context::new();
        assert!(!context.as_raw().is_null());
    }

    #[test]
    fn runs_functions_and_calls() {
        let module = parse(
            "fn add(left: i64, right: i64) -> i64 => left + right\nfn main() -> i64\n    let value = add(20, 22)\n    return value\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn runs_conditional_control_flow() {
        let module = parse(
            "fn choose(flag: bool, left: i64, right: i64) -> i64\n    if flag\n        left\n    else\n        right\nfn main() -> i64 => choose(true, 42, 0)\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn calls_the_runtime() {
        let module =
            parse("fn main() -> i64\n    println(\"Nerv JIT\")\n    println(42)\n    return 0\n")
                .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 0);
    }

    #[test]
    fn calls_runtime_namespaces() {
        let module = parse(
            "fn main() -> i64\n    println(nerv.math.sqrt(9.0))\n    println(nerv.string.concat(\"Ne\", \"rv\"))\n    return nerv.string.len(\"Nerv\")\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 4);
    }

    #[test]
    fn runs_struct_values() {
        let module = parse(
            "struct Pair\n    left: i64\n    right: i64\nfn sum(pair: Pair) -> i64 => pair.left + pair.right\nfn main() -> i64 => sum(Pair { left: 19, right: 23 })\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn updates_struct_fields() {
        let module = parse(
            "struct Pair\n    left: i64\n    right: i64\nfn main() -> i64\n    let mut pair = Pair { left: 19, right: 0 }\n    pair.right = 23\n    return pair.left + pair.right\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn lowers_non_capturing_lambdas() {
        let module = parse(
            "fn main() -> i64\n    let callback: fn(i64) -> i64 = fn(value: i64) -> i64 => value + 1\n    return 42\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn calls_non_capturing_lambdas() {
        let module =
            parse("fn main() -> i64 => (fn(value: i64) -> i64 => value + 1)(41)\n").unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn calls_function_values() {
        let module = parse(
            "fn add_one(value: i64) -> i64 => value + 1\nfn apply(callback: fn(i64) -> i64, value: i64) -> i64 => callback(value)\nfn main() -> i64 => apply(add_one, 41)\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn maps_and_reduces_function_values() {
        let module = parse(
            "fn double(value: i64) -> i64 => value * 2\nfn add(total: i64, value: i64) -> i64 => total + value\nfn main() -> i64\n    let values: []i64 = [1, 2, 3]\n    let doubled = nerv.arrays.map(values, 3, double)\n    return nerv.arrays.reduce(doubled, 3, 0, add)\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 12);
    }

    #[test]
    fn runs_mutable_loops() {
        let module = parse(
            "fn main() -> i64\n    let mut total = 0\n    for value in 1..=4\n        total = total + value\n    while total < 42\n        total = total + 1\n    return total\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn runs_void_main() {
        let module = parse("fn main() -> void\n    println(\"Nerv\")\n").unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 0);
    }

    #[test]
    fn runs_fieldless_enums() {
        let module = parse(
            "enum Flag\n    Off\n    On\nfn main() -> i64\n    let state: Flag = On()\n    if state == Off()\n        0\n    else\n        42\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn constructs_i64_enum_payloads() {
        let module = parse(
            "enum Choice\n    Some(i64)\n    None\nfn make() -> Choice => Some(4)\nfn main() -> i64\n    make()\n    return 42\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn matches_enum_payloads() {
        let module = parse(
            "enum Choice\n    Some(i64)\n    None\nfn main() -> i64\n    match Some(42)\n        Some(value) => value\n        None => 0\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn dispatches_implementation_methods() {
        let module = parse(
            "trait Drawable\n    fn draw(self) -> i64\n\nenum Light\n    On\n\nimpl Light for Drawable\n    fn draw(self) -> i64 => 42\n\nfn main() -> i64\n    let light = On()\n    let drawable: Drawable = light\n    drawable:draw()\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 42);
    }

    #[test]
    fn runs_i64_arrays() {
        let module = parse(
            "fn main() -> i64\n    let values: []i64 = [1, 2, 3]\n    values[1] = 40\n    return values[0] + values[1] + values[2]\n",
        )
        .unwrap();
        assert_eq!(run_main(&[module]).unwrap(), 44);
    }

    #[test]
    fn runs_heavy_loop_shape() {
        let module = parse(
            "fn main() -> i64\n    let r = nerv.random(4)\n    let values: []i64 = nerv.arrays.new(4, 0)\n    for i in 0..4\n        for j in 0..16\n            values[i] = values[i] + (j % 3)\n        values[i] = values[i] + r\n    return values[0]\n",
        )
        .unwrap();
        assert!((15..=18).contains(&run_main(&[module]).unwrap()));
    }
}
