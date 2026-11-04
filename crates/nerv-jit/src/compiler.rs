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
use std::collections::HashMap;

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
}

pub(super) fn generate(
    program: &CheckedProgram,
    mode: BuildMode,
    standalone: bool,
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
        };
        let result = compiler
            .compile(program, standalone)
            .and_then(|()| super::optimize::optimize(module, mode, standalone));
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
                unsafe {
                    LLVMStructTypeInContext(
                        self.context,
                        [self.int_ty(), self.int_ty()].as_mut_ptr(),
                        2,
                        0,
                    )
                }
            } else {
                self.int_ty()
            };
            self.types.insert(enumeration.id, ty);
            self.payloads.insert(enumeration.id, payload);
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
        let runtime = if program.function(main).result == Type::Int {
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
        // A lexical binding needs one stack slot per invocation, not one alloca
        // per loop iteration (especially important for unoptimized builds).
        let pointer = unsafe {
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
        };
        unsafe { LLVMBuildStore(self.builder, value.value, pointer) };
        slots.insert(binding.id, Value { value: pointer, ty });
        Ok(())
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
                let value = self.expr(value, slots)?;
                self.bind(binding, value, slots)?;
            }
            S::Assign { target, value } => {
                let value = self.expr(value, slots)?;
                unsafe { LLVMBuildStore(self.builder, value.value, slots[target].value) };
            }
            S::AssignIndex { base, index, value } => {
                let base = self.expr(base, slots)?;
                let index = self.expr(index, slots)?;
                let value = self.expr(value, slots)?;
                self.array_store(base, index, value)?;
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
                let array = self.external(
                    "nerv_array_i64_new",
                    ty,
                    &[self.integer(elements.len() as i64), self.integer(0)],
                )?;
                for (index, element) in elements.iter().enumerate() {
                    let element = self.expr(element, slots)?;
                    self.array_store(array, self.integer(index as i64), element)?;
                }
                return Ok(array);
            }
            E::ArrayRepeat { value, length } => {
                let value = self.expr(value, slots)?;
                let length = self.expr(length, slots)?;
                return self.external("nerv_array_i64_new", ty, &[length, value]);
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
                            LLVMGetUndef(ty),
                            self.integer(*tag).value,
                            0,
                            c"enum.tag".as_ptr(),
                        );
                        LLVMBuildInsertValue(
                            self.builder,
                            value,
                            payload.value,
                            1,
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
                let base = self.expr(base, slots)?;
                let index = self.expr(index, slots)?;
                let pointer = self.array_element(base, index)?;
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
                let floating = value.ty == Type::Float;
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
                let value = self.expr(value, slots)?;
                match conversion {
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
        let payload = if has_payload {
            Some(unsafe {
                LLVMBuildExtractValue(self.builder, value.value, 1, c"enum.payload".as_ptr())
            })
        } else {
            None
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
                        value: payload.expect("checked enum payload"),
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
        self.block(body, slots)?;
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
        self.block(body, slots)?;
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
        let floating = left.ty == Type::Float;
        let left = self.expr(left, slots)?;
        let mut right = self.expr(right, slots)?;
        if matches!(op, B::Div | B::Rem) && !floating {
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
                (B::Lt, false) => {
                    LLVMBuildICmp(self.builder, IP::LLVMIntSLT, left.value, right.value, name)
                }
                (B::LtEq, false) => {
                    LLVMBuildICmp(self.builder, IP::LLVMIntSLE, left.value, right.value, name)
                }
                (B::Gt, false) => {
                    LLVMBuildICmp(self.builder, IP::LLVMIntSGT, left.value, right.value, name)
                }
                (B::GtEq, false) => {
                    LLVMBuildICmp(self.builder, IP::LLVMIntSGE, left.value, right.value, name)
                }
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

    fn array_element(&mut self, array: Value, index: Value) -> Result<LLVMValueRef, Error> {
        Ok(self
            .external("nerv_array_i64_at", self.pointer_ty(), &[array, index])?
            .value)
    }
    fn array_store(&mut self, array: Value, index: Value, value: Value) -> Result<(), Error> {
        let pointer = self.array_element(array, index)?;
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
