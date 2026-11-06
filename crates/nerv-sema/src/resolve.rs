//! Resolution is part of semantic analysis, never an LLVM backend responsibility.
use std::collections::{HashMap, HashSet};

use super::{
    Binding, Error, LoadedModule, VariantInfo, all_functions, callee_name, error_at, expr_type, ir,
};
use nerv_syntax::{self as ast, ExprKind as E, StatementKind as S, Type as T};

#[derive(Clone)]
struct Local {
    binding: ir::Binding,
    ty: T,
}
type Scope = HashMap<String, Local>;

struct Resolver<'a> {
    next: u32,
    functions: HashMap<&'a str, &'a ast::Function>,
    function_ids: HashMap<&'a str, ir::SymbolId>,
    structs: HashMap<&'a str, &'a ast::Struct>,
    nominals: HashMap<&'a str, ir::SymbolId>,
    variants: HashMap<&'a str, VariantInfo>,
    constructors: HashMap<&'a str, (ir::SymbolId, ir::SymbolId, i64)>,
    lifted: Vec<ir::Function>,
}

pub(super) fn resolve(modules: &[LoadedModule]) -> Result<ir::CheckedProgram, Error> {
    let mut resolver = Resolver {
        next: 0,
        functions: HashMap::new(),
        function_ids: HashMap::new(),
        structs: HashMap::new(),
        nominals: HashMap::new(),
        variants: HashMap::new(),
        constructors: HashMap::new(),
        lifted: Vec::new(),
    };
    // Allocate global IDs before visiting any body, supporting forward calls and
    // recursion. Allocation order follows source/module order, not HashMap order.
    for loaded in modules {
        for structure in &loaded.module.structs {
            let id = resolver.id();
            if resolver.nominals.insert(&structure.name, id).is_some() {
                return Err(failure(structure.span, "duplicate type"));
            }
            resolver.structs.insert(&structure.name, structure);
        }
        for enumeration in &loaded.module.enums {
            let id = resolver.id();
            if resolver.nominals.insert(&enumeration.name, id).is_some() {
                return Err(failure(enumeration.span, "duplicate type"));
            }
            for (tag, variant) in enumeration.variants.iter().enumerate() {
                let variant_id = resolver.id();
                resolver
                    .constructors
                    .insert(&variant.name, (id, variant_id, tag as i64));
                resolver.variants.insert(
                    &variant.name,
                    VariantInfo {
                        enumeration: enumeration.name.clone(),
                        payload: variant.fields.first().cloned(),
                        variants: enumeration
                            .variants
                            .iter()
                            .map(|v| v.name.clone())
                            .collect(),
                    },
                );
            }
        }
        for function in all_functions(&loaded.module) {
            let id = resolver.id();
            resolver.functions.insert(&function.name, function);
            resolver.function_ids.insert(&function.name, id);
        }
    }
    let mut program = ir::CheckedProgram {
        functions: Vec::new(),
        structs: Vec::new(),
        enums: Vec::new(),
        main: None,
        process_entry: false,
        tests: Vec::new(),
        benches: Vec::new(),
    };
    for loaded in modules {
        for structure in &loaded.module.structs {
            let mut names = HashSet::new();
            let mut fields = Vec::new();
            for field in &structure.fields {
                if !names.insert(&field.name) {
                    return Err(failure(field.span, "duplicate struct field"));
                }
                if field.ty == T::Void {
                    return Err(failure(field.span, "struct field cannot have type void"));
                }
                fields.push(resolver.binding(&field.name, &field.ty, false, field.span)?);
            }
            program.structs.push(ir::Struct {
                id: resolver.nominals[structure.name.as_str()],
                name: structure.name.clone(),
                fields,
                span: structure.span,
            });
        }
        for enumeration in &loaded.module.enums {
            let mut variants = Vec::new();
            for variant in &enumeration.variants {
                if variant.fields.len() > 1 {
                    return Err(failure(
                        variant.span,
                        "enum variants require at most one payload value",
                    ));
                }
                let (_, id, tag) = resolver.constructors[variant.name.as_str()];
                variants.push(ir::Variant {
                    id,
                    name: variant.name.clone(),
                    tag,
                    payload: variant
                        .fields
                        .first()
                        .map(|ty| {
                            if *ty == T::Void {
                                return Err(failure(
                                    variant.span,
                                    "enum payload cannot have type void",
                                ));
                            }
                            resolver.ty(ty, variant.span)
                        })
                        .transpose()?,
                    span: variant.span,
                });
            }
            program.enums.push(ir::Enum {
                id: resolver.nominals[enumeration.name.as_str()],
                name: enumeration.name.clone(),
                variants,
                span: enumeration.span,
            });
        }
        for function in all_functions(&loaded.module) {
            let id = resolver.function_ids[function.name.as_str()];
            let checked = resolver.function(function, id)?;
            if function.name == "main" {
                if !checked.params.is_empty()
                    || !matches!(checked.result, ir::Type::Int | ir::Type::Void)
                {
                    return Err(failure(
                        function.span,
                        "`main` must have type fn() -> i64 or fn() -> void",
                    ));
                }
                program.main = Some(id);
            }
            if function.name.starts_with("__nerv_test_") {
                program.tests.push(id);
            }
            if function.name.starts_with("__nerv_bench_") {
                program.benches.push(id);
            }
            program.functions.push(checked);
        }
    }
    program.functions.extend(resolver.lifted);
    validate_layouts(&program)?;
    Ok(program)
}

fn failure(span: ast::Span, message: impl Into<String>) -> Error {
    error_at(std::path::Path::new("<buffer>"), 0, 0, message).at(span)
}

impl Resolver<'_> {
    fn id(&mut self) -> ir::SymbolId {
        let id = ir::SymbolId(self.next);
        self.next += 1;
        id
    }

    fn ty(&self, ty: &T, span: ast::Span) -> Result<ir::Type, Error> {
        Ok(match ty {
            T::Applied { .. } => return Err(failure(span, "unresolved generic type")),
            T::Void => ir::Type::Void,
            T::Bool => ir::Type::Bool,
            T::Int => ir::Type::Int,
            T::Integer { bits, signed } => ir::Type::Integer {
                bits: *bits,
                signed: *signed,
            },
            T::Float32 => ir::Type::Float32,
            T::Float => ir::Type::Float,
            T::String => ir::Type::String,
            T::Array(element) => {
                if **element == T::Void {
                    return Err(failure(span, "array element cannot have type void"));
                }
                ir::Type::Array(Box::new(self.ty(element, span)?))
            }
            T::Function { params, result } => ir::Type::Function {
                params: params
                    .iter()
                    .map(|ty| {
                        if *ty == T::Void {
                            return Err(failure(span, "parameter cannot have type void"));
                        }
                        self.ty(ty, span)
                    })
                    .collect::<Result<_, _>>()?,
                result: Box::new(self.ty(result, span)?),
            },
            T::Named(name) => ir::Type::Named(
                *self
                    .nominals
                    .get(name.as_str())
                    .ok_or_else(|| failure(span, format!("unknown type `{name}`")))?,
            ),
        })
    }

    fn binding(
        &mut self,
        name: &str,
        ty: &T,
        mutable: bool,
        span: ast::Span,
    ) -> Result<ir::Binding, Error> {
        Ok(ir::Binding {
            id: self.id(),
            name: name.into(),
            ty: self.ty(ty, span)?,
            mutable,
            span,
        })
    }

    fn parameters(
        &mut self,
        params: &[ast::Parameter],
    ) -> Result<(Vec<ir::Binding>, Scope), Error> {
        let mut scope = Scope::new();
        let mut bindings = Vec::new();
        for param in params {
            if scope.contains_key(&param.name) {
                return Err(failure(param.span, "duplicate parameter"));
            }
            if param.ty == T::Void {
                return Err(failure(param.span, "parameter cannot have type void"));
            }
            let binding = self.binding(&param.name, &param.ty, false, param.span)?;
            scope.insert(
                param.name.clone(),
                Local {
                    binding: binding.clone(),
                    ty: param.ty.clone(),
                },
            );
            bindings.push(binding);
        }
        Ok((bindings, scope))
    }

    fn function(
        &mut self,
        function: &ast::Function,
        id: ir::SymbolId,
    ) -> Result<ir::Function, Error> {
        let (params, mut scope) = self.parameters(&function.params)?;
        let body = self.block(&function.body, &mut scope)?;
        Ok(ir::Function {
            id,
            name: function.name.clone(),
            params,
            result: self.ty(&function.result, function.span)?,
            body,
            span: function.span,
        })
    }

    fn inferred(&self, expression: &ast::Expr, scope: &Scope) -> Result<T, Error> {
        let values = scope
            .iter()
            .map(|(name, local)| {
                (
                    name.clone(),
                    Binding {
                        ty: local.ty.clone(),
                        mutable: local.binding.mutable,
                    },
                )
            })
            .collect();
        expr_type(
            std::path::Path::new("<buffer>"),
            expression,
            &values,
            &self.functions,
            &self.structs,
            &self.variants,
        )
    }

    fn field_index(&self, ty: &T, name: &str, span: ast::Span) -> Result<u32, Error> {
        let T::Named(structure) = ty else {
            return Err(failure(span, "field access requires a struct"));
        };
        self.structs
            .get(structure.as_str())
            .and_then(|structure| structure.fields.iter().position(|field| field.name == name))
            .map(|index| index as u32)
            .ok_or_else(|| failure(span, format!("unknown field `{name}`")))
    }

    fn block(
        &mut self,
        body: &[ast::Statement],
        scope: &mut Scope,
    ) -> Result<Vec<ir::Statement>, Error> {
        body.iter()
            .map(|statement| self.statement(statement, scope))
            .collect()
    }

    fn statement(
        &mut self,
        statement: &ast::Statement,
        scope: &mut Scope,
    ) -> Result<ir::Statement, Error> {
        use ir::StatementKind as K;
        let kind = match &statement.kind {
            S::Let {
                name,
                mutable,
                value,
                ..
            } => {
                let actual = self.inferred(value, scope)?;
                let value = self.expression(value, scope)?;
                let binding = self.binding(name, &actual, *mutable, statement.span)?;
                scope.insert(
                    name.clone(),
                    Local {
                        binding: binding.clone(),
                        ty: actual,
                    },
                );
                K::Let { binding, value }
            }
            S::Assign { name, value } => K::Assign {
                target: scope[name].binding.id,
                value: self.expression(value, scope)?,
            },
            S::AssignIndex { base, index, value } => K::AssignIndex {
                base: self.expression(base, scope)?,
                index: self.expression(index, scope)?,
                value: self.expression(value, scope)?,
            },
            S::AssignField { base, name, value } => {
                let E::Name(local) = &base.kind else {
                    return Err(failure(
                        base.span,
                        "field assignment requires a named value",
                    ));
                };
                K::AssignField {
                    target: scope[local].binding.id,
                    index: self.field_index(&scope[local].ty, name, base.span)?,
                    value: self.expression(value, scope)?,
                }
            }
            S::While { condition, body } => K::While {
                condition: self.expression(condition, scope)?,
                body: self.block(body, &mut scope.clone())?,
            },
            S::For {
                name,
                start,
                end,
                inclusive,
                body,
            } => {
                let start = self.expression(start, scope)?;
                let end = self.expression(end, scope)?;
                let binding = self.binding(name, &T::Int, false, statement.span)?;
                let mut inner = scope.clone();
                inner.insert(
                    name.clone(),
                    Local {
                        binding: binding.clone(),
                        ty: T::Int,
                    },
                );
                K::For {
                    binding,
                    start,
                    end,
                    inclusive: *inclusive,
                    body: self.block(body, &mut inner)?,
                }
            }
            S::Return(value) => K::Return(self.expression(value, scope)?),
            S::Expr(value) => K::Expr(self.expression(value, scope)?),
        };
        let ty = if let K::Expr(expr) = &kind {
            expr.ty.clone()
        } else {
            ir::Type::Void
        };
        Ok(ir::Statement {
            span: statement.span,
            ty,
            kind,
        })
    }

    fn expression(&mut self, expression: &ast::Expr, scope: &Scope) -> Result<ir::Expr, Error> {
        use ir::ExprKind as K;
        let actual = self.inferred(expression, scope)?;
        let ty = self.ty(&actual, expression.span)?;
        let kind = match &expression.kind {
            E::Int(value) => K::Int(*value),
            E::Float(value) => K::Float(*value),
            E::Bool(value) => K::Bool(*value),
            E::String(value) => K::String(value.clone()),
            E::Name(name) => {
                if let Some(local) = scope.get(name) {
                    K::Local(local.binding.id)
                } else {
                    K::Function(self.function_ids[name.as_str()])
                }
            }
            E::Array(values) => K::Array(
                values
                    .iter()
                    .map(|value| self.expression(value, scope))
                    .collect::<Result<_, _>>()?,
            ),
            E::ArrayRepeat { value, length } => K::ArrayRepeat {
                value: Box::new(self.expression(value, scope)?),
                length: Box::new(self.expression(length, scope)?),
            },
            E::Struct { name, fields } => {
                let structure = self.nominals[name.as_str()];
                let mut seen = HashSet::new();
                let mut checked = Vec::new();
                for (field, value) in fields {
                    let index = self.field_index(&actual, field, expression.span)?;
                    if !seen.insert(index) {
                        return Err(failure(value.span, format!("duplicate field `{field}`")));
                    }
                    checked.push((index, self.expression(value, scope)?));
                }
                K::Struct {
                    structure,
                    fields: checked,
                }
            }
            E::Field { base, name } => {
                let index =
                    self.field_index(&self.inferred(base, scope)?, name, expression.span)?;
                K::Field {
                    base: Box::new(self.expression(base, scope)?),
                    index,
                }
            }
            E::Index { base, index } => K::Index {
                base: Box::new(self.expression(base, scope)?),
                index: Box::new(self.expression(index, scope)?),
            },
            E::Unary { op, value } => K::Unary {
                op: *op,
                value: Box::new(self.expression(value, scope)?),
            },
            E::Binary { left, op, right } => K::Binary {
                left: Box::new(self.expression(left, scope)?),
                op: *op,
                right: Box::new(self.expression(right, scope)?),
            },
            E::If {
                condition,
                then_body,
                else_body,
            } => K::If {
                condition: Box::new(self.expression(condition, scope)?),
                then_body: self.block(then_body, &mut scope.clone())?,
                else_body: self.block(else_body, &mut scope.clone())?,
            },
            E::Match { value, arms } => {
                let T::Named(enumeration) = self.inferred(value, scope)? else {
                    unreachable!("checked match")
                };
                let enumeration = self.nominals[enumeration.as_str()];
                let value = Box::new(self.expression(value, scope)?);
                let mut checked = Vec::new();
                for arm in arms {
                    let (_, variant, tag) = self.constructors[arm.variant.as_str()];
                    let mut inner = scope.clone();
                    let binding = if let Some(name) = &arm.binding {
                        let payload = self.variants[arm.variant.as_str()]
                            .payload
                            .clone()
                            .expect("checked payload binding");
                        let binding = self.binding(name, &payload, false, arm.span)?;
                        inner.insert(
                            name.clone(),
                            Local {
                                binding: binding.clone(),
                                ty: payload,
                            },
                        );
                        Some(binding)
                    } else {
                        None
                    };
                    checked.push(ir::MatchArm {
                        variant,
                        tag,
                        binding,
                        body: self.expression(&arm.body, &inner)?,
                        span: arm.span,
                    });
                }
                K::Match {
                    value,
                    enumeration,
                    arms: checked,
                }
            }
            E::Lambda {
                params,
                result,
                body,
            } => {
                let id = self.id();
                let (params, inner) = self.parameters(params)?;
                let body = self.expression(body, &inner)?;
                self.lifted.push(ir::Function {
                    id,
                    name: format!("lambda_{}", id.0),
                    params,
                    result: self.ty(result, expression.span)?,
                    span: expression.span,
                    body: vec![ir::Statement {
                        span: body.span,
                        ty: body.ty.clone(),
                        kind: ir::StatementKind::Expr(body),
                    }],
                });
                K::Function(id)
            }
            E::Method { base, name, args } => {
                let T::Named(type_name) = self.inferred(base, scope)? else {
                    unreachable!("checked receiver")
                };
                let id = self.function_ids[format!("{type_name}__{name}").as_str()];
                let mut checked = vec![self.expression(base, scope)?];
                checked.extend(
                    args.iter()
                        .map(|arg| self.expression(arg, scope))
                        .collect::<Result<Vec<_>, _>>()?,
                );
                K::Call {
                    target: ir::CallTarget::Direct(id),
                    args: checked,
                }
            }
            E::Call { callee, args } => {
                let name = callee_name(callee);
                let checked = args
                    .iter()
                    .map(|arg| self.expression(arg, scope))
                    .collect::<Result<Vec<_>, _>>()?;
                match name.as_deref() {
                    Some("assert") => K::Assert(Box::new(checked.into_iter().next().unwrap())),
                    Some(name) if ast::numeric_type(name).is_some() => {
                        let value = checked.into_iter().next().unwrap();
                        let conversion = match (&value.ty, &ty) {
                            (ir::Type::Int, ir::Type::Float) => ir::Conversion::IntToFloat,
                            (ir::Type::Float, ir::Type::Int) => ir::Conversion::FloatToInt,
                            _ if value.ty == ty => ir::Conversion::Identity,
                            _ => ir::Conversion::Numeric,
                        };
                        K::Convert {
                            conversion,
                            value: Box::new(value),
                        }
                    }
                    Some(name)
                        if super::runtime_type(
                            name,
                            &args
                                .iter()
                                .map(|arg| self.inferred(arg, scope))
                                .collect::<Result<Vec<_>, _>>()?,
                        )
                        .is_some() =>
                    {
                        let checked = if matches!(name, "print" | "println") {
                            checked
                                .into_iter()
                                .map(|value| {
                                    let ty = match value.ty {
                                        ir::Type::Integer { signed: true, .. } => ir::Type::Int,
                                        ir::Type::Integer { signed: false, .. } => {
                                            ir::Type::Integer {
                                                bits: 64,
                                                signed: false,
                                            }
                                        }
                                        ir::Type::Float32 => ir::Type::Float,
                                        _ => return value,
                                    };
                                    ir::Expr {
                                        span: value.span,
                                        ty,
                                        kind: K::Convert {
                                            conversion: ir::Conversion::Numeric,
                                            value: Box::new(value),
                                        },
                                    }
                                })
                                .collect()
                        } else {
                            checked
                        };
                        K::Runtime {
                            symbol: runtime_symbol(name, &checked),
                            args: checked,
                        }
                    }
                    Some(name)
                        if self
                            .constructors
                            .contains_key(name.rsplit('.').next().unwrap()) =>
                    {
                        if let Some((qualifier, variant)) = name.rsplit_once('.')
                            && qualifier != self.variants[variant].enumeration
                        {
                            return Err(failure(
                                callee.span,
                                format!("unknown enum constructor `{name}`"),
                            ));
                        }
                        let (enumeration, variant, tag) =
                            self.constructors[name.rsplit('.').next().unwrap()];
                        K::Enum {
                            enumeration,
                            variant,
                            tag,
                            payload: checked.into_iter().next().map(Box::new),
                        }
                    }
                    Some(name)
                        if !scope
                            .get(name)
                            .is_some_and(|local| matches!(local.ty, T::Function { .. }))
                            && self.function_ids.contains_key(name) =>
                    {
                        K::Call {
                            target: ir::CallTarget::Direct(self.function_ids[name]),
                            args: checked,
                        }
                    }
                    _ => K::Call {
                        target: ir::CallTarget::Indirect(Box::new(self.expression(callee, scope)?)),
                        args: checked,
                    },
                }
            }
        };
        Ok(ir::Expr {
            span: expression.span,
            ty,
            kind,
        })
    }
}

fn runtime_symbol(name: &str, args: &[ir::Expr]) -> String {
    match name {
        "print" | "println" => {
            let suffix = match args[0].ty {
                ir::Type::Int => "i64",
                ir::Type::Integer { signed: false, .. } => "u64",
                ir::Type::Float => "f64",
                ir::Type::Bool => "bool",
                ir::Type::String => "string",
                _ => unreachable!("checked print"),
            };
            format!("nerv_{name}_{suffix}")
        }
        "nerv.arrays.len" => "nerv_array_length".into(),
        name if name.starts_with("nerv.json.") => {
            format!("nerv_json_{}", name.trim_start_matches("nerv.json."))
        }
        name if name.starts_with("nerv.net.") => {
            format!("nerv_net_{}", name.trim_start_matches("nerv.net."))
        }
        name if name.starts_with("nerv.io.") => {
            format!("nerv_io_{}", name.trim_start_matches("nerv.io."))
        }
        "nerv.math.sqrt" => "nerv_sqrt".into(),
        "nerv.math.pow" => "nerv_pow".into(),
        "nerv.math.abs" => if args[0].ty == ir::Type::Int {
            "nerv_abs_i64"
        } else {
            "nerv_abs_f64"
        }
        .into(),
        "nerv.string.len" => "nerv_string_len".into(),
        "nerv.string.concat" => "nerv_string_concat".into(),
        "nerv.random" => "nerv_random_i64".into(),
        name if name.starts_with("nerv.string.") => {
            format!("nerv_str_{}", name.trim_start_matches("nerv.string."))
        }
        name if name.starts_with("nerv.arrays.") => {
            format!("nerv_array_i64_{}", name.trim_start_matches("nerv.arrays."))
        }
        name if name.starts_with("nerv.collections.") => {
            format!("nerv_{}", name.trim_start_matches("nerv.collections."))
        }
        _ => unreachable!("checked runtime symbol"),
    }
}

fn validate_layouts(program: &ir::CheckedProgram) -> Result<(), Error> {
    fn visit(
        id: ir::SymbolId,
        layouts: &HashMap<ir::SymbolId, (ast::Span, Vec<ir::SymbolId>, bool)>,
        active: &mut HashSet<ir::SymbolId>,
        done: &mut HashSet<ir::SymbolId>,
    ) -> Result<(), Error> {
        if done.contains(&id) {
            return Ok(());
        }
        let (span, children, is_struct) = &layouts[&id];
        if !active.insert(id) {
            return Err(failure(
                *span,
                if *is_struct {
                    "recursive struct has infinite size"
                } else {
                    "recursive enum has infinite size"
                },
            ));
        }
        for child in children {
            visit(*child, layouts, active, done)?;
        }
        active.remove(&id);
        done.insert(id);
        Ok(())
    }
    let children = |types: Vec<&ir::Type>| {
        types
            .into_iter()
            .filter_map(|ty| {
                if let ir::Type::Named(id) = ty {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    };
    let mut layouts = HashMap::new();
    for structure in &program.structs {
        layouts.insert(
            structure.id,
            (
                structure.span,
                children(structure.fields.iter().map(|f| &f.ty).collect()),
                true,
            ),
        );
    }
    for enumeration in &program.enums {
        layouts.insert(
            enumeration.id,
            (
                enumeration.span,
                children(
                    enumeration
                        .variants
                        .iter()
                        .filter_map(|v| v.payload.as_ref())
                        .collect(),
                ),
                false,
            ),
        );
    }
    let mut done = HashSet::new();
    for id in layouts.keys() {
        visit(*id, &layouts, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}
