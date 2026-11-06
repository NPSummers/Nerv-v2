use super::*;
use syntax::{ExprKind as E, StatementKind as S};

type Names = HashMap<String, String>;

fn declarations(module: &Module) -> Vec<String> {
    module
        .functions
        .iter()
        .map(|f| f.name.clone())
        .chain(module.structs.iter().map(|s| s.name.clone()))
        .chain(module.enums.iter().map(|e| e.name.clone()))
        .chain(
            module
                .enums
                .iter()
                .flat_map(|e| e.variants.iter().map(|v| v.name.clone())),
        )
        .chain(module.traits.iter().map(|t| t.name.clone()))
        .collect()
}

pub(super) fn qualify(modules: &[LoadedModule]) -> Result<Vec<LoadedModule>, Errors> {
    let mut declared = HashSet::new();
    for loaded in modules {
        if let Some(name) = &loaded.module.name {
            if !declared.insert(name.join(".")) {
                return Err(error_at(&loaded.path, 1, 1, "duplicate module name").into());
            }
        }
    }
    let mut counts = HashMap::<String, usize>::new();
    for module in modules {
        for name in declarations(&module.module) {
            *counts.entry(name).or_default() += 1;
        }
    }
    let mut maps = Vec::new();
    for (index, module) in modules.iter().enumerate() {
        let names = declarations(&module.module)
            .into_iter()
            .map(|name| {
                let renamed = if (counts[&name] > 1
                    || (name == "main" && index + 1 < modules.len()))
                    && !module.path.starts_with("<stdlib>")
                    && !(name == "main" && index + 1 == modules.len())
                {
                    if name.starts_with("__nerv_test_") || name.starts_with("__nerv_bench_") {
                        format!("{name}_module_{index}")
                    } else {
                        format!("module_{index}__{name}")
                    }
                } else {
                    name.clone()
                };
                (name, renamed)
            })
            .collect::<Names>();
        maps.push(names);
    }
    let mut exports = maps.clone();
    for _ in 0..modules.len() {
        for (index, loaded) in modules.iter().enumerate() {
            let mut candidates = HashMap::<String, Option<String>>::new();
            for target in loaded.resolved_imports.values() {
                if let Some(other) = modules.iter().position(|module| module.path == *target) {
                    for (name, renamed) in &exports[other] {
                        candidates
                            .entry(name.clone())
                            .and_modify(|previous| {
                                if previous.as_ref() != Some(renamed) {
                                    *previous = None;
                                }
                            })
                            .or_insert(Some(renamed.clone()));
                    }
                }
            }
            let mut merged = maps[index].clone();
            for (name, renamed) in candidates {
                if let Some(renamed) = renamed {
                    merged.entry(name).or_insert(renamed);
                }
            }
            exports[index] = merged;
        }
    }
    let mut result = modules.to_vec();
    for (index, loaded) in result.iter_mut().enumerate() {
        let mut names = exports[index].clone();
        for (alias, target) in &loaded.resolved_imports {
            if let Some(other) = modules.iter().position(|module| module.path == *target) {
                for (name, renamed) in &exports[other] {
                    names.insert(format!("{alias}.{name}"), renamed.clone());
                }
            }
        }
        for (other, module) in modules.iter().enumerate() {
            if let Some(alias) = &module.module.name {
                for (name, renamed) in &exports[other] {
                    names.insert(format!("{}.{name}", alias.join(".")), renamed.clone());
                }
            }
        }
        let own = &maps[index];
        for function in &mut loaded.module.functions {
            rename(&mut function.name, own);
            rewrite_function(function, &names);
        }
        for structure in &mut loaded.module.structs {
            let previous = structure.name.clone();
            rename(&mut structure.name, own);
            if let Some(name) = &mut structure.implements {
                rename(name, &names);
            }
            let mut structure_names = names.clone();
            for generic in &mut structure.generics {
                structure_names.remove(&generic.name);
                if let Some(constraint) = &mut generic.constraint {
                    rename(constraint, &names);
                }
            }
            for field in &mut structure.fields {
                rewrite_type(&mut field.ty, &structure_names);
            }
            for method in &mut structure.methods {
                method.name = method.name.replacen(
                    &format!("{previous}__"),
                    &format!("{}__", structure.name),
                    1,
                );
                rewrite_function(method, &structure_names);
            }
        }
        for enumeration in &mut loaded.module.enums {
            rename(&mut enumeration.name, own);
            let mut enum_names = names.clone();
            for generic in &mut enumeration.generics {
                enum_names.remove(&generic.name);
                if let Some(constraint) = &mut generic.constraint {
                    rename(constraint, &names);
                }
            }
            for variant in &mut enumeration.variants {
                rename(&mut variant.name, own);
                for ty in &mut variant.fields {
                    rewrite_type(ty, &enum_names);
                }
            }
        }
        for declaration in &mut loaded.module.traits {
            rename(&mut declaration.name, own);
            for method in &mut declaration.methods {
                rewrite_function(method, &names);
            }
        }
        for implementation in &mut loaded.module.impls {
            if let Some(name) = &mut implementation.trait_name {
                rename(name, &names);
            }
            let previous = implementation.type_name.clone();
            rename(&mut implementation.type_name, &names);
            for method in &mut implementation.methods {
                method.name = method.name.replacen(
                    &format!("{previous}__"),
                    &format!("{}__", implementation.type_name),
                    1,
                );
                rewrite_function(method, &names);
            }
        }
    }
    Ok(result)
}

fn rename(name: &mut String, names: &Names) {
    if let Some(value) = names.get(name) {
        *name = value.clone();
    }
}

fn rewrite_type(ty: &mut Type, names: &Names) {
    match ty {
        Type::Named(name) => rename(name, names),
        Type::Applied { name, args } => {
            rename(name, names);
            for arg in args {
                rewrite_type(arg, names);
            }
        }
        Type::Array(element) => rewrite_type(element, names),
        Type::Function { params, result } => {
            for param in params {
                rewrite_type(param, names);
            }
            rewrite_type(result, names);
        }
        _ => {}
    }
}
fn rewrite_function(function: &mut Function, names: &Names) {
    let mut names = names.clone();
    for generic in &mut function.generics {
        names.remove(&generic.name);
        if let Some(constraint) = &mut generic.constraint {
            rename(constraint, &names);
        }
    }
    rewrite_type(&mut function.result, &names);
    let mut locals = HashSet::new();
    for param in &mut function.params {
        rewrite_type(&mut param.ty, &names);
        locals.insert(param.name.clone());
    }
    rewrite_block(&mut function.body, &names, &mut locals);
}
fn rewrite_block(body: &mut [SpannedStatement], names: &Names, locals: &mut HashSet<String>) {
    for statement in body {
        match &mut statement.kind {
            S::Let {
                name, ty, value, ..
            } => {
                if let Some(ty) = ty {
                    rewrite_type(ty, names);
                }
                rewrite_expr(value, names, locals);
                locals.insert(name.clone());
            }
            S::Assign { value, .. } | S::Return(value) | S::Expr(value) => {
                rewrite_expr(value, names, locals)
            }
            S::AssignIndex { base, index, value } => {
                for expr in [base, index, value] {
                    rewrite_expr(expr, names, locals);
                }
            }
            S::AssignField { base, value, .. } => {
                rewrite_expr(base, names, locals);
                rewrite_expr(value, names, locals);
            }
            S::While { condition, body } => {
                rewrite_expr(condition, names, locals);
                rewrite_block(body, names, &mut locals.clone());
            }
            S::For {
                name,
                start,
                end,
                body,
                ..
            } => {
                rewrite_expr(start, names, locals);
                rewrite_expr(end, names, locals);
                let mut inner = locals.clone();
                inner.insert(name.clone());
                rewrite_block(body, names, &mut inner);
            }
            _ => {}
        }
    }
}
fn rewrite_expr(expr: &mut SpannedExpr, names: &Names, locals: &HashSet<String>) {
    if let Some(name) = callee_name(expr) {
        if !locals.contains(name.split('.').next().unwrap()) {
            if let Some(renamed) = names.get(&name) {
                expr.kind = E::Name(renamed.clone());
                return;
            }
        }
    }
    match &mut expr.kind {
        E::Array(values) => {
            for value in values {
                rewrite_expr(value, names, locals);
            }
        }
        E::ArrayRepeat { value, length } => {
            rewrite_expr(value, names, locals);
            rewrite_expr(length, names, locals);
        }
        E::Struct { name, fields } => {
            rename(name, names);
            for (_, value) in fields {
                rewrite_expr(value, names, locals);
            }
        }
        E::If {
            condition,
            then_body,
            else_body,
        } => {
            rewrite_expr(condition, names, locals);
            rewrite_block(then_body, names, &mut locals.clone());
            rewrite_block(else_body, names, &mut locals.clone());
        }
        E::Match { value, arms } => {
            rewrite_expr(value, names, locals);
            for arm in arms {
                rename(&mut arm.variant, names);
                let mut inner = locals.clone();
                if let Some(name) = &arm.binding {
                    inner.insert(name.clone());
                }
                rewrite_expr(&mut arm.body, names, &inner);
            }
        }
        E::Lambda {
            params,
            result,
            body,
        } => {
            let mut inner = locals.clone();
            for param in params {
                rewrite_type(&mut param.ty, names);
                inner.insert(param.name.clone());
            }
            rewrite_type(result, names);
            rewrite_expr(body, names, &inner);
        }
        E::Try(value) | E::Unary { value, .. } | E::Field { base: value, .. } => {
            rewrite_expr(value, names, locals)
        }
        E::Call { callee, args } => {
            rewrite_expr(callee, names, locals);
            for arg in args {
                rewrite_expr(arg, names, locals);
            }
        }
        E::Method { base, args, .. } => {
            rewrite_expr(base, names, locals);
            for arg in args {
                rewrite_expr(arg, names, locals);
            }
        }
        E::Index { base, index } => {
            rewrite_expr(base, names, locals);
            rewrite_expr(index, names, locals);
        }
        E::Binary { left, right, .. } => {
            rewrite_expr(left, names, locals);
            rewrite_expr(right, names, locals);
        }
        _ => {}
    }
}
