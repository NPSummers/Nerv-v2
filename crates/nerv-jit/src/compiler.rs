//! LLVM emission consumes only checked IR. Names, overloads, member indices,
//! nominal identities, conversions, and function signatures are already resolved.
use super::*;
use llvm_sys::{
    LLVMIntPredicate as IP, LLVMRealPredicate as FP,
    analysis::{LLVMVerifierFailureAction, LLVMVerifyModule},
    core::*,
    prelude::*,
};
use nerv_sema::ir::{self, BinaryOp as B, ExprKind as E, StatementKind as S, UnaryOp as U};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
struct Value {
    value: LLVMValueRef,
    ty: LLVMTypeRef,
}
#[derive(Clone, Copy)]
struct Function {
    value: LLVMValueRef,
    ty: LLVMTypeRef,
}
type Slots = HashMap<SymbolId, Value>;

struct Compiler {
    context: LLVMContextRef,
    module: LLVMModuleRef,
    builder: LLVMBuilderRef,
    types: HashMap<SymbolId, LLVMTypeRef>,
    payloads: HashMap<SymbolId, bool>,
    functions: HashMap<SymbolId, Function>,
    externals: HashMap<String, Function>,
    ranges: HashMap<SymbolId, super::range::Range>,
    array_lengths: HashMap<SymbolId, i64>,
    immutable_depths: HashMap<SymbolId, usize>,
    loop_depth: usize,
    mode: BuildMode,
    assigned: HashSet<SymbolId>,
    reciprocal_loop: bool,
}

pub(super) fn generate(
    program: &CheckedProgram,
    mode: BuildMode,
    standalone: bool,
) -> Result<GeneratedModule, Error> {
    generate_for_cpu(
        program,
        mode,
        standalone,
        if standalone {
            CpuTarget::Generic
        } else {
            CpuTarget::Native
        },
    )
}

pub(super) fn generate_for_cpu(
    program: &CheckedProgram,
    mode: BuildMode,
    standalone: bool,
    cpu: CpuTarget,
) -> Result<GeneratedModule, Error> {
    unsafe {
        let context = LLVMContextCreate();
        if context.is_null() {
            return Err(Error::new("could not create an LLVM context"));
        }
        let module = LLVMModuleCreateWithNameInContext(c"nerv".as_ptr(), context);
        let builder = LLVMCreateBuilderInContext(context);
        let mut compiler = Compiler {
            context,
            module,
            builder,
            types: HashMap::new(),
            payloads: HashMap::new(),
            functions: HashMap::new(),
            externals: HashMap::new(),
            ranges: HashMap::new(),
            array_lengths: HashMap::new(),
            immutable_depths: HashMap::new(),
            loop_depth: 0,
            mode,
            assigned: HashSet::new(),
            reciprocal_loop: false,
        };
        let result = compiler
            .compile(program, standalone)
            .and_then(|()| super::optimize::optimize(module, mode, cpu, standalone));
        LLVMDisposeBuilder(builder);
        if let Err(error) = result {
            LLVMDisposeModule(module);
            LLVMContextDispose(context);
            Err(error)
        } else {
            Ok(GeneratedModule { module, context })
        }
    }
}

impl Compiler {
    fn compile(&mut self, program: &CheckedProgram, standalone: bool) -> Result<(), Error> {
        for structure in program.structs() {
            let name = cstring(&format!("struct_{}", structure.id.0))?;
            self.types.insert(structure.id, unsafe {
                LLVMStructCreateNamed(self.context, name.as_ptr())
            });
        }
        for enumeration in program.enums() {
            let payload = enumeration.has_payload();
            let ty = if payload {
                let name = cstring(&format!("enum_{}", enumeration.id.0))?;
                unsafe { LLVMStructCreateNamed(self.context, name.as_ptr()) }
            } else {
                self.int_ty()
            };
            self.types.insert(enumeration.id, ty);
            self.payloads.insert(enumeration.id, payload);
        }
        for enumeration in program.enums().iter().filter(|e| e.has_payload()) {
            let mut fields = vec![self.int_ty()];
            fields.extend(enumeration.variants.iter().map(|variant| {
                variant
                    .payload
                    .as_ref()
                    .map_or(self.int_ty(), |ty| self.ty(ty))
            }));
            unsafe {
                LLVMStructSetBody(
                    self.types[&enumeration.id],
                    fields.as_mut_ptr(),
                    fields.len() as u32,
                    0,
                );
            }
        }
        for structure in program.structs() {
            let mut fields = structure
                .fields
                .iter()
                .map(|field| self.ty(&field.ty))
                .collect::<Vec<_>>();
            unsafe {
                LLVMStructSetBody(
                    self.types[&structure.id],
                    fields.as_mut_ptr(),
                    fields.len() as u32,
                    0,
                )
            };
        }
        for function in program.functions() {
            let signature = self.signature(
                &function
                    .params
                    .iter()
                    .map(|p| p.ty.clone())
                    .collect::<Vec<_>>(),
                &function.result,
            );
            let name = cstring(&symbol(function.id))?;
            let value = unsafe { LLVMAddFunction(self.module, name.as_ptr(), signature) };
            self.functions.insert(
                function.id,
                Function {
                    value,
                    ty: signature,
                },
            );
        }
        for function in program.functions() {
            self.function(function)
                .map_err(|error| error.at(function.span))?;
        }
        if standalone {
            self.entry_adapter(program)?;
        }
        unsafe {
            let mut message = ptr::null_mut();
            if LLVMVerifyModule(
                self.module,
                LLVMVerifierFailureAction::LLVMReturnStatusAction,
                &mut message,
            ) != 0
            {
                let text = if message.is_null() {
                    "invalid LLVM module".into()
                } else {
                    let text = CStr::from_ptr(message).to_string_lossy().into_owned();
                    LLVMDisposeMessage(message);
                    text
                };
                return Err(Error::new(text));
            }
        }
        Ok(())
    }

    fn entry_adapter(&mut self, program: &CheckedProgram) -> Result<(), Error> {
        let main = program
            .main()
            .ok_or_else(|| Error::new("missing `main` function"))?;
        let entry = self.functions[&main];
        let int32 = unsafe { LLVMInt32TypeInContext(self.context) };
        let signature = unsafe { LLVMFunctionType(int32, ptr::null_mut(), 0, 0) };
        let function = unsafe { LLVMAddFunction(self.module, c"main".as_ptr(), signature) };
        let block =
            unsafe { LLVMAppendBasicBlockInContext(self.context, function, c"entry".as_ptr()) };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, block) };
        self.external("nerv_runtime_abi_v2", self.void_ty(), &[])?;
        let runtime = if program.process_entry() {
            if program.function(main).result == Type::Int {
                "nerv_process_i64"
            } else {
                "nerv_process_void"
            }
        } else if program.function(main).result == Type::Int {
            "nerv_start_i64"
        } else {
            "nerv_start_void"
        };
        let result = self.external(
            runtime,
            int32,
            &[Value {
                value: entry.value,
                ty: self.pointer_ty(),
            }],
        )?;
        unsafe { LLVMBuildRet(self.builder, result.value) };
        Ok(())
    }

    fn ty(&self, ty: &Type) -> LLVMTypeRef {
        match ty {
            Type::Void => self.void_ty(),
            Type::Bool => self.bool_ty(),
            Type::Int => self.int_ty(),
            Type::Integer { bits, .. } => unsafe { LLVMIntTypeInContext(self.context, *bits) },
            Type::Float32 => unsafe { LLVMFloatTypeInContext(self.context) },
            Type::Float => self.float_ty(),
            Type::String | Type::Array(_) | Type::Function { .. } => self.pointer_ty(),
            Type::Named(id) => self.types[id],
        }
    }

    fn signature(&self, params: &[Type], result: &Type) -> LLVMTypeRef {
        let mut params = params.iter().map(|ty| self.ty(ty)).collect::<Vec<_>>();
        unsafe { LLVMFunctionType(self.ty(result), params.as_mut_ptr(), params.len() as u32, 0) }
    }

    fn function(&mut self, source: &ir::Function) -> Result<(), Error> {
        self.ranges.clear();
        self.array_lengths.clear();
        self.immutable_depths.clear();
        self.assigned = super::range::assigned_locals(&source.body);
        let function = self.functions[&source.id];
        let entry = unsafe {
            LLVMAppendBasicBlockInContext(self.context, function.value, c"entry".as_ptr())
        };
        unsafe { LLVMPositionBuilderAtEnd(self.builder, entry) };
        let mut slots = Slots::new();
        for (index, parameter) in source.params.iter().enumerate() {
            let value = Value {
                value: unsafe { LLVMGetParam(function.value, index as u32) },
                ty: self.ty(&parameter.ty),
            };
            self.bind(parameter, value, &mut slots)?;
        }
        let result = self.block(&source.body, &mut slots)?;
        if unsafe { LLVMGetBasicBlockTerminator(LLVMGetInsertBlock(self.builder)).is_null() } {
            unsafe {
                if source.result == Type::Void {
                    LLVMBuildRetVoid(self.builder);
                } else {
                    LLVMBuildRet(self.builder, result.value);
                }
            }
        }
        Ok(())
    }

    fn bind(
        &mut self,
        binding: &ir::Binding,
        value: Value,
        slots: &mut Slots,
    ) -> Result<(), Error> {
        let name = cstring(&format!("local_{}", binding.id.0))?;
        let ty = self.ty(&binding.ty);
        let pointer = self.slot(ty, &name);
        unsafe { LLVMBuildStore(self.builder, value.value, pointer) };
        slots.insert(binding.id, Value { value: pointer, ty });
        if !self.assigned.contains(&binding.id) {
            self.immutable_depths.insert(binding.id, self.loop_depth);
        }
        Ok(())
    }

    fn slot(&self, ty: LLVMTypeRef, name: &CStr) -> LLVMValueRef {
        unsafe {
            let allocator = LLVMCreateBuilderInContext(self.context);
            let function = LLVMGetBasicBlockParent(LLVMGetInsertBlock(self.builder));
            let entry = LLVMGetEntryBasicBlock(function);
            let first = LLVMGetFirstInstruction(entry);
            if first.is_null() {
                LLVMPositionBuilderAtEnd(allocator, entry);
            } else {
                LLVMPositionBuilderBefore(allocator, first);
            }
            let pointer = LLVMBuildAlloca(allocator, ty, name.as_ptr());
            LLVMDisposeBuilder(allocator);
            pointer
        }
    }

    fn block(&mut self, body: &[ir::Statement], slots: &mut Slots) -> Result<Value, Error> {
        let mut last = self.void();
        for statement in body {
            last = self
                .statement(statement, slots)
                .map_err(|error| error.at(statement.span))?;
            if unsafe { !LLVMGetBasicBlockTerminator(LLVMGetInsertBlock(self.builder)).is_null() } {
                break;
            }
        }
        Ok(last)
    }

    fn statement(&mut self, statement: &ir::Statement, slots: &mut Slots) -> Result<Value, Error> {
        match &statement.kind {
            S::Let { binding, value } => {
                let range = super::range::infer(value, &self.ranges);
                let length = self.array_length(value);
                let value = self.expr(value, slots)?;
                self.bind(binding, value, slots)?;
                if !self.assigned.contains(&binding.id) {
                    if let Some(range) = range {
                        self.ranges.insert(binding.id, range);
                    }
                    if let Some(length) = length {
                        self.array_lengths.insert(binding.id, length);
                    }
                }
            }
            S::Assign { target, value } => {
                let value = self.expr(value, slots)?;
                unsafe { LLVMBuildStore(self.builder, value.value, slots[target].value) };
            }
            S::AssignIndex { base, index, value } => {
                let proven = self.proven_index(base, index);
                let base = self.expr(base, slots)?;
                let index = self.expr(index, slots)?;
                let value = self.expr(value, slots)?;
                let pointer = self.array_element_checked(base, index, proven, value.ty)?;
                unsafe { LLVMBuildStore(self.builder, value.value, pointer) };
            }
            S::AssignField {
                target,
                index,
                value,
            } => {
                let value = self.expr(value, slots)?;
                let slot = slots[target];
                unsafe {
                    let current =
                        LLVMBuildLoad2(self.builder, slot.ty, slot.value, c"struct.load".as_ptr());
                    let updated = LLVMBuildInsertValue(
                        self.builder,
                        current,
                        value.value,
                        *index,
                        c"field.set".as_ptr(),
                    );
                    LLVMBuildStore(self.builder, updated, slot.value);
                }
            }
            S::While { condition, body } => self.while_loop(condition, body, slots)?,
            S::For {
                binding,
                start,
                end,
                inclusive,
                body,
            } => self.for_loop(binding, start, end, *inclusive, body, slots)?,
            S::Return(value) => {
                let value = self.expr(value, slots)?;
                unsafe { LLVMBuildRet(self.builder, value.value) };
            }
            S::Expr(value) => return self.expr(value, slots),
        }
        Ok(self.void())
    }

    fn expr(&mut self, expression: &ir::Expr, slots: &mut Slots) -> Result<Value, Error> {
        self.expr_inner(expression, slots)
            .map_err(|error| error.at(expression.span))
    }

    fn expr_inner(&mut self, expression: &ir::Expr, slots: &mut Slots) -> Result<Value, Error> {
        let ty = self.ty(&expression.ty);
        let value = match &expression.kind {
            E::Int(value) => unsafe { LLVMConstInt(ty, *value as u64, 1) },
            E::Float(value) => unsafe { LLVMConstReal(ty, *value) },
            E::Bool(value) => unsafe { LLVMConstInt(ty, *value as u64, 0) },
            E::String(value) => {
                let text = cstring(value)?;
                let bytes = unsafe {
                    LLVMBuildGlobalString(self.builder, text.as_ptr(), c"string".as_ptr())
                };
                return self.external(
                    "nerv_string_new",
                    ty,
                    &[
                        Value {
                            value: bytes,
                            ty: self.pointer_ty(),
                        },
                        self.integer(value.len() as i64),
                    ],
                );
            }
            E::Local(id) => unsafe {
                LLVMBuildLoad2(self.builder, ty, slots[id].value, c"load".as_ptr())
            },
            E::Function(id) => self.functions[id].value,
            E::Array(elements) => {
                let Type::Array(element) = &expression.ty else {
                    unreachable!("checked array")
                };
                let array =
                    self.new_array(self.integer(elements.len() as i64), self.ty(element))?;
                for (index, element) in elements.iter().enumerate() {
                    let element = self.expr(element, slots)?;
                    self.array_store(array, self.integer(index as i64), element)?;
                }
                return Ok(array);
            }
            E::ArrayRepeat { value, length } => {
                let value = self.expr(value, slots)?;
                let length = self.expr(length, slots)?;
                if value.ty == self.int_ty() {
                    return self.external("nerv_array_i64_new", ty, &[length, value]);
                }
                let array = self.new_array(length, value.ty)?;
                let index = self.slot(self.int_ty(), c"array.index");
                unsafe {
                    LLVMBuildStore(self.builder, self.integer(0).value, index);
                }
                let test = self.new_block(c"array.repeat.test");
                let body = self.new_block(c"array.repeat.body");
                let done = self.new_block(c"array.repeat.done");
                unsafe {
                    LLVMBuildBr(self.builder, test);
                    LLVMPositionBuilderAtEnd(self.builder, test);
                    let current =
                        LLVMBuildLoad2(self.builder, self.int_ty(), index, c"array.index".as_ptr());
                    let more = LLVMBuildICmp(
                        self.builder,
                        IP::LLVMIntSLT,
                        current,
                        length.value,
                        c"array.more".as_ptr(),
                    );
                    LLVMBuildCondBr(self.builder, more, body, done);
                    LLVMPositionBuilderAtEnd(self.builder, body);
                    self.array_store(
                        array,
                        Value {
                            value: current,
                            ty: self.int_ty(),
                        },
                        value,
                    )?;
                    let next = LLVMBuildAdd(
                        self.builder,
                        current,
                        self.integer(1).value,
                        c"array.next".as_ptr(),
                    );
                    LLVMBuildStore(self.builder, next, index);
                    LLVMBuildBr(self.builder, test);
                    LLVMPositionBuilderAtEnd(self.builder, done);
                }
                return Ok(array);
            }
            E::Struct { fields, .. } => {
                let mut value = unsafe { LLVMGetUndef(ty) };
                for (index, field) in fields {
                    let field = self.expr(field, slots)?;
                    value = unsafe {
                        LLVMBuildInsertValue(
                            self.builder,
                            value,
                            field.value,
                            *index,
                            c"field".as_ptr(),
                        )
                    };
                }
                value
            }
            E::Enum {
                enumeration,
                tag,
                payload,
                ..
            } => {
                if !self.payloads[enumeration] {
                    unsafe { LLVMConstInt(ty, *tag as u64, 0) }
                } else {
                    let payload = if let Some(payload) = payload {
                        self.expr(payload, slots)?
                    } else {
                        self.integer(0)
                    };
                    unsafe {
                        let value = LLVMBuildInsertValue(
                            self.builder,
                            LLVMConstNull(ty),
                            self.integer(*tag).value,
                            0,
                            c"enum.tag".as_ptr(),
                        );
                        LLVMBuildInsertValue(
                            self.builder,
                            value,
                            payload.value,
                            (*tag + 1) as u32,
                            c"enum.payload".as_ptr(),
                        )
                    }
                }
            }
            E::Field { base, index } => {
                let base = self.expr(base, slots)?;
                unsafe {
                    LLVMBuildExtractValue(self.builder, base.value, *index, c"field".as_ptr())
                }
            }
            E::Index { base, index } => {
                let proven = self.proven_index(base, index);
                let base = self.expr(base, slots)?;
                let index = self.expr(index, slots)?;
                let pointer = self.array_element_checked(base, index, proven, ty)?;
                unsafe { LLVMBuildLoad2(self.builder, ty, pointer, c"array.load".as_ptr()) }
            }
            E::If {
                condition,
                then_body,
                else_body,
            } => return self.if_expr(condition, then_body, else_body, ty, slots),
            E::Match {
                value,
                enumeration,
                arms,
            } => return self.match_expr(value, *enumeration, arms, ty, slots),
            E::Unary { op, value } => {
                let floating = matches!(value.ty, Type::Float | Type::Float32);
                let value = self.expr(value, slots)?;
                unsafe {
                    match op {
                        U::Neg if floating => {
                            LLVMBuildFNeg(self.builder, value.value, c"neg".as_ptr())
                        }
                        U::Neg => LLVMBuildNeg(self.builder, value.value, c"neg".as_ptr()),
                        U::Not => LLVMBuildNot(self.builder, value.value, c"not".as_ptr()),
                    }
                }
            }
            E::Binary { left, op, right } => return self.binary(*op, left, right, ty, slots),
            E::Call { target, args } => {
                let function = match target {
                    ir::CallTarget::Direct(id) => self.functions[id],
                    ir::CallTarget::Indirect(callee) => {
                        let Type::Function { params, result } = &callee.ty else {
                            unreachable!("checked call signature")
                        };
                        let signature = self.signature(params, result);
                        let value = self.expr(callee, slots)?;
                        Function {
                            value: value.value,
                            ty: signature,
                        }
                    }
                };
                let mut args = args
                    .iter()
                    .map(|arg| self.expr(arg, slots).map(|value| value.value))
                    .collect::<Result<Vec<_>, _>>()?;
                unsafe {
                    LLVMBuildCall2(
                        self.builder,
                        function.ty,
                        function.value,
                        args.as_mut_ptr(),
                        args.len() as u32,
                        if expression.ty == Type::Void {
                            c"".as_ptr()
                        } else {
                            c"call".as_ptr()
                        },
                    )
                }
            }
            E::Runtime { symbol, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.expr(arg, slots))
                    .collect::<Result<Vec<_>, _>>()?;
                return self.external(symbol, ty, &args);
            }
            E::Assert(condition) => {
                let condition = self.expr(condition, slots)?;
                self.guard(condition.value, 2)?;
                return Ok(self.void());
            }
            E::Convert { conversion, value } => {
                let source_type = value.ty.clone();
                let value = self.expr(value, slots)?;
                match conversion {
                    ir::Conversion::Numeric => {
                        return self.numeric_conversion(&source_type, &expression.ty, value);
                    }
                    ir::Conversion::Identity => value.value,
                    ir::Conversion::IntToFloat => unsafe {
                        LLVMBuildSIToFP(self.builder, value.value, ty, c"to.float".as_ptr())
                    },
                    ir::Conversion::FloatToInt => {
                        return self.external("nerv_f64_to_i64", ty, &[value]);
                    }
                }
            }
        };
        Ok(Value { value, ty })
    }

    fn numeric_conversion(
        &mut self,
        source: &Type,
        destination: &Type,
        value: Value,
    ) -> Result<Value, Error> {
        let ty = self.ty(destination);
        let integer = |t: &Type| match t {
            Type::Int => Some((64, true)),
            Type::Integer { bits, signed } => Some((*bits, *signed)),
            _ => None,
        };
        let from = integer(source);
        let to = integer(destination);
        let name = c"convert".as_ptr();
        let converted = unsafe {
            match (from, to) {
                (Some((from_bits, signed)), Some((to_bits, _))) => {
                    if from_bits == to_bits {
                        value.value
                    } else if from_bits > to_bits {
                        LLVMBuildTrunc(self.builder, value.value, ty, name)
                    } else if signed {
                        LLVMBuildSExt(self.builder, value.value, ty, name)
                    } else {
                        LLVMBuildZExt(self.builder, value.value, ty, name)
                    }
                }
                (Some((_, true)), None) => LLVMBuildSIToFP(self.builder, value.value, ty, name),
                (Some((_, false)), None) => LLVMBuildUIToFP(self.builder, value.value, ty, name),
                (None, Some((bits, signed))) => {
                    let upper = 2f64.powi(if signed { bits as i32 - 1 } else { bits as i32 });
                    let lower = if signed { -upper } else { 0.0 };
                    let above = LLVMBuildFCmp(
                        self.builder,
                        FP::LLVMRealOGE,
                        value.value,
                        LLVMConstReal(value.ty, lower),
                        c"convert.lower".as_ptr(),
                    );
                    let below = LLVMBuildFCmp(
                        self.builder,
                        FP::LLVMRealOLT,
                        value.value,
                        LLVMConstReal(value.ty, upper),
                        c"convert.upper".as_ptr(),
                    );
                    let valid = LLVMBuildAnd(self.builder, above, below, c"convert.valid".as_ptr());
                    self.guard(valid, 5)?;
                    if signed {
                        LLVMBuildFPToSI(self.builder, value.value, ty, name)
                    } else {
                        LLVMBuildFPToUI(self.builder, value.value, ty, name)
                    }
                }
                (None, None) if source == destination => value.value,
                (None, None) if *destination == Type::Float32 => {
                    LLVMBuildFPTrunc(self.builder, value.value, ty, name)
                }
                (None, None) => LLVMBuildFPExt(self.builder, value.value, ty, name),
            }
        };
        Ok(Value {
            value: converted,
            ty,
        })
    }

    fn if_expr(
        &mut self,
        condition: &ir::Expr,
        then_body: &[ir::Statement],
        else_body: &[ir::Statement],
        ty: LLVMTypeRef,
        slots: &mut Slots,
    ) -> Result<Value, Error> {
        let condition = self.expr(condition, slots)?;
        let then_block = self.new_block(c"if.then");
        let else_block = self.new_block(c"if.else");
        let merge = self.new_block(c"if.merge");
        unsafe {
            LLVMBuildCondBr(self.builder, condition.value, then_block, else_block);
            LLVMPositionBuilderAtEnd(self.builder, then_block);
        }
        let then_value = self.block(then_body, slots)?;
        let then_end = unsafe { LLVMGetInsertBlock(self.builder) };
        unsafe {
            LLVMBuildBr(self.builder, merge);
            LLVMPositionBuilderAtEnd(self.builder, else_block);
        }
        let else_value = self.block(else_body, slots)?;
        let else_end = unsafe { LLVMGetInsertBlock(self.builder) };
        unsafe {
            LLVMBuildBr(self.builder, merge);
            LLVMPositionBuilderAtEnd(self.builder, merge);
        }
        Ok(self.phi(
            ty,
            &[(then_value.value, then_end), (else_value.value, else_end)],
        ))
    }

    fn match_expr(
        &mut self,
        expression: &ir::Expr,
        enumeration: SymbolId,
        arms: &[ir::MatchArm],
        ty: LLVMTypeRef,
        slots: &mut Slots,
    ) -> Result<Value, Error> {
        let value = self.expr(expression, slots)?;
        let has_payload = self.payloads[&enumeration];
        let tag = if has_payload {
            unsafe { LLVMBuildExtractValue(self.builder, value.value, 0, c"enum.tag".as_ptr()) }
        } else {
            value.value
        };
        let merge = self.new_block(c"match.merge");
        let fallback = self.new_block(c"match.invalid");
        let blocks = arms
            .iter()
            .map(|_| self.new_block(c"match.arm"))
            .collect::<Vec<_>>();
        let switch = unsafe { LLVMBuildSwitch(self.builder, tag, fallback, arms.len() as u32) };
        for (arm, block) in arms.iter().zip(&blocks) {
            unsafe { LLVMAddCase(switch, self.integer(arm.tag).value, *block) };
        }
        let mut incoming = Vec::new();
        for (arm, block) in arms.iter().zip(blocks) {
            unsafe { LLVMPositionBuilderAtEnd(self.builder, block) };
            if let Some(binding) = &arm.binding {
                self.bind(
                    binding,
                    Value {
                        value: unsafe {
                            LLVMBuildExtractValue(
                                self.builder,
                                value.value,
                                (arm.tag + 1) as u32,
                                c"enum.payload".as_ptr(),
                            )
                        },
                        ty: self.ty(&binding.ty),
                    },
                    slots,
                )?;
            }
            let result = self.expr(&arm.body, slots)?;
            incoming.push((result.value, unsafe { LLVMGetInsertBlock(self.builder) }));
            unsafe { LLVMBuildBr(self.builder, merge) };
        }
        unsafe {
            LLVMPositionBuilderAtEnd(self.builder, fallback);
            LLVMBuildUnreachable(self.builder);
            LLVMPositionBuilderAtEnd(self.builder, merge);
        }
        Ok(self.phi(ty, &incoming))
    }

    fn phi(&self, ty: LLVMTypeRef, incoming: &[(LLVMValueRef, LLVMBasicBlockRef)]) -> Value {
        if ty == self.void_ty() {
            return self.void();
        }
        let mut values = incoming.iter().map(|(value, _)| *value).collect::<Vec<_>>();
        let mut blocks = incoming.iter().map(|(_, block)| *block).collect::<Vec<_>>();
        let value = unsafe {
            let phi = LLVMBuildPhi(self.builder, ty, c"result".as_ptr());
            LLVMAddIncoming(
                phi,
                values.as_mut_ptr(),
                blocks.as_mut_ptr(),
                values.len() as u32,
            );
            phi
        };
        Value { value, ty }
    }

    fn while_loop(
        &mut self,
        condition: &ir::Expr,
        body: &[ir::Statement],
        slots: &mut Slots,
    ) -> Result<(), Error> {
        let test = self.new_block(c"while.condition");
        let body_block = self.new_block(c"while.body");
        let exit = self.new_block(c"while.exit");
        unsafe {
            LLVMBuildBr(self.builder, test);
            LLVMPositionBuilderAtEnd(self.builder, test);
        }
        let condition = self.expr(condition, slots)?;
        unsafe {
            LLVMBuildCondBr(self.builder, condition.value, body_block, exit);
            LLVMPositionBuilderAtEnd(self.builder, body_block);
        }
        self.loop_depth += 1;
        let previous_reciprocal_loop = std::mem::replace(&mut self.reciprocal_loop, false);
        self.block(body, slots)?;
        self.reciprocal_loop = previous_reciprocal_loop;
        self.loop_depth -= 1;
        unsafe {
            LLVMBuildBr(self.builder, test);
            LLVMPositionBuilderAtEnd(self.builder, exit);
        }
        Ok(())
    }

    fn for_loop(
        &mut self,
        binding: &ir::Binding,
        start: &ir::Expr,
        end: &ir::Expr,
        inclusive: bool,
        body: &[ir::Statement],
        slots: &mut Slots,
    ) -> Result<(), Error> {
        // Reciprocal setup pays off only with reuse. For now require a proven
        // minimum of 16 iterations; short/unknown-trip loops keep hardware div.
        let amortized = super::range::infer(start, &self.ranges)
            .zip(super::range::infer(end, &self.ranges))
            .is_some_and(|((_, start_max), (end_min, _))| {
                i128::from(end_min) - i128::from(start_max) + i128::from(inclusive) >= 16
            });
        let range = super::range::infer(start, &self.ranges)
            .zip(super::range::infer(end, &self.ranges))
            .and_then(|((lo, _), (_, hi))| {
                let hi = if inclusive { hi } else { hi.checked_sub(1)? };
                (lo <= hi).then_some((lo, hi))
            });
        let start = self.expr(start, slots)?;
        let end = self.expr(end, slots)?;
        self.bind(binding, start, slots)?;
        let slot = slots[&binding.id].value;
        let test = self.new_block(c"for.condition");
        let body_block = self.new_block(c"for.body");
        let exit = self.new_block(c"for.exit");
        unsafe {
            LLVMBuildBr(self.builder, test);
            LLVMPositionBuilderAtEnd(self.builder, test);
        }
        let index =
            unsafe { LLVMBuildLoad2(self.builder, self.int_ty(), slot, c"for.index".as_ptr()) };
        let predicate = if inclusive {
            IP::LLVMIntSLE
        } else {
            IP::LLVMIntSLT
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
        unsafe {
            LLVMBuildCondBr(self.builder, condition, body_block, exit);
            LLVMPositionBuilderAtEnd(self.builder, body_block);
        }
        self.loop_depth += 1;
        let previous_reciprocal_loop = std::mem::replace(&mut self.reciprocal_loop, amortized);
        if !binding.mutable {
            self.immutable_depths.insert(binding.id, self.loop_depth);
            if let Some(range) = range {
                self.ranges.insert(binding.id, range);
            }
        }
        self.block(body, slots)?;
        self.ranges.remove(&binding.id);
        self.loop_depth -= 1;
        self.reciprocal_loop = previous_reciprocal_loop;
        let index =
            unsafe { LLVMBuildLoad2(self.builder, self.int_ty(), slot, c"for.next".as_ptr()) };
        if inclusive {
            let increment = self.new_block(c"for.increment");
            unsafe {
                let finished = LLVMBuildICmp(
                    self.builder,
                    IP::LLVMIntEQ,
                    index,
                    end.value,
                    c"for.finished".as_ptr(),
                );
                LLVMBuildCondBr(self.builder, finished, exit, increment);
                LLVMPositionBuilderAtEnd(self.builder, increment);
            }
        }
        unsafe {
            let next = LLVMBuildAdd(
                self.builder,
                index,
                self.integer(1).value,
                c"for.increment".as_ptr(),
            );
            LLVMBuildStore(self.builder, next, slot);
            LLVMBuildBr(self.builder, test);
            LLVMPositionBuilderAtEnd(self.builder, exit);
        }
        Ok(())
    }

    fn binary(
        &mut self,
        op: B,
        left: &ir::Expr,
        right: &ir::Expr,
        ty: LLVMTypeRef,
        slots: &mut Slots,
    ) -> Result<Value, Error> {
        if matches!(op, B::And | B::Or) {
            let condition = self.expr(left, slots)?;
            let rhs = self.new_block(c"logical.rhs");
            let merge = self.new_block(c"logical.merge");
            let origin = unsafe { LLVMGetInsertBlock(self.builder) };
            unsafe {
                if op == B::And {
                    LLVMBuildCondBr(self.builder, condition.value, rhs, merge);
                } else {
                    LLVMBuildCondBr(self.builder, condition.value, merge, rhs);
                }
                LLVMPositionBuilderAtEnd(self.builder, rhs);
            }
            let right = self.expr(right, slots)?;
            let right_end = unsafe { LLVMGetInsertBlock(self.builder) };
            unsafe {
                LLVMBuildBr(self.builder, merge);
                LLVMPositionBuilderAtEnd(self.builder, merge);
            }
            return Ok(self.phi(ty, &[(condition.value, origin), (right.value, right_end)]));
        }
        let floating = matches!(left.ty, Type::Float | Type::Float32);
        let integer = match left.ty {
            Type::Integer { bits, signed } => Some((bits, signed)),
            _ => None,
        };
        let numerator_upper = super::range::infer(left, &self.ranges)
            .filter(|(lo, hi)| *lo >= 0 && *hi <= u32::MAX as i64)
            .map(|(_, hi)| hi as u32);
        let right_small = super::range::fits_u32(super::range::infer(right, &self.ranges));
        let reciprocal = self.mode == BuildMode::Release
            && self.reciprocal_loop
            && matches!(&right.kind,
            E::Local(id) if self.immutable_depths.get(id).is_some_and(|depth| *depth < self.loop_depth));
        let left = self.expr(left, slots)?;
        let mut right = self.expr(right, slots)?;
        if let Some((bits, signed)) = integer {
            if matches!(op, B::Div | B::Rem) {
                unsafe {
                    let zero = LLVMConstInt(left.ty, 0, 0);
                    let nonzero = LLVMBuildICmp(
                        self.builder,
                        IP::LLVMIntNE,
                        right.value,
                        zero,
                        c"div.nonzero".as_ptr(),
                    );
                    self.guard(nonzero, 1)?;
                    if signed {
                        let minimum = LLVMBuildICmp(
                            self.builder,
                            IP::LLVMIntEQ,
                            left.value,
                            LLVMConstInt(left.ty, 1u64 << (bits - 1), 0),
                            c"div.minimum".as_ptr(),
                        );
                        let negative_one = LLVMBuildICmp(
                            self.builder,
                            IP::LLVMIntEQ,
                            right.value,
                            LLVMConstAllOnes(right.ty),
                            c"div.negative_one".as_ptr(),
                        );
                        let overflow = LLVMBuildAnd(
                            self.builder,
                            minimum,
                            negative_one,
                            c"div.overflow".as_ptr(),
                        );
                        right.value = LLVMBuildSelect(
                            self.builder,
                            overflow,
                            LLVMConstInt(right.ty, 1, 0),
                            right.value,
                            c"div.safe".as_ptr(),
                        );
                    }
                    let value = match (op, signed) {
                        (B::Div, true) => {
                            LLVMBuildSDiv(self.builder, left.value, right.value, c"div".as_ptr())
                        }
                        (B::Div, false) => {
                            LLVMBuildUDiv(self.builder, left.value, right.value, c"div".as_ptr())
                        }
                        (B::Rem, true) => {
                            LLVMBuildSRem(self.builder, left.value, right.value, c"rem".as_ptr())
                        }
                        _ => LLVMBuildURem(self.builder, left.value, right.value, c"rem".as_ptr()),
                    };
                    return Ok(Value { value, ty });
                }
            }
        }
        if matches!(op, B::Div | B::Rem) && !floating && integer.is_none() {
            unsafe {
                let nonzero = LLVMBuildICmp(
                    self.builder,
                    IP::LLVMIntNE,
                    right.value,
                    self.integer(0).value,
                    c"div.nonzero".as_ptr(),
                );
                self.guard(nonzero, 1)?;
                let minimum = LLVMBuildICmp(
                    self.builder,
                    IP::LLVMIntEQ,
                    left.value,
                    self.integer(i64::MIN).value,
                    c"div.minimum".as_ptr(),
                );
                let negative_one = LLVMBuildICmp(
                    self.builder,
                    IP::LLVMIntEQ,
                    right.value,
                    self.integer(-1).value,
                    c"div.negative_one".as_ptr(),
                );
                let overflow = LLVMBuildAnd(
                    self.builder,
                    minimum,
                    negative_one,
                    c"div.overflow".as_ptr(),
                );
                right.value = LLVMBuildSelect(
                    self.builder,
                    overflow,
                    self.integer(1).value,
                    right.value,
                    c"div.safe".as_ptr(),
                );
            }
            return Ok(self.integer_division(
                op,
                left,
                right,
                right_small,
                reciprocal,
                numerator_upper,
            ));
        }
        let name = c"binary".as_ptr();
        let value = unsafe {
            match (op, floating) {
                (B::Add, false) => LLVMBuildAdd(self.builder, left.value, right.value, name),
                (B::Sub, false) => LLVMBuildSub(self.builder, left.value, right.value, name),
                (B::Mul, false) => LLVMBuildMul(self.builder, left.value, right.value, name),
                (B::Div, false) => LLVMBuildSDiv(self.builder, left.value, right.value, name),
                (B::Rem, false) => LLVMBuildSRem(self.builder, left.value, right.value, name),
                (B::Add, true) => LLVMBuildFAdd(self.builder, left.value, right.value, name),
                (B::Sub, true) => LLVMBuildFSub(self.builder, left.value, right.value, name),
                (B::Mul, true) => LLVMBuildFMul(self.builder, left.value, right.value, name),
                (B::Div, true) => LLVMBuildFDiv(self.builder, left.value, right.value, name),
                (B::Rem, true) => LLVMBuildFRem(self.builder, left.value, right.value, name),
                (B::Eq, false) => {
                    LLVMBuildICmp(self.builder, IP::LLVMIntEQ, left.value, right.value, name)
                }
                (B::NotEq, false) => {
                    LLVMBuildICmp(self.builder, IP::LLVMIntNE, left.value, right.value, name)
                }
                (B::Lt, false) => LLVMBuildICmp(
                    self.builder,
                    if integer.is_some_and(|(_, signed)| !signed) {
                        IP::LLVMIntULT
                    } else {
                        IP::LLVMIntSLT
                    },
                    left.value,
                    right.value,
                    name,
                ),
                (B::LtEq, false) => LLVMBuildICmp(
                    self.builder,
                    if integer.is_some_and(|(_, signed)| !signed) {
                        IP::LLVMIntULE
                    } else {
                        IP::LLVMIntSLE
                    },
                    left.value,
                    right.value,
                    name,
                ),
                (B::Gt, false) => LLVMBuildICmp(
                    self.builder,
                    if integer.is_some_and(|(_, signed)| !signed) {
                        IP::LLVMIntUGT
                    } else {
                        IP::LLVMIntSGT
                    },
                    left.value,
                    right.value,
                    name,
                ),
                (B::GtEq, false) => LLVMBuildICmp(
                    self.builder,
                    if integer.is_some_and(|(_, signed)| !signed) {
                        IP::LLVMIntUGE
                    } else {
                        IP::LLVMIntSGE
                    },
                    left.value,
                    right.value,
                    name,
                ),
                (B::Eq, true) => {
                    LLVMBuildFCmp(self.builder, FP::LLVMRealOEQ, left.value, right.value, name)
                }
                (B::NotEq, true) => {
                    LLVMBuildFCmp(self.builder, FP::LLVMRealUNE, left.value, right.value, name)
                }
                (B::Lt, true) => {
                    LLVMBuildFCmp(self.builder, FP::LLVMRealOLT, left.value, right.value, name)
                }
                (B::LtEq, true) => {
                    LLVMBuildFCmp(self.builder, FP::LLVMRealOLE, left.value, right.value, name)
                }
                (B::Gt, true) => {
                    LLVMBuildFCmp(self.builder, FP::LLVMRealOGT, left.value, right.value, name)
                }
                (B::GtEq, true) => {
                    LLVMBuildFCmp(self.builder, FP::LLVMRealOGE, left.value, right.value, name)
                }
                (B::And | B::Or, _) => unreachable!("short circuit emitted above"),
            }
        };
        Ok(Value { value, ty })
    }

    // A guarded narrowing optimization, not a language-level numeric conversion.
    // Nonnegative operands fitting u32 have exactly the same quotient/remainder
    // in i64. Wide and negative values retain the original signed i64 path.
    // The caller has already checked zero and made MIN / -1 well-defined.
    fn integer_division(
        &self,
        op: B,
        left: Value,
        right: Value,
        right_small: bool,
        reciprocal: bool,
        numerator_upper: Option<u32>,
    ) -> Value {
        unsafe {
            let limit = self.integer(u32::MAX as i64).value;
            let left_fits = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntULE,
                left.value,
                limit,
                c"div.left_fits_u32".as_ptr(),
            );
            let right_fits = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntULE,
                right.value,
                limit,
                c"div.right_fits_u32".as_ptr(),
            );
            let true_value = LLVMConstInt(LLVMInt1TypeInContext(self.context), 1, 0);
            let small = LLVMBuildAnd(
                self.builder,
                if numerator_upper.is_some() {
                    true_value
                } else {
                    left_fits
                },
                if right_small { true_value } else { right_fits },
                c"div.small".as_ptr(),
            );
            let narrow_block = self.new_block(c"div.u32");
            let wide_block = self.new_block(c"div.i64");
            let merge = self.new_block(c"div.result");
            LLVMBuildCondBr(self.builder, small, narrow_block, wide_block);
            LLVMPositionBuilderAtEnd(self.builder, narrow_block);
            let int32 = LLVMInt32TypeInContext(self.context);
            let lhs = LLVMBuildTrunc(self.builder, left.value, int32, c"div.lhs32".as_ptr());
            let rhs = LLVMBuildTrunc(self.builder, right.value, int32, c"div.rhs32".as_ptr());
            let narrow = if reciprocal {
                self.reciprocal_division(op, left, right, numerator_upper)
            } else {
                let narrow = if op == B::Div {
                    LLVMBuildUDiv(self.builder, lhs, rhs, c"div32".as_ptr())
                } else {
                    LLVMBuildURem(self.builder, lhs, rhs, c"rem32".as_ptr())
                };
                LLVMBuildZExt(self.builder, narrow, self.int_ty(), c"div.extend".as_ptr())
            };
            let narrow_end = LLVMGetInsertBlock(self.builder);
            LLVMBuildBr(self.builder, merge);
            LLVMPositionBuilderAtEnd(self.builder, wide_block);
            let wide = if op == B::Div {
                LLVMBuildSDiv(self.builder, left.value, right.value, c"div64".as_ptr())
            } else {
                LLVMBuildSRem(self.builder, left.value, right.value, c"rem64".as_ptr())
            };
            LLVMBuildBr(self.builder, merge);
            LLVMPositionBuilderAtEnd(self.builder, merge);
            self.phi(self.int_ty(), &[(narrow, narrow_end), (wide, wide_block)])
        }
    }

    // For 0 <= n < 2^32 and 1 <= d < 2^32, m=floor(2^32/d).
    // q=floor(n*m/2^32) underestimates floor(n/d) by at most one:
    // n/2^32 < 1 bounds the reciprocal's rounding error. One correction is
    // therefore exact. Products fit u64, including d=1 (m=2^32).
    // Only immutable divisors defined outside the current loop use this path;
    // LLVM LICM can hoist the setup division without changing evaluation order.
    fn reciprocal_division(
        &self,
        op: B,
        left: Value,
        right: Value,
        numerator_upper: Option<u32>,
    ) -> LLVMValueRef {
        unsafe {
            if let Some(upper) = numerator_upper {
                return self.bounded_reciprocal_division(op, left, right, upper);
            }
            let multiplier = LLVMBuildUDiv(
                self.builder,
                self.integer(1_i64 << 32).value,
                right.value,
                c"div.reciprocal".as_ptr(),
            );
            self.corrected_reciprocal_division(op, left, right, multiplier)
        }
    }

    // Rounded-up reciprocal m=ceil(2^32/d), error e=m*d-2^32.
    // If B*e < 2^32, then for every 0<=n<=B the quotient floor(n*m/2^32)
    // is exact: its error n*e/(d*2^32) is strictly less than 1/d, the
    // smallest gap between a non-integer n/d and the next integer.
    // The runtime predicate depends only on the invariant divisor and the
    // proven source interval, so LLVM can unswitch it outside the loop.
    fn bounded_reciprocal_division(
        &self,
        op: B,
        left: Value,
        right: Value,
        upper: u32,
    ) -> LLVMValueRef {
        unsafe {
            let scale = self.integer(1_i64 << 32).value;
            let quotient = LLVMBuildUDiv(
                self.builder,
                self.integer(u32::MAX as i64).value,
                right.value,
                c"div.reciprocal".as_ptr(),
            );
            let rounded = LLVMBuildAdd(
                self.builder,
                quotient,
                self.integer(1).value,
                c"div.rounded".as_ptr(),
            );
            let product = LLVMBuildMul(self.builder, rounded, right.value, c"div.scaled".as_ptr());
            let error = LLVMBuildSub(self.builder, product, scale, c"div.error".as_ptr());
            let error_bound = LLVMBuildMul(
                self.builder,
                error,
                self.integer(upper as i64).value,
                c"div.error_bound".as_ptr(),
            );
            let exact = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntULT,
                error_bound,
                scale,
                c"div.exact".as_ptr(),
            );
            let fast = self.new_block(c"div.bounded");
            let corrected = self.new_block(c"div.corrected");
            let merge = self.new_block(c"div.bounded.result");
            LLVMBuildCondBr(self.builder, exact, fast, corrected);
            LLVMPositionBuilderAtEnd(self.builder, fast);
            let int32 = LLVMInt32TypeInContext(self.context);
            let numerator32 = LLVMBuildTrunc(
                self.builder,
                left.value,
                int32,
                c"div.exact_numerator32".as_ptr(),
            );
            let magic32 =
                LLVMBuildTrunc(self.builder, rounded, int32, c"div.exact_magic32".as_ptr());
            let magic = LLVMBuildZExt(
                self.builder,
                magic32,
                self.int_ty(),
                c"div.exact_magic".as_ptr(),
            );
            let numerator = LLVMBuildZExt(
                self.builder,
                numerator32,
                self.int_ty(),
                c"div.exact_numerator".as_ptr(),
            );
            let product = LLVMBuildMul(
                self.builder,
                numerator,
                magic,
                c"div.exact_product".as_ptr(),
            );
            let quotient64 = LLVMBuildLShr(
                self.builder,
                product,
                self.integer(32).value,
                c"div.exact_quotient".as_ptr(),
            );
            let quotient = LLVMBuildTrunc(
                self.builder,
                quotient64,
                int32,
                c"div.exact_quotient32".as_ptr(),
            );
            let result = if op == B::Div {
                quotient
            } else {
                let divisor32 = LLVMBuildTrunc(
                    self.builder,
                    right.value,
                    int32,
                    c"div.exact_divisor32".as_ptr(),
                );
                let consumed = LLVMBuildMul(
                    self.builder,
                    quotient,
                    divisor32,
                    c"div.exact_consumed".as_ptr(),
                );
                LLVMBuildSub(
                    self.builder,
                    numerator32,
                    consumed,
                    c"div.exact_remainder".as_ptr(),
                )
            };
            let is_one = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntEQ,
                right.value,
                self.integer(1).value,
                c"div.exact_is_one".as_ptr(),
            );
            let result = LLVMBuildSelect(
                self.builder,
                is_one,
                if op == B::Div {
                    numerator32
                } else {
                    LLVMConstInt(int32, 0, 0)
                },
                result,
                c"div.exact_unit".as_ptr(),
            );
            let result = LLVMBuildZExt(
                self.builder,
                result,
                self.int_ty(),
                c"div.exact_extend".as_ptr(),
            );
            LLVMBuildBr(self.builder, merge);
            LLVMPositionBuilderAtEnd(self.builder, corrected);
            let no_error = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntEQ,
                error,
                self.integer(0).value,
                c"div.no_error".as_ptr(),
            );
            let down = LLVMBuildSub(
                self.builder,
                rounded,
                self.integer(1).value,
                c"div.round_down".as_ptr(),
            );
            let floor =
                LLVMBuildSelect(self.builder, no_error, rounded, down, c"div.floor".as_ptr());
            let fallback = self.corrected_reciprocal_division(op, left, right, floor);
            LLVMBuildBr(self.builder, merge);
            LLVMPositionBuilderAtEnd(self.builder, merge);
            self.phi(self.int_ty(), &[(result, fast), (fallback, corrected)])
                .value
        }
    }

    fn corrected_reciprocal_division(
        &self,
        op: B,
        left: Value,
        right: Value,
        multiplier: LLVMValueRef,
    ) -> LLVMValueRef {
        unsafe {
            // d=1 is handled separately below, allowing a widening 32x32
            // multiply instead of an expensive vector i64 multiply.
            let int32 = LLVMInt32TypeInContext(self.context);
            let multiplier32 = LLVMBuildTrunc(
                self.builder,
                multiplier,
                int32,
                c"div.multiplier32".as_ptr(),
            );
            let multiplier = LLVMBuildZExt(
                self.builder,
                multiplier32,
                self.int_ty(),
                c"div.multiplier".as_ptr(),
            );
            let numerator =
                LLVMBuildTrunc(self.builder, left.value, int32, c"div.numerator32".as_ptr());
            let numerator64 = LLVMBuildZExt(
                self.builder,
                numerator,
                self.int_ty(),
                c"div.numerator".as_ptr(),
            );
            let product = LLVMBuildMul(
                self.builder,
                numerator64,
                multiplier,
                c"div.product".as_ptr(),
            );
            let quotient = LLVMBuildLShr(
                self.builder,
                product,
                self.integer(32).value,
                c"div.estimate".as_ptr(),
            );
            // The estimate and q*d are <= n <= u32::MAX. Keep correction
            // arithmetic narrow so the target can use widening SIMD multiplies.
            let quotient =
                LLVMBuildTrunc(self.builder, quotient, int32, c"div.estimate32".as_ptr());
            let divisor =
                LLVMBuildTrunc(self.builder, right.value, int32, c"div.divisor32".as_ptr());
            let consumed = LLVMBuildMul(self.builder, quotient, divisor, c"div.consumed".as_ptr());
            let remainder =
                LLVMBuildSub(self.builder, numerator, consumed, c"div.remainder".as_ptr());
            let correct = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntUGE,
                remainder,
                divisor,
                c"div.correct".as_ptr(),
            );
            let result = if op == B::Div {
                let adjustment =
                    LLVMBuildZExt(self.builder, correct, int32, c"div.adjustment".as_ptr());
                LLVMBuildAdd(self.builder, quotient, adjustment, c"div.quotient".as_ptr())
            } else {
                let adjustment = LLVMBuildSelect(
                    self.builder,
                    correct,
                    divisor,
                    LLVMConstInt(int32, 0, 0),
                    c"div.adjustment".as_ptr(),
                );
                LLVMBuildSub(self.builder, remainder, adjustment, c"div.reduced".as_ptr())
            };
            let is_one = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntEQ,
                right.value,
                self.integer(1).value,
                c"div.is_one".as_ptr(),
            );
            let result = LLVMBuildSelect(
                self.builder,
                is_one,
                if op == B::Div {
                    numerator
                } else {
                    LLVMConstInt(int32, 0, 0)
                },
                result,
                c"div.unit".as_ptr(),
            );
            LLVMBuildZExt(self.builder, result, self.int_ty(), c"div.extend".as_ptr())
        }
    }

    fn new_array(&mut self, length: Value, element: LLVMTypeRef) -> Result<Value, Error> {
        if element == self.int_ty() {
            return self.external(
                "nerv_array_i64_new",
                self.pointer_ty(),
                &[length, self.integer(0)],
            );
        }
        let size = unsafe { LLVMSizeOf(element) };
        self.external(
            "nerv_array_new",
            self.pointer_ty(),
            &[
                length,
                Value {
                    value: size,
                    ty: self.int_ty(),
                },
            ],
        )
    }

    fn array_length(&self, expression: &ir::Expr) -> Option<i64> {
        match &expression.kind {
            E::Local(id) => self.array_lengths.get(id).copied(),
            E::Array(elements) => i64::try_from(elements.len()).ok(),
            E::ArrayRepeat { length, .. } => super::range::infer(length, &self.ranges)
                .and_then(|(lo, hi)| (lo == hi && lo >= 0).then_some(lo)),
            E::Runtime { symbol, args } if symbol == "nerv_array_i64_new" => {
                super::range::infer(&args[0], &self.ranges)
                    .and_then(|(lo, hi)| (lo == hi && lo >= 0).then_some(lo))
            }
            _ => None,
        }
    }

    fn proven_index(&self, base: &ir::Expr, index: &ir::Expr) -> bool {
        self.array_length(base)
            .zip(super::range::infer(index, &self.ranges))
            .is_some_and(|(length, (lo, hi))| lo >= 0 && hi < length)
    }

    fn array_element(
        &mut self,
        array: Value,
        index: Value,
        element: LLVMTypeRef,
    ) -> Result<LLVMValueRef, Error> {
        self.array_element_checked(array, index, false, element)
    }

    fn array_element_checked(
        &mut self,
        array: Value,
        index: Value,
        proven: bool,
        element: LLVMTypeRef,
    ) -> Result<LLVMValueRef, Error> {
        unsafe {
            let valid = LLVMBuildIsNotNull(self.builder, array.value, c"array.valid".as_ptr());
            self.guard(valid, 4)?;
            // ABI v2 exposes only this immutable C-layout prefix, never Vec's
            // private Rust layout. Element memory remains mutable and aliasable.
            let header = LLVMStructTypeInContext(
                self.context,
                [self.pointer_ty(), self.int_ty()].as_mut_ptr(),
                2,
                0,
            );
            let length_ptr = LLVMBuildStructGEP2(
                self.builder,
                header,
                array.value,
                1,
                c"array.length.ptr".as_ptr(),
            );
            let length = self.invariant_load(self.int_ty(), length_ptr, c"array.length");
            // Unsigned comparison also rejects all negative i64 indices.
            let in_bounds = LLVMBuildICmp(
                self.builder,
                IP::LLVMIntULT,
                index.value,
                length,
                c"array.in_bounds".as_ptr(),
            );
            if !proven {
                self.guard(in_bounds, 3)?;
            }
            let data_ptr = LLVMBuildStructGEP2(
                self.builder,
                header,
                array.value,
                0,
                c"array.data.ptr".as_ptr(),
            );
            let data = self.invariant_load(self.pointer_ty(), data_ptr, c"array.data");
            Ok(LLVMBuildInBoundsGEP2(
                self.builder,
                element,
                data,
                [index.value].as_mut_ptr(),
                1,
                c"array.element".as_ptr(),
            ))
        }
    }

    fn invariant_load(&self, ty: LLVMTypeRef, pointer: LLVMValueRef, name: &CStr) -> LLVMValueRef {
        unsafe {
            let value = LLVMBuildLoad2(self.builder, ty, pointer, name.as_ptr());
            let kind = LLVMGetMDKindIDInContext(self.context, c"invariant.load".as_ptr(), 14);
            let metadata = LLVMMDNodeInContext2(self.context, ptr::null_mut(), 0);
            LLVMSetMetadata(value, kind, LLVMMetadataAsValue(self.context, metadata));
            value
        }
    }
    fn array_store(&mut self, array: Value, index: Value, value: Value) -> Result<(), Error> {
        let pointer = self.array_element(array, index, value.ty)?;
        unsafe { LLVMBuildStore(self.builder, value.value, pointer) };
        Ok(())
    }
    fn guard(&mut self, condition: LLVMValueRef, code: i64) -> Result<(), Error> {
        let success = self.new_block(c"check.ok");
        let failure = self.new_block(c"check.failed");
        unsafe {
            LLVMBuildCondBr(self.builder, condition, success, failure);
            LLVMPositionBuilderAtEnd(self.builder, failure);
        }
        self.external("nerv_fail", self.void_ty(), &[self.integer(code)])?;
        unsafe {
            LLVMBuildUnreachable(self.builder);
            LLVMPositionBuilderAtEnd(self.builder, success);
        }
        Ok(())
    }
    fn external(
        &mut self,
        name: &str,
        result: LLVMTypeRef,
        args: &[Value],
    ) -> Result<Value, Error> {
        let function = if let Some(function) = self.externals.get(name) {
            *function
        } else {
            let mut params = args.iter().map(|value| value.ty).collect::<Vec<_>>();
            let ty =
                unsafe { LLVMFunctionType(result, params.as_mut_ptr(), params.len() as u32, 0) };
            let name_c = cstring(name)?;
            let value = unsafe { LLVMAddFunction(self.module, name_c.as_ptr(), ty) };
            let function = Function { value, ty };
            self.externals.insert(name.into(), function);
            function
        };
        let mut args = args.iter().map(|value| value.value).collect::<Vec<_>>();
        let value = unsafe {
            LLVMBuildCall2(
                self.builder,
                function.ty,
                function.value,
                args.as_mut_ptr(),
                args.len() as u32,
                if result == self.void_ty() {
                    c"".as_ptr()
                } else {
                    c"runtime.call".as_ptr()
                },
            )
        };
        Ok(Value { value, ty: result })
    }
    fn new_block(&self, name: &CStr) -> LLVMBasicBlockRef {
        unsafe {
            LLVMAppendBasicBlockInContext(
                self.context,
                LLVMGetBasicBlockParent(LLVMGetInsertBlock(self.builder)),
                name.as_ptr(),
            )
        }
    }
    fn integer(&self, value: i64) -> Value {
        Value {
            value: unsafe { LLVMConstInt(self.int_ty(), value as u64, 1) },
            ty: self.int_ty(),
        }
    }
    fn void(&self) -> Value {
        Value {
            value: ptr::null_mut(),
            ty: self.void_ty(),
        }
    }
    fn void_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMVoidTypeInContext(self.context) }
    }
    fn bool_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMInt1TypeInContext(self.context) }
    }
    fn int_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMInt64TypeInContext(self.context) }
    }
    fn float_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMDoubleTypeInContext(self.context) }
    }
    fn pointer_ty(&self) -> LLVMTypeRef {
        unsafe { LLVMPointerTypeInContext(self.context, 0) }
    }
}
