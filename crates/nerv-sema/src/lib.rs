use std::{
    collections::{HashMap, HashSet},
    fmt, fs,
    path::{Path, PathBuf},
};

use nerv_syntax::{
    self as syntax, BinaryOp, Expr, Function, Module, Statement, Struct as StructDecl, Type,
    UnaryOp,
};

#[derive(Debug)]
pub struct Program {
    pub modules: Vec<LoadedModule>,
}

#[derive(Debug)]
pub struct LoadedModule {
    pub path: PathBuf,
    pub module: Module,
}

#[derive(Clone)]
struct Binding {
    ty: Type,
    mutable: bool,
}

#[derive(Clone)]
struct VariantInfo {
    enumeration: String,
    payload: Option<Type>,
    variants: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Error {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}: {}",
            self.path.display(),
            self.line,
            self.column,
            self.message
        )
    }
}

impl std::error::Error for Error {}

pub fn check_path(path: impl AsRef<Path>) -> Result<Program, Error> {
    let mut modules = Vec::new();
    let mut visited = HashSet::new();
    let mut loading = HashSet::new();
    load(path.as_ref(), &mut modules, &mut visited, &mut loading)?;
    check(&modules)?;
    Ok(Program { modules })
}

fn load(
    path: &Path,
    modules: &mut Vec<LoadedModule>,
    visited: &mut HashSet<PathBuf>,
    loading: &mut HashSet<PathBuf>,
) -> Result<(), Error> {
    let path = path
        .canonicalize()
        .map_err(|error| error_at(path, 0, 0, error.to_string()))?;
    if visited.contains(&path) {
        return Ok(());
    }
    if !loading.insert(path.clone()) {
        return Err(error_at(&path, 0, 0, "import cycle"));
    }
    let source =
        fs::read_to_string(&path).map_err(|error| error_at(&path, 0, 0, error.to_string()))?;
    let module = syntax::parse(&source).map_err(|error| frontend_error(&path, error))?;
    for import in &module.imports {
        if import.path.first().is_some_and(|part| part == "nerv") {
            continue;
        }
        let mut target = path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        for part in &import.path {
            target.push(part);
        }
        target.set_extension("nerv");
        load(&target, modules, visited, loading)?;
    }
    loading.remove(&path);
    visited.insert(path.clone());
    modules.push(LoadedModule { path, module });
    Ok(())
}

fn check(modules: &[LoadedModule]) -> Result<(), Error> {
    let mut functions = HashMap::new();
    let mut structs = HashMap::new();
    let mut variants = HashMap::new();
    let mut traits = HashSet::new();
    let mut implementations = HashSet::new();
    for loaded in modules {
        for structure in &loaded.module.structs {
            if structs.insert(structure.name.as_str(), structure).is_some() {
                return Err(error_at(
                    &loaded.path,
                    structure.line,
                    structure.column,
                    "duplicate struct",
                ));
            }
        }
        for enumeration in &loaded.module.enums {
            let names: Vec<String> = enumeration
                .variants
                .iter()
                .map(|variant| variant.name.clone())
                .collect();
            for variant in &enumeration.variants {
                if variants
                    .insert(
                        variant.name.as_str(),
                        VariantInfo {
                            enumeration: enumeration.name.clone(),
                            payload: variant.fields.first().cloned(),
                            variants: names.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(error_at(
                        &loaded.path,
                        variant.line,
                        variant.column,
                        "duplicate enum variant",
                    ));
                }
            }
        }
        for trait_decl in &loaded.module.traits {
            if !traits.insert(trait_decl.name.as_str()) {
                return Err(error_at(
                    &loaded.path,
                    trait_decl.line,
                    trait_decl.column,
                    "duplicate trait",
                ));
            }
        }
        for structure in &loaded.module.structs {
            if let Some(trait_name) = &structure.implements {
                implementations.insert((structure.name.as_str(), trait_name.as_str()));
            }
        }
        for implementation in &loaded.module.impls {
            if let Some(trait_name) = &implementation.trait_name {
                if !implementations.insert((implementation.type_name.as_str(), trait_name.as_str()))
                {
                    return Err(error_at(
                        &loaded.path,
                        implementation.line,
                        implementation.column,
                        "duplicate trait implementation",
                    ));
                }
            }
        }
        for function in all_functions(&loaded.module) {
            if functions.insert(function.name.as_str(), function).is_some() {
                return Err(error_at(
                    &loaded.path,
                    function.line,
                    function.column,
                    "duplicate function",
                ));
            }
        }
    }
    for loaded in modules {
        for implementation in &loaded.module.impls {
            if let Some(trait_name) = &implementation.trait_name {
                if !traits.contains(trait_name.as_str()) {
                    return Err(error_at(
                        &loaded.path,
                        implementation.line,
                        implementation.column,
                        "unknown trait",
                    ));
                }
            }
        }
    }
    for loaded in modules {
        for function in all_functions(&loaded.module) {
            check_function(
                &loaded.path,
                function,
                &functions,
                &structs,
                &variants,
                &implementations,
            )?;
        }
    }
    Ok(())
}

fn all_functions(module: &Module) -> impl Iterator<Item = &Function> {
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
}

fn check_function(
    path: &Path,
    function: &Function,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
    implementations: &HashSet<(&str, &str)>,
) -> Result<(), Error> {
    let mut values = HashMap::new();
    for parameter in &function.params {
        if values
            .insert(
                parameter.name.clone(),
                Binding {
                    ty: parameter.ty.clone(),
                    mutable: false,
                },
            )
            .is_some()
        {
            return Err(error_at(
                path,
                parameter.line,
                parameter.column,
                "duplicate parameter",
            ));
        }
    }
    let mut last = Type::Void;
    let mut returns = false;
    for statement in &function.body {
        last = match statement {
            Statement::Let {
                name,
                mutable,
                ty,
                value,
                line,
                column,
                ..
            } => {
                let actual = expr_type(path, value, &values, functions, structs, variants)?;
                if let Some(expected) = ty
                    && *expected != actual
                    && !implements(&actual, expected, implementations)
                {
                    return Err(error_at(
                        path,
                        *line,
                        *column,
                        format!("expected {expected:?}, found {actual:?}"),
                    ));
                }
                values.insert(
                    name.clone(),
                    Binding {
                        ty: binding_type(ty, actual),
                        mutable: *mutable,
                    },
                );
                Type::Void
            }
            Statement::Assign { name, value } => {
                let expected = values
                    .get(name)
                    .ok_or_else(|| error_at(path, 0, 0, format!("unknown name `{name}`")))?;
                if !expected.mutable {
                    return Err(error_at(path, 0, 0, format!("cannot assign to `{name}`")));
                }
                let actual = expr_type(path, value, &values, functions, structs, variants)?;
                if actual != expected.ty {
                    return Err(error_at(path, 0, 0, "assignment type mismatch"));
                }
                Type::Void
            }
            Statement::AssignIndex { base, index, value } => {
                require_mutable(path, base, &values)?;
                let Type::Array(element) =
                    expr_type(path, base, &values, functions, structs, variants)?
                else {
                    return Err(error_at(path, 0, 0, "index assignment requires an array"));
                };
                if expr_type(path, index, &values, functions, structs, variants)? != Type::Int
                    || expr_type(path, value, &values, functions, structs, variants)? != *element
                {
                    return Err(error_at(path, 0, 0, "invalid array assignment"));
                }
                Type::Void
            }
            Statement::AssignField { base, name, value } => {
                require_mutable(path, base, &values)?;
                check_field_assignment(
                    path, base, name, value, &values, functions, structs, variants,
                )?;
                Type::Void
            }
            Statement::While { condition, body } => {
                if expr_type(path, condition, &values, functions, structs, variants)? != Type::Bool
                {
                    return Err(error_at(path, 0, 0, "while condition must be bool"));
                }
                block_type(
                    path,
                    body,
                    &mut values.clone(),
                    functions,
                    structs,
                    variants,
                )?;
                Type::Void
            }
            Statement::For {
                name,
                start,
                end,
                body,
                ..
            } => {
                if expr_type(path, start, &values, functions, structs, variants)? != Type::Int
                    || expr_type(path, end, &values, functions, structs, variants)? != Type::Int
                {
                    return Err(error_at(path, 0, 0, "range bounds must be integers"));
                }
                let mut loop_values = values.clone();
                loop_values.insert(
                    name.clone(),
                    Binding {
                        ty: Type::Int,
                        mutable: false,
                    },
                );
                block_type(path, body, &mut loop_values, functions, structs, variants)?;
                Type::Void
            }
            Statement::Return(value) => {
                let actual = expr_type(path, value, &values, functions, structs, variants)?;
                if actual != function.result {
                    return Err(error_at(
                        path,
                        function.line,
                        function.column,
                        format!("expected {:?}, found {actual:?}", function.result),
                    ));
                }
                returns = true;
                Type::Void
            }
            Statement::Expr(value) => {
                expr_type(path, value, &values, functions, structs, variants)?
            }
        };
    }
    if function.result != Type::Void && !returns && last != function.result {
        return Err(error_at(
            path,
            function.line,
            function.column,
            format!("expected {:?}, found {last:?}", function.result),
        ));
    }
    Ok(())
}

fn expr_type(
    path: &Path,
    expr: &Expr,
    values: &HashMap<String, Binding>,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
) -> Result<Type, Error> {
    match expr {
        Expr::Int(_) => Ok(Type::Int),
        Expr::Float(_) => Ok(Type::Float),
        Expr::String(_) => Ok(Type::String),
        Expr::Bool(_) => Ok(Type::Bool),
        Expr::Array(elements) => {
            for value in elements {
                if expr_type(path, value, values, functions, structs, variants)? != Type::Int {
                    return Err(error_at(path, 0, 0, "arrays currently require i64 values"));
                }
            }
            Ok(Type::Array(Box::new(Type::Int)))
        }
        Expr::ArrayRepeat { value, length } => {
            if expr_type(path, value, values, functions, structs, variants)? != Type::Int
                || expr_type(path, length, values, functions, structs, variants)? != Type::Int
            {
                return Err(error_at(
                    path,
                    0,
                    0,
                    "array values and lengths must be integers",
                ));
            }
            Ok(Type::Array(Box::new(Type::Int)))
        }
        Expr::Name(name) => values
            .get(name.as_str())
            .map(|binding| binding.ty.clone())
            .or_else(|| {
                functions.get(name.as_str()).map(|function| Type::Function {
                    params: function
                        .params
                        .iter()
                        .map(|param| param.ty.clone())
                        .collect(),
                    result: Box::new(function.result.clone()),
                })
            })
            .ok_or_else(|| error_at(path, 0, 0, format!("unknown name `{name}`"))),
        Expr::Lambda {
            params,
            result,
            body,
        } => {
            let lambda_values = params
                .iter()
                .map(|param| {
                    (
                        param.name.clone(),
                        Binding {
                            ty: param.ty.clone(),
                            mutable: false,
                        },
                    )
                })
                .collect();
            let actual = expr_type(path, body, &lambda_values, functions, structs, variants)?;
            if actual != *result {
                return Err(error_at(path, 0, 0, "lambda result type mismatch"));
            }
            Ok(Type::Function {
                params: params.iter().map(|param| param.ty.clone()).collect(),
                result: Box::new(result.clone()),
            })
        }
        Expr::Method { base, name, args } => {
            let Type::Named(type_name) =
                expr_type(path, base, values, functions, structs, variants)?
            else {
                return Err(error_at(
                    path,
                    0,
                    0,
                    "method call requires a concrete value",
                ));
            };
            let method = format!("{type_name}__{name}");
            let function = functions
                .get(method.as_str())
                .ok_or_else(|| error_at(path, 0, 0, format!("unknown method `{name}`")))?;
            if function.params.len() != args.len() + 1 {
                return Err(error_at(path, 0, 0, "wrong argument count"));
            }
            if function.params[0].ty != Type::Named(type_name.clone()) {
                return Err(error_at(path, 0, 0, "invalid method receiver"));
            }
            for (arg, parameter) in args.iter().zip(function.params.iter().skip(1)) {
                if expr_type(path, arg, values, functions, structs, variants)? != parameter.ty {
                    return Err(error_at(
                        path,
                        parameter.line,
                        parameter.column,
                        "argument type mismatch",
                    ));
                }
            }
            Ok(function.result.clone())
        }
        Expr::Match { value, arms } => {
            let Type::Named(enumeration) =
                expr_type(path, value, values, functions, structs, variants)?
            else {
                return Err(error_at(path, 0, 0, "match requires an enum value"));
            };
            let mut seen = HashSet::new();
            let mut result = None;
            for arm in arms {
                let info = variants.get(arm.variant.as_str()).ok_or_else(|| {
                    error_at(path, 0, 0, format!("unknown variant `{}`", arm.variant))
                })?;
                if info.enumeration != enumeration {
                    return Err(error_at(
                        path,
                        0,
                        0,
                        "match variant belongs to another enum",
                    ));
                }
                if !seen.insert(arm.variant.as_str()) {
                    return Err(error_at(path, 0, 0, "duplicate match variant"));
                }
                let mut arm_values = values.clone();
                match (&info.payload, &arm.binding) {
                    (Some(ty), Some(name)) => {
                        arm_values.insert(
                            name.clone(),
                            Binding {
                                ty: ty.clone(),
                                mutable: false,
                            },
                        );
                    }
                    (Some(_), None) => {
                        return Err(error_at(path, 0, 0, "match payload needs a binding"));
                    }
                    (None, Some(_)) => {
                        return Err(error_at(path, 0, 0, "match variant has no payload"));
                    }
                    (None, None) => {}
                }
                let ty = expr_type(path, &arm.body, &arm_values, functions, structs, variants)?;
                if let Some(expected) = &result {
                    if *expected != ty {
                        return Err(error_at(path, 0, 0, "match arms have different types"));
                    }
                } else {
                    result = Some(ty);
                }
            }
            let expected = variants
                .values()
                .find(|info| info.enumeration == enumeration)
                .map(|info| &info.variants)
                .unwrap();
            if seen.len() != expected.len()
                || expected.iter().any(|name| !seen.contains(name.as_str()))
            {
                return Err(error_at(path, 0, 0, "non-exhaustive match"));
            }
            result.ok_or_else(|| error_at(path, 0, 0, "match needs at least one arm"))
        }
        Expr::If {
            condition,
            then_body,
            else_body,
        } => {
            if expr_type(path, condition, values, functions, structs, variants)? != Type::Bool {
                return Err(error_at(path, 0, 0, "if condition must be bool"));
            }
            let then_type = block_type(
                path,
                then_body,
                &mut values.clone(),
                functions,
                structs,
                variants,
            )?;
            let else_type = block_type(
                path,
                else_body,
                &mut values.clone(),
                functions,
                structs,
                variants,
            )?;
            if then_type == else_type {
                Ok(then_type)
            } else {
                Err(error_at(path, 0, 0, "if branches have different types"))
            }
        }
        Expr::Unary { op, value } => {
            let ty = expr_type(path, value, values, functions, structs, variants)?;
            match (op, &ty) {
                (UnaryOp::Neg, Type::Int | Type::Float) | (UnaryOp::Not, Type::Bool) => Ok(ty),
                _ => Err(error_at(path, 0, 0, "invalid unary operation")),
            }
        }
        Expr::Binary { left, op, right } => {
            let left = expr_type(path, left, values, functions, structs, variants)?;
            let right = expr_type(path, right, values, functions, structs, variants)?;
            match op {
                BinaryOp::And | BinaryOp::Or if left == Type::Bool && right == Type::Bool => {
                    Ok(Type::Bool)
                }
                BinaryOp::Eq | BinaryOp::NotEq if left == right => Ok(Type::Bool),
                BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
                    if numeric(&left) && left == right =>
                {
                    Ok(Type::Bool)
                }
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem
                    if numeric(&left) && left == right =>
                {
                    Ok(left)
                }
                _ => Err(error_at(path, 0, 0, "invalid binary operation")),
            }
        }
        Expr::Call { callee, args } => {
            let Some(name) = callee_name(callee) else {
                return Err(error_at(path, 0, 0, "unsupported call target"));
            };
            let args = args
                .iter()
                .map(|arg| expr_type(path, arg, values, functions, structs, variants))
                .collect::<Result<Vec<_>, _>>()?;
            if let Some(result) = runtime_type(&name, &args) {
                return result.ok_or_else(|| error_at(path, 0, 0, "invalid runtime call"));
            }
            let variant = name.rsplit('.').next().unwrap();
            if let Some(info) = variants.get(variant) {
                let valid = match (&info.payload, args.as_slice()) {
                    (None, []) => true,
                    (Some(payload), [arg]) => arg == payload,
                    _ => false,
                };
                if !valid {
                    return Err(error_at(path, 0, 0, "invalid enum variant arguments"));
                }
                return Ok(Type::Named(info.enumeration.clone()));
            }
            if let Some(Binding {
                ty: Type::Function { params, result },
                ..
            }) = values.get(name.as_str())
            {
                if args != *params {
                    return Err(error_at(path, 0, 0, "function argument type mismatch"));
                }
                return Ok((**result).clone());
            }
            let function = functions
                .get(name.as_str())
                .ok_or_else(|| error_at(path, 0, 0, format!("unknown function `{name}`")))?;
            if args.len() != function.params.len() {
                return Err(error_at(
                    path,
                    function.line,
                    function.column,
                    "wrong argument count",
                ));
            }
            for (arg, parameter) in args.iter().zip(&function.params) {
                if *arg != parameter.ty {
                    return Err(error_at(
                        path,
                        parameter.line,
                        parameter.column,
                        "argument type mismatch",
                    ));
                }
            }
            Ok(function.result.clone())
        }
        Expr::Struct { name, fields } => {
            let structure = structs
                .get(name.as_str())
                .ok_or_else(|| error_at(path, 0, 0, format!("unknown struct `{name}`")))?;
            if fields.len() != structure.fields.len() {
                return Err(error_at(path, 0, 0, "wrong struct field count"));
            }
            for (name, value) in fields {
                let field = structure
                    .fields
                    .iter()
                    .find(|field| field.name == *name)
                    .ok_or_else(|| error_at(path, 0, 0, format!("unknown field `{name}`")))?;
                if expr_type(path, value, values, functions, structs, variants)? != field.ty {
                    return Err(error_at(path, 0, 0, "struct field type mismatch"));
                }
            }
            Ok(Type::Named(name.clone()))
        }
        Expr::Field { base, name } => {
            let Type::Named(struct_name) =
                expr_type(path, base, values, functions, structs, variants)?
            else {
                return Err(error_at(path, 0, 0, "field access requires a struct"));
            };
            structs
                .get(struct_name.as_str())
                .and_then(|structure| structure.fields.iter().find(|field| field.name == *name))
                .map(|field| field.ty.clone())
                .ok_or_else(|| error_at(path, 0, 0, format!("unknown field `{name}`")))
        }
        Expr::Index { base, index } => {
            let Type::Array(element) = expr_type(path, base, values, functions, structs, variants)?
            else {
                return Err(error_at(path, 0, 0, "indexing requires an array"));
            };
            if expr_type(path, index, values, functions, structs, variants)? != Type::Int {
                return Err(error_at(path, 0, 0, "array index must be an integer"));
            }
            Ok(*element)
        }
    }
}

fn check_field_assignment(
    path: &Path,
    base: &Expr,
    name: &str,
    value: &Expr,
    values: &HashMap<String, Binding>,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
) -> Result<(), Error> {
    let Type::Named(struct_name) = expr_type(path, base, values, functions, structs, variants)?
    else {
        return Err(error_at(path, 0, 0, "field assignment requires a struct"));
    };
    let field = structs
        .get(struct_name.as_str())
        .and_then(|structure| structure.fields.iter().find(|field| field.name == name))
        .ok_or_else(|| error_at(path, 0, 0, format!("unknown field `{name}`")))?;
    let actual = expr_type(path, value, values, functions, structs, variants)?;
    if actual != field.ty {
        return Err(error_at(path, 0, 0, "field assignment type mismatch"));
    }
    Ok(())
}

fn require_mutable(
    path: &Path,
    base: &Expr,
    values: &HashMap<String, Binding>,
) -> Result<(), Error> {
    let Expr::Name(name) = base else {
        return Err(error_at(path, 0, 0, "assignment target must be a binding"));
    };
    let binding = values
        .get(name)
        .ok_or_else(|| error_at(path, 0, 0, format!("unknown name `{name}`")))?;
    if binding.mutable {
        Ok(())
    } else {
        Err(error_at(path, 0, 0, format!("cannot assign to `{name}`")))
    }
}

fn callee_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Name(name) => Some(name.clone()),
        Expr::Field { base, name } => Some(format!("{}.{}", callee_name(base)?, name)),
        _ => None,
    }
}

fn runtime_type(name: &str, args: &[Type]) -> Option<Option<Type>> {
    Some(match (name, args) {
        ("print" | "println", [_]) => Some(Type::Void),
        ("nerv.math.sqrt", [Type::Float]) => Some(Type::Float),
        ("nerv.math.pow", [Type::Float, Type::Float]) => Some(Type::Float),
        ("nerv.math.abs", [Type::Int]) => Some(Type::Int),
        ("nerv.math.abs", [Type::Float]) => Some(Type::Float),
        ("nerv.string.len", [Type::String]) => Some(Type::Int),
        ("nerv.string.concat", [Type::String, Type::String]) => Some(Type::String),
        ("nerv.string.is_empty" | "nerv.string.char_count", [Type::String]) => Some(Type::Int),
        ("nerv.string.to_upper" | "nerv.string.to_lower" | "nerv.string.trim", [Type::String]) => {
            Some(Type::String)
        }
        (
            "nerv.string.contains"
            | "nerv.string.starts_with"
            | "nerv.string.ends_with"
            | "nerv.string.find",
            [Type::String, Type::String],
        ) => Some(Type::Int),
        ("nerv.string.split" | "nerv.string.split_once", [Type::String, Type::String]) => {
            Some(Type::String)
        }
        ("nerv.string.char_at", [Type::String, Type::Int]) => Some(Type::Int),
        ("nerv.string.repeat", [Type::String, Type::Int]) => Some(Type::String),
        ("nerv.string.replace", [Type::String, Type::String, Type::String]) => Some(Type::String),
        ("nerv.string.slice", [Type::String, Type::Int, Type::Int]) => Some(Type::String),
        ("nerv.arrays.new", [Type::Int, Type::Int]) => Some(Type::Array(Box::new(Type::Int))),
        (
            "nerv.arrays.map",
            [
                Type::Array(element),
                Type::Int,
                Type::Function { params, result },
            ],
        ) if **element == Type::Int && *params == [Type::Int] && **result == Type::Int => {
            Some(Type::Array(Box::new(Type::Int)))
        }
        (
            "nerv.arrays.reduce",
            [
                Type::Array(element),
                Type::Int,
                Type::Int,
                Type::Function { params, result },
            ],
        ) if **element == Type::Int
            && *params == [Type::Int, Type::Int]
            && **result == Type::Int =>
        {
            Some(Type::Int)
        }
        ("nerv.random", [Type::Int]) => Some(Type::Int),
        (
            "nerv.collections.vec_new"
            | "nerv.collections.map_new"
            | "nerv.collections.set_new"
            | "nerv.collections.deque_new",
            [],
        ) => Some(Type::Int),
        ("nerv.collections.vec_push", [Type::Int, Type::Int]) => Some(Type::Void),
        (
            "nerv.collections.vec_set" | "nerv.collections.vec_insert",
            [Type::Int, Type::Int, Type::Int],
        ) => Some(Type::Void),
        (
            "nerv.collections.vec_clear"
            | "nerv.collections.vec_free"
            | "nerv.collections.deque_free",
            [Type::Int],
        ) => Some(Type::Void),
        (
            "nerv.collections.vec_pop"
            | "nerv.collections.vec_len"
            | "nerv.collections.vec_is_empty"
            | "nerv.collections.vec_as_ptr"
            | "nerv.collections.deque_pop_back"
            | "nerv.collections.deque_pop_front"
            | "nerv.collections.deque_len",
            [Type::Int],
        ) => Some(Type::Int),
        (
            "nerv.collections.vec_get"
            | "nerv.collections.vec_remove"
            | "nerv.collections.vec_contains"
            | "nerv.collections.vec_index_of",
            [Type::Int, Type::Int],
        ) => Some(Type::Int),
        ("nerv.collections.map_insert", [Type::Int, Type::String, Type::Int]) => Some(Type::Int),
        (
            "nerv.collections.map_get"
            | "nerv.collections.map_remove"
            | "nerv.collections.map_contains_key",
            [Type::Int, Type::String],
        ) => Some(Type::Int),
        ("nerv.collections.map_len", [Type::Int]) => Some(Type::Int),
        ("nerv.collections.map_free", [Type::Int]) => Some(Type::Void),
        (
            "nerv.collections.set_insert"
            | "nerv.collections.set_contains"
            | "nerv.collections.set_remove",
            [Type::Int, Type::Int],
        ) => Some(Type::Int),
        ("nerv.collections.set_len", [Type::Int]) => Some(Type::Int),
        ("nerv.collections.set_free", [Type::Int]) => Some(Type::Void),
        (
            "nerv.collections.deque_push_back" | "nerv.collections.deque_push_front",
            [Type::Int, Type::Int],
        ) => Some(Type::Void),
        (
            "print"
            | "println"
            | "nerv.math.sqrt"
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
        ) => None,
        _ => return None,
    })
}

fn block_type(
    path: &Path,
    body: &[Statement],
    values: &mut HashMap<String, Binding>,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
) -> Result<Type, Error> {
    let mut last = Type::Void;
    for statement in body {
        last = match statement {
            Statement::Let {
                name,
                mutable,
                ty,
                value,
                ..
            } => {
                let actual = expr_type(path, value, values, functions, structs, variants)?;
                if let Some(expected) = ty
                    && *expected != actual
                {
                    return Err(error_at(path, 0, 0, "let binding type mismatch"));
                }
                values.insert(
                    name.clone(),
                    Binding {
                        ty: binding_type(ty, actual),
                        mutable: *mutable,
                    },
                );
                Type::Void
            }
            Statement::Assign { name, value } => {
                let expected = values
                    .get(name)
                    .ok_or_else(|| error_at(path, 0, 0, format!("unknown name `{name}`")))?;
                if !expected.mutable {
                    return Err(error_at(path, 0, 0, format!("cannot assign to `{name}`")));
                }
                let actual = expr_type(path, value, values, functions, structs, variants)?;
                if actual != expected.ty {
                    return Err(error_at(path, 0, 0, "assignment type mismatch"));
                }
                Type::Void
            }
            Statement::AssignIndex { base, index, value } => {
                require_mutable(path, base, values)?;
                let Type::Array(element) =
                    expr_type(path, base, values, functions, structs, variants)?
                else {
                    return Err(error_at(path, 0, 0, "index assignment requires an array"));
                };
                if expr_type(path, index, values, functions, structs, variants)? != Type::Int
                    || expr_type(path, value, values, functions, structs, variants)? != *element
                {
                    return Err(error_at(path, 0, 0, "invalid array assignment"));
                }
                Type::Void
            }
            Statement::AssignField { base, name, value } => {
                require_mutable(path, base, values)?;
                check_field_assignment(
                    path, base, name, value, values, functions, structs, variants,
                )?;
                Type::Void
            }
            Statement::While { condition, body } => {
                if expr_type(path, condition, values, functions, structs, variants)? != Type::Bool {
                    return Err(error_at(path, 0, 0, "while condition must be bool"));
                }
                block_type(
                    path,
                    body,
                    &mut values.clone(),
                    functions,
                    structs,
                    variants,
                )?;
                Type::Void
            }
            Statement::For {
                name,
                start,
                end,
                body,
                ..
            } => {
                if expr_type(path, start, values, functions, structs, variants)? != Type::Int
                    || expr_type(path, end, values, functions, structs, variants)? != Type::Int
                {
                    return Err(error_at(path, 0, 0, "range bounds must be integers"));
                }
                let mut loop_values = values.clone();
                loop_values.insert(
                    name.clone(),
                    Binding {
                        ty: Type::Int,
                        mutable: false,
                    },
                );
                block_type(path, body, &mut loop_values, functions, structs, variants)?;
                Type::Void
            }
            Statement::Return(value) | Statement::Expr(value) => {
                expr_type(path, value, values, functions, structs, variants)?
            }
        };
    }
    Ok(last)
}

fn numeric(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Float)
}

fn implements(actual: &Type, expected: &Type, implementations: &HashSet<(&str, &str)>) -> bool {
    let (Type::Named(actual), Type::Named(expected)) = (actual, expected) else {
        return false;
    };
    implementations.contains(&(actual, expected))
}

fn binding_type(annotation: &Option<Type>, actual: Type) -> Type {
    annotation
        .as_ref()
        .filter(|annotation| **annotation == actual)
        .cloned()
        .unwrap_or(actual)
}
fn error_at(path: &Path, line: usize, column: usize, message: impl Into<String>) -> Error {
    Error {
        path: path.to_path_buf(),
        line,
        column,
        message: message.into(),
    }
}
fn frontend_error(path: &Path, error: syntax::FrontendError) -> Error {
    match error {
        syntax::FrontendError::Lex(error) => {
            error_at(path, error.line, error.column, error.to_string())
        }
        syntax::FrontendError::Parse(error) => {
            error_at(path, error.line, error.column, error.message)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn checks_imported_functions() {
        let root = std::env::temp_dir().join(format!(
            "nerv-sema-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("math.nerv"),
            "fn add(a: i64, b: i64) -> i64 => a + b\n",
        )
        .unwrap();
        fs::write(
            root.join("main.nerv"),
            "import math\nfn main() -> i64 => add(1, 2)\n",
        )
        .unwrap();
        assert!(check_path(root.join("main.nerv")).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_assigning_immutable_bindings() {
        let root = std::env::temp_dir().join(format!(
            "nerv-sema-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("main.nerv"),
            "fn main() -> void\n    let value = 1\n    value = 2\n",
        )
        .unwrap();
        assert!(check_path(root.join("main.nerv")).is_err());
        fs::write(
            root.join("main.nerv"),
            "fn main() -> void\n    let mut value = 1\n    value = 2\n",
        )
        .unwrap();
        assert!(check_path(root.join("main.nerv")).is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checks_exhaustive_enum_matches() {
        let root = std::env::temp_dir().join(format!(
            "nerv-sema-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("main.nerv"),
            "enum Choice\n    Some(i64)\n    None\n\nfn main() -> i64\n    match Some(4)\n        Some(value) => value\n        None => 0\n",
        )
        .unwrap();
        assert!(check_path(root.join("main.nerv")).is_ok());
        fs::write(
            root.join("main.nerv"),
            "enum Choice\n    Some(i64)\n    None\n\nfn main() -> i64\n    match Some(4)\n        Some(value) => value\n",
        )
        .unwrap();
        assert!(check_path(root.join("main.nerv")).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checks_trait_method_dispatch() {
        let root = std::env::temp_dir().join(format!(
            "nerv-sema-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("main.nerv"),
            "trait Drawable\n    fn draw(self) -> i64\n\nenum Light\n    On\n\nimpl Light for Drawable\n    fn draw(self) -> i64 => 42\n\nfn main() -> i64\n    let light = On()\n    let drawable: Drawable = light\n    drawable:draw()\n",
        )
        .unwrap();
        assert!(check_path(root.join("main.nerv")).is_ok());
        fs::remove_dir_all(root).unwrap();
    }
}
