use super::*;
use syntax::GenericParameter;

struct Expansion {
    modules: Vec<LoadedModule>,
    functions: HashMap<String, (usize, Function)>,
    structs: HashMap<String, (usize, StructDecl)>,
    enums: HashMap<String, (usize, syntax::Enum)>,
    instances: Vec<(String, Vec<Type>, String)>,
}

pub(super) fn expand(modules: &[LoadedModule]) -> Result<Vec<LoadedModule>, Errors> {
    let mut functions = HashSet::new();
    let mut types = HashSet::new();
    for loaded in modules {
        for function in all_functions(&loaded.module) {
            if !functions.insert(&function.name) {
                return Err(error_at(
                    &loaded.path,
                    function.line,
                    function.column,
                    "duplicate function",
                )
                .at(function.span)
                .into());
            }
        }
        for (name, span) in loaded
            .module
            .structs
            .iter()
            .map(|s| (&s.name, s.span))
            .chain(loaded.module.enums.iter().map(|e| (&e.name, e.span)))
        {
            if !types.insert(name) {
                return Err(error_at(&loaded.path, 0, 0, "duplicate type")
                    .at(span)
                    .into());
            }
        }
    }
    let mut expansion = Expansion {
        modules: modules.to_vec(),
        functions: HashMap::new(),
        structs: HashMap::new(),
        enums: HashMap::new(),
        instances: Vec::new(),
    };
    for (index, loaded) in expansion.modules.iter_mut().enumerate() {
        loaded.module.functions.retain(|function| {
            if function.generics.is_empty() {
                return true;
            }
            expansion
                .functions
                .insert(function.name.clone(), (index, function.clone()));
            false
        });
        loaded.module.structs.retain(|structure| {
            if structure.generics.is_empty() {
                return true;
            }
            expansion
                .structs
                .insert(structure.name.clone(), (index, structure.clone()));
            false
        });
        loaded.module.enums.retain(|enumeration| {
            if enumeration.generics.is_empty() {
                return true;
            }
            expansion
                .enums
                .insert(enumeration.name.clone(), (index, enumeration.clone()));
            false
        });
    }
    if expansion.functions.is_empty() && expansion.structs.is_empty() && expansion.enums.is_empty()
    {
        return Ok(expansion.modules);
    }
    for index in 0..expansion.modules.len() {
        let mut module = expansion.modules[index].module.clone();
        for structure in &mut module.structs {
            for field in &mut structure.fields {
                expansion.ty(&mut field.ty, &HashMap::new(), field.span)?;
            }
        }
        for enumeration in &mut module.enums {
            for variant in &mut enumeration.variants {
                for ty in &mut variant.fields {
                    expansion.ty(ty, &HashMap::new(), variant.span)?;
                }
            }
        }
        for function in module
            .functions
            .iter_mut()
            .chain(module.structs.iter_mut().flat_map(|s| &mut s.methods))
            .chain(module.impls.iter_mut().flat_map(|i| &mut i.methods))
        {
            expansion.signature(function, &HashMap::new())?;
        }
        let original = &mut expansion.modules[index].module;
        original
            .functions
            .splice(..module.functions.len(), module.functions);
        original
            .structs
            .splice(..module.structs.len(), module.structs);
        original.enums.splice(..module.enums.len(), module.enums);
        original.impls = module.impls;
    }
    let mut processed = HashSet::new();
    loop {
        let next = expansion
            .modules
            .iter()
            .enumerate()
            .find_map(|(index, loaded)| {
                all_functions(&loaded.module)
                    .find(|f| !processed.contains(&f.name))
                    .map(|f| (index, f.clone()))
            });
        let Some((index, mut function)) = next else {
            break;
        };
        processed.insert(function.name.clone());
        let mut scope = function
            .params
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    Binding {
                        ty: p.ty.clone(),
                        mutable: false,
                    },
                )
            })
            .collect();
        expansion.block(&mut function.body, &mut scope, &function.result)?;
        let module = &mut expansion.modules[index].module;
        let target = module
            .functions
            .iter_mut()
            .chain(module.structs.iter_mut().flat_map(|s| &mut s.methods))
            .chain(module.impls.iter_mut().flat_map(|i| &mut i.methods))
            .find(|f| f.name == function.name)
            .unwrap();
        *target = function;
    }
    Ok(expansion.modules)
}

impl Expansion {
    fn error(&self, span: Span, message: impl Into<String>) -> Error {
        error_at(Path::new("<buffer>"), 0, 0, message).at(span)
    }

    fn signature(
        &mut self,
        function: &mut Function,
        substitutions: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        for parameter in &mut function.params {
            self.ty(&mut parameter.ty, substitutions, parameter.span)?;
        }
        self.ty(&mut function.result, substitutions, function.span)
    }

    fn ty(
        &mut self,
        ty: &mut Type,
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Result<(), Error> {
        match ty {
            Type::Named(name) => {
                if let Some(replacement) = substitutions.get(name) {
                    *ty = replacement.clone();
                }
            }
            Type::Array(element) => self.ty(element, substitutions, span)?,
            Type::Function { params, result } => {
                for parameter in params {
                    self.ty(parameter, substitutions, span)?;
                }
                self.ty(result, substitutions, span)?;
            }
            Type::Applied { name, args } => {
                for arg in args.iter_mut() {
                    self.ty(arg, substitutions, span)?;
                }
                *ty = Type::Named(self.instantiate(name, args, span)?);
            }
            _ => {}
        }
        Ok(())
    }

    fn instantiate(&mut self, name: &str, args: &[Type], span: Span) -> Result<String, Error> {
        if let Some((_, _, instance)) = self
            .instances
            .iter()
            .find(|(n, a, _)| n == name && a == args)
        {
            return Ok(instance.clone());
        }
        let params = if let Some((_, f)) = self.functions.get(name) {
            &f.generics
        } else if let Some((_, s)) = self.structs.get(name) {
            &s.generics
        } else if let Some((_, e)) = self.enums.get(name) {
            &e.generics
        } else {
            return Err(self.error(span, format!("unknown generic `{name}`")));
        };
        if params.len() != args.len() {
            return Err(self.error(
                span,
                format!(
                    "`{name}` expects {} type arguments, found {}",
                    params.len(),
                    args.len()
                ),
            ));
        }
        for (param, arg) in params.iter().zip(args) {
            if let Some(constraint) = &param.constraint {
                let Type::Named(actual) = arg else {
                    return Err(self.error(
                        span,
                        format!("type {arg:?} does not implement `{constraint}`"),
                    ));
                };
                let implemented = self.modules.iter().any(|m| {
                    m.module
                        .structs
                        .iter()
                        .any(|s| s.name == *actual && s.implements.as_ref() == Some(constraint))
                        || m.module.impls.iter().any(|i| {
                            i.type_name == *actual && i.trait_name.as_ref() == Some(constraint)
                        })
                });
                if !implemented {
                    return Err(self.error(
                        span,
                        format!("type `{actual}` does not implement `{constraint}`"),
                    ));
                }
            }
        }
        let mut substitutions: HashMap<_, _> = params
            .iter()
            .zip(args)
            .map(|(p, a)| (p.name.clone(), a.clone()))
            .collect();
        if self.instances.len() >= 256 {
            return Err(self.error(span, "generic specialization limit exceeded"));
        }
        let instance = format!("{name}__generic_{}", self.instances.len());
        self.instances
            .push((name.to_owned(), args.to_vec(), instance.clone()));
        if let Some((index, mut function)) = self.functions.get(name).cloned() {
            function.name = instance.clone();
            function.generics.clear();
            self.signature(&mut function, &substitutions)?;
            self.substitute_block(&mut function.body, &substitutions)?;
            self.modules[index].module.functions.push(function);
        } else if let Some((index, mut structure)) = self.structs.get(name).cloned() {
            structure.name = instance.clone();
            structure.generics.clear();
            substitutions.insert(name.to_owned(), Type::Named(instance.clone()));
            for field in &mut structure.fields {
                self.ty(&mut field.ty, &substitutions, field.span)?;
            }
            for method in &mut structure.methods {
                method.name = method.name.replacen(name, &instance, 1);
                self.signature(method, &substitutions)?;
                self.substitute_block(&mut method.body, &substitutions)?;
            }
            self.modules[index].module.structs.push(structure);
        } else if let Some((index, mut enumeration)) = self.enums.get(name).cloned() {
            enumeration.name = instance.clone();
            enumeration.generics.clear();
            for variant in &mut enumeration.variants {
                variant.name = format!("{instance}__{}", variant.name);
                for field in &mut variant.fields {
                    self.ty(field, &substitutions, variant.span)?;
                }
            }
            self.modules[index].module.enums.push(enumeration);
        }
        Ok(instance)
    }

    fn infer(&self, expr: &SpannedExpr, values: &HashMap<String, Binding>) -> Result<Type, Error> {
        let functions = self
            .modules
            .iter()
            .flat_map(|m| all_functions(&m.module))
            .map(|f| (f.name.as_str(), f))
            .collect();
        let structs = self
            .modules
            .iter()
            .flat_map(|m| &m.module.structs)
            .map(|s| (s.name.as_str(), s))
            .collect();
        let mut variants = HashMap::new();
        for enumeration in self.modules.iter().flat_map(|m| &m.module.enums) {
            for variant in &enumeration.variants {
                variants.insert(
                    variant.name.as_str(),
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
        expr_type(
            Path::new("<buffer>"),
            expr,
            values,
            &functions,
            &structs,
            &variants,
        )
    }

    fn unify(
        &self,
        pattern: &Type,
        actual: &Type,
        params: &[GenericParameter],
        substitutions: &mut HashMap<String, Type>,
        span: Span,
    ) -> Result<(), Error> {
        match (pattern, actual) {
            (Type::Named(name), _) if params.iter().any(|p| p.name == *name) => {
                if let Some(previous) = substitutions.insert(name.clone(), actual.clone()) {
                    if previous != *actual {
                        return Err(self.error(span, format!("conflicting types for `{name}`")));
                    }
                }
            }
            (Type::Array(p), Type::Array(a)) => self.unify(p, a, params, substitutions, span)?,
            (Type::Applied { name, args }, Type::Named(instance)) => {
                let Some((_, actual_args, _)) = self
                    .instances
                    .iter()
                    .find(|(n, _, i)| n == name && i == instance)
                else {
                    return Err(self.error(span, "generic argument type mismatch"));
                };
                for (p, a) in args.iter().zip(actual_args) {
                    self.unify(p, a, params, substitutions, span)?;
                }
            }
            (
                Type::Function {
                    params: pp,
                    result: pr,
                },
                Type::Function {
                    params: ap,
                    result: ar,
                },
            ) if pp.len() == ap.len() => {
                for (p, a) in pp.iter().zip(ap) {
                    self.unify(p, a, params, substitutions, span)?;
                }
                self.unify(pr, ar, params, substitutions, span)?;
            }
            _ if pattern == actual => {}
            _ => return Err(self.error(span, "generic argument type mismatch")),
        }
        Ok(())
    }

    fn arguments(
        &self,
        params: &[GenericParameter],
        substitutions: &HashMap<String, Type>,
        span: Span,
    ) -> Result<Vec<Type>, Error> {
        params
            .iter()
            .map(|p| {
                substitutions.get(&p.name).cloned().ok_or_else(|| {
                    self.error(span, format!("cannot infer type parameter `{}`", p.name))
                })
            })
            .collect()
    }

    fn substitute_block(
        &mut self,
        body: &mut [SpannedStatement],
        substitutions: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        for statement in body {
            match &mut statement.kind {
                Statement::Let { ty, value, .. } => {
                    if let Some(ty) = ty {
                        self.ty(ty, substitutions, statement.span)?;
                    }
                    self.substitute_expr(value, substitutions)?;
                }
                Statement::Assign { value, .. }
                | Statement::Return(value)
                | Statement::Expr(value) => self.substitute_expr(value, substitutions)?,
                Statement::AssignIndex { base, index, value } => {
                    for expr in [base, index, value] {
                        self.substitute_expr(expr, substitutions)?;
                    }
                }
                Statement::AssignField { base, value, .. } => {
                    self.substitute_expr(base, substitutions)?;
                    self.substitute_expr(value, substitutions)?;
                }
                Statement::While { condition, body } => {
                    self.substitute_expr(condition, substitutions)?;
                    self.substitute_block(body, substitutions)?;
                }
                Statement::For {
                    start, end, body, ..
                } => {
                    self.substitute_expr(start, substitutions)?;
                    self.substitute_expr(end, substitutions)?;
                    self.substitute_block(body, substitutions)?;
                }
            }
        }
        Ok(())
    }

    fn substitute_expr(
        &mut self,
        expression: &mut SpannedExpr,
        substitutions: &HashMap<String, Type>,
    ) -> Result<(), Error> {
        match &mut expression.kind {
            Expr::Lambda {
                params,
                result,
                body,
            } => {
                for p in params {
                    self.ty(&mut p.ty, substitutions, p.span)?;
                }
                self.ty(result, substitutions, expression.span)?;
                self.substitute_expr(body, substitutions)?;
            }
            Expr::If {
                condition,
                then_body,
                else_body,
            } => {
                self.substitute_expr(condition, substitutions)?;
                self.substitute_block(then_body, substitutions)?;
                self.substitute_block(else_body, substitutions)?;
            }
            Expr::Match { value, arms } => {
                self.substitute_expr(value, substitutions)?;
                for arm in arms {
                    self.substitute_expr(&mut arm.body, substitutions)?;
                }
            }
            Expr::Array(values) => {
                for value in values {
                    self.substitute_expr(value, substitutions)?;
                }
            }
            Expr::ArrayRepeat {
                value,
                length: other,
            }
            | Expr::Binary {
                left: value,
                right: other,
                ..
            }
            | Expr::Index {
                base: value,
                index: other,
            } => {
                self.substitute_expr(value, substitutions)?;
                self.substitute_expr(other, substitutions)?;
            }
            Expr::Call { callee, args }
            | Expr::Method {
                base: callee, args, ..
            } => {
                self.substitute_expr(callee, substitutions)?;
                for arg in args {
                    self.substitute_expr(arg, substitutions)?;
                }
            }
            Expr::Struct { fields, .. } => {
                for (_, value) in fields {
                    self.substitute_expr(value, substitutions)?;
                }
            }
            Expr::Unary { value, .. } | Expr::Field { base: value, .. } => {
                self.substitute_expr(value, substitutions)?
            }
            _ => {}
        }
        Ok(())
    }

    fn block(
        &mut self,
        body: &mut [SpannedStatement],
        scope: &mut HashMap<String, Binding>,
        result: &Type,
    ) -> Result<(), Error> {
        let last = body.len().saturating_sub(1);
        for (index, statement) in body.iter_mut().enumerate() {
            match &mut statement.kind {
                Statement::Let {
                    name,
                    mutable,
                    ty,
                    value,
                    ..
                } => {
                    if let Some(ty) = ty {
                        self.ty(ty, &HashMap::new(), statement.span)?;
                    }
                    self.expression(value, scope, ty.as_ref())?;
                    scope.insert(
                        name.clone(),
                        Binding {
                            ty: ty.clone().unwrap_or(self.infer(value, scope)?),
                            mutable: *mutable,
                        },
                    );
                }
                Statement::Return(value) => self.expression(value, scope, Some(result))?,
                Statement::Expr(value) => self.expression(
                    value,
                    scope,
                    if index == last { Some(result) } else { None },
                )?,
                Statement::Assign { name, value } => {
                    self.expression(value, scope, scope.get(name).map(|b| &b.ty))?
                }
                Statement::AssignIndex { base, index, value } => {
                    for expr in [base, index, value] {
                        self.expression(expr, scope, None)?;
                    }
                }
                Statement::AssignField { base, value, .. } => {
                    self.expression(base, scope, None)?;
                    self.expression(value, scope, None)?;
                }
                Statement::While { condition, body } => {
                    self.expression(condition, scope, None)?;
                    self.block(body, &mut scope.clone(), result)?;
                }
                Statement::For {
                    name,
                    start,
                    end,
                    body,
                    ..
                } => {
                    self.expression(start, scope, None)?;
                    self.expression(end, scope, None)?;
                    let mut inner = scope.clone();
                    inner.insert(
                        name.clone(),
                        Binding {
                            ty: Type::Int,
                            mutable: false,
                        },
                    );
                    self.block(body, &mut inner, result)?;
                }
            }
        }
        Ok(())
    }

    fn expression(
        &mut self,
        expression: &mut SpannedExpr,
        scope: &HashMap<String, Binding>,
        expected: Option<&Type>,
    ) -> Result<(), Error> {
        let span = expression.span;
        match &mut expression.kind {
            Expr::Call { callee, args } => {
                for arg in args.iter_mut() {
                    self.expression(arg, scope, None)?;
                }
                if let Expr::Name(name) = &mut callee.kind {
                    if let Some((_, template)) = self.functions.get(name).cloned() {
                        if template.params.len() != args.len() {
                            return Err(self.error(span, "wrong argument count"));
                        }
                        let mut substitutions = HashMap::new();
                        for (param, arg) in template.params.iter().zip(args.iter()) {
                            self.unify(
                                &param.ty,
                                &self.infer(arg, scope)?,
                                &template.generics,
                                &mut substitutions,
                                arg.span,
                            )?;
                        }
                        if let Some(expected) = expected.filter(|t| **t != Type::Void) {
                            self.unify(
                                &template.result,
                                expected,
                                &template.generics,
                                &mut substitutions,
                                span,
                            )?;
                        }
                        let types = self.arguments(&template.generics, &substitutions, span)?;
                        *name = self.instantiate(name, &types, span)?;
                    } else {
                        let template = self.enums.values().find_map(|(_, e)| {
                            e.variants
                                .iter()
                                .find(|v| v.name == *name)
                                .map(|v| (e.clone(), v.clone()))
                        });
                        if let Some((enumeration, variant)) = template {
                            if variant.fields.len() != args.len() {
                                return Err(self.error(span, "wrong enum argument count"));
                            }
                            let mut substitutions = HashMap::new();
                            for (field, arg) in variant.fields.iter().zip(args.iter()) {
                                self.unify(
                                    field,
                                    &self.infer(arg, scope)?,
                                    &enumeration.generics,
                                    &mut substitutions,
                                    span,
                                )?;
                            }
                            if let Some(Type::Named(instance)) = expected {
                                if let Some((_, types, _)) = self
                                    .instances
                                    .iter()
                                    .find(|(n, _, i)| *n == enumeration.name && i == instance)
                                {
                                    for (p, ty) in enumeration.generics.iter().zip(types) {
                                        substitutions.insert(p.name.clone(), ty.clone());
                                    }
                                }
                            }
                            let types =
                                self.arguments(&enumeration.generics, &substitutions, span)?;
                            let instance = self.instantiate(&enumeration.name, &types, span)?;
                            *name = format!("{instance}__{name}");
                        }
                    }
                } else {
                    self.expression(callee, scope, None)?;
                }
            }
            Expr::Struct { name, fields } => {
                for (_, value) in fields.iter_mut() {
                    self.expression(value, scope, None)?;
                }
                if let Some((_, template)) = self.structs.get(name).cloned() {
                    let mut substitutions = HashMap::new();
                    for (field, value) in fields.iter() {
                        let declaration = template
                            .fields
                            .iter()
                            .find(|f| f.name == *field)
                            .ok_or_else(|| self.error(span, format!("unknown field `{field}`")))?;
                        self.unify(
                            &declaration.ty,
                            &self.infer(value, scope)?,
                            &template.generics,
                            &mut substitutions,
                            span,
                        )?;
                    }
                    let types = self.arguments(&template.generics, &substitutions, span)?;
                    *name = self.instantiate(name, &types, span)?;
                }
            }
            Expr::Match { value, arms } => {
                self.expression(value, scope, None)?;
                let ty = self.infer(value, scope)?;
                let enumeration = self
                    .modules
                    .iter()
                    .flat_map(|m| &m.module.enums)
                    .find(|e| Type::Named(e.name.clone()) == ty)
                    .cloned();
                for arm in arms {
                    let mut inner = scope.clone();
                    if let Some(enumeration) = &enumeration {
                        let specialized = format!("{}__{}", enumeration.name, arm.variant);
                        let variant = enumeration
                            .variants
                            .iter()
                            .find(|v| v.name == arm.variant || v.name == specialized);
                        if let Some(variant) = variant {
                            arm.variant = variant.name.clone();
                            if let (Some(name), Some(ty)) = (&arm.binding, variant.fields.first()) {
                                inner.insert(
                                    name.clone(),
                                    Binding {
                                        ty: ty.clone(),
                                        mutable: false,
                                    },
                                );
                            }
                        }
                    }
                    self.expression(&mut arm.body, &inner, expected)?;
                }
            }
            Expr::If {
                condition,
                then_body,
                else_body,
            } => {
                self.expression(condition, scope, None)?;
                self.block(
                    then_body,
                    &mut scope.clone(),
                    expected.unwrap_or(&Type::Void),
                )?;
                self.block(
                    else_body,
                    &mut scope.clone(),
                    expected.unwrap_or(&Type::Void),
                )?;
            }
            Expr::Lambda {
                params,
                result,
                body,
            } => {
                for param in params.iter_mut() {
                    self.ty(&mut param.ty, &HashMap::new(), span)?;
                }
                self.ty(result, &HashMap::new(), span)?;
                let inner = params
                    .iter()
                    .map(|p| {
                        (
                            p.name.clone(),
                            Binding {
                                ty: p.ty.clone(),
                                mutable: false,
                            },
                        )
                    })
                    .collect();
                self.expression(body, &inner, Some(result))?;
            }
            Expr::Method { base, args, .. } => {
                self.expression(base, scope, None)?;
                for arg in args {
                    self.expression(arg, scope, None)?;
                }
            }
            Expr::Array(values) => {
                for value in values {
                    self.expression(value, scope, None)?;
                }
            }
            Expr::ArrayRepeat {
                value,
                length: other,
            }
            | Expr::Binary {
                left: value,
                right: other,
                ..
            }
            | Expr::Index {
                base: value,
                index: other,
            } => {
                self.expression(value, scope, None)?;
                self.expression(other, scope, None)?;
            }
            Expr::Unary { value, .. } | Expr::Field { base: value, .. } => {
                self.expression(value, scope, None)?
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_specializations() {
        for (source, expected) in [
            (
                "fn pick<T>(a: T, b: T) -> T => a\nfn main() -> i64 => pick(1, true)\n",
                "conflicting types",
            ),
            (
                "fn empty<T>() -> i64 => 0\nfn main() -> i64 => empty()\n",
                "cannot infer",
            ),
            (
                "fn constrained<T: Show>(a: T) -> T => a\nfn main() -> i64 => constrained(1)\n",
                "does not implement",
            ),
            (
                "struct Box<T>\n    value: T\nfn main(x: Box<i64, bool>) -> i64 => 0\n",
                "expects 1 type arguments",
            ),
            (
                "fn pick<T, T>(a: T) -> T => a\n",
                "duplicate generic parameter",
            ),
        ] {
            let error = match syntax::parse(source) {
                Ok(module) => check_modules(&[module]).unwrap_err().to_string(),
                Err(error) => error.to_string(),
            };
            assert!(error.contains(expected), "{source}: {error}");
        }
    }

    #[test]
    fn checks_trait_constraints_and_generic_methods() {
        let source = "trait Read\n    fn read(self) -> i64\nstruct Value implements Read\n    data: i64\n    fn read(self) -> i64 => self.data\nfn read<T: Read>(value: T) -> i64 => value:read()\nfn main() -> i64 => read(Value { data: 42 })\n";
        check_modules(&[syntax::parse(source).unwrap()]).unwrap();
    }
}
