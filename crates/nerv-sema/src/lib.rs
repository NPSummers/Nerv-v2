use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt, fs,
    path::{Path, PathBuf},
};

use nerv_syntax::{
    self as syntax, BinaryOp, Expr as SpannedExpr, ExprKind as Expr, Function, Module, SourceFile,
    SourceId, SourceMap, Span, Statement as SpannedStatement, StatementKind as Statement,
    Struct as StructDecl, Type, UnaryOp,
};

mod generics;
pub mod ir;
mod resolve;

#[derive(Debug)]
pub struct Program {
    pub ir: ir::CheckedProgram,
    pub modules: Vec<LoadedModule>,
    pub sources: SourceMap,
}

#[derive(Debug, Clone)]
pub struct LoadedModule {
    pub source: SourceId,
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
    pub span: Option<Box<Span>>,
    pub source: Option<Box<SourceFile>>,
    pub notes: Vec<String>,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let (Some(source), Some(span)) = (&self.source, self.span.as_deref()) {
            return f.write_str(&source.render(*span, &self.message, &self.notes));
        }
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

impl Error {
    fn at(mut self, span: Span) -> Self {
        if self.span.is_none() {
            self.span = Some(Box::new(span));
        }
        self
    }

    fn note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }
}

#[derive(Debug)]
pub struct Errors(pub Vec<Error>);

impl From<Error> for Errors {
    fn from(error: Error) -> Self {
        Self(vec![error])
    }
}

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, error) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f, "\n")?;
            }
            write!(f, "{error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Errors {}

impl Errors {
    fn with_sources(mut self, sources: &SourceMap) -> Self {
        for error in &mut self.0 {
            let source = error
                .span
                .as_deref()
                .map(|span| sources.get(span.source))
                .or_else(|| sources.find(&error.path));
            if let Some(source) = source {
                let span = **error.span.get_or_insert_with(|| {
                    let start = source.offset(error.line.max(1), error.column.max(1));
                    let length = source.text[start..]
                        .chars()
                        .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
                        .map(char::len_utf8)
                        .sum::<usize>();
                    Box::new(Span {
                        source: source.id,
                        start,
                        end: start + length,
                    })
                });
                (error.line, error.column) = source.location(span.start);
                error.path = source.path.clone();
                error.source = Some(Box::new(source.clone()));
            }
        }
        self
    }
}

pub fn check_path(path: impl AsRef<Path>) -> Result<Program, Errors> {
    check_path_with_imports(path, &ImportPaths::new())
}

pub type ImportPaths = BTreeMap<PathBuf, BTreeMap<String, PathBuf>>;

pub fn check_path_with_imports(
    path: impl AsRef<Path>,
    imports: &ImportPaths,
) -> Result<Program, Errors> {
    let mut modules = Vec::new();
    let mut visited = HashSet::new();
    let mut loading = HashSet::new();
    let mut sources = SourceMap::default();
    load(
        path.as_ref(),
        &mut modules,
        &mut visited,
        &mut loading,
        &mut sources,
        imports,
    )
    .map_err(|error| Errors::from(error).with_sources(&sources))?;
    let ir = check_loaded(&modules).map_err(|errors| errors.with_sources(&sources))?;
    Ok(Program {
        ir,
        modules,
        sources,
    })
}

fn check_loaded(modules: &[LoadedModule]) -> Result<ir::CheckedProgram, Errors> {
    let modules = generics::expand(modules)?;
    check(&modules)?;
    resolve::resolve(&modules).map_err(Into::into)
}

/// Check already-loaded syntax modules for embedding and compiler unit tests.
/// Callers assign source IDs with `parse_source` when using multiple buffers.
pub fn check_modules(modules: &[Module]) -> Result<ir::CheckedProgram, Errors> {
    let loaded = modules
        .iter()
        .cloned()
        .map(|module| LoadedModule {
            source: module.source,
            path: PathBuf::from("<buffer>"),
            module,
        })
        .collect::<Vec<_>>();
    check_loaded(&loaded)
}

fn load(
    path: &Path,
    modules: &mut Vec<LoadedModule>,
    visited: &mut HashSet<PathBuf>,
    loading: &mut HashSet<PathBuf>,
    sources: &mut SourceMap,
    imports: &ImportPaths,
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
    let id = sources.add(path.clone(), source.clone());
    let module =
        syntax::parse_source(&source, id).map_err(|error| frontend_error(&path, id, error))?;
    for import in &module.imports {
        if import.path.first().is_some_and(|part| part == "nerv") {
            if import.path.len() == 2 && matches!(import.path[1].as_str(), "core" | "std") {
                load_standard(&import.path[1], modules, visited, sources)?;
            }
            continue;
        }
        let package = imports
            .iter()
            .filter(|(root, _)| path.starts_with(root))
            .max_by_key(|(root, _)| root.components().count());
        let dependency = package.and_then(|(_, aliases)| aliases.get(&import.path[0]));
        let mut target = if let Some(entry) = dependency {
            if import.path.len() == 1 {
                entry.clone()
            } else {
                let mut target = entry.parent().unwrap().to_path_buf();
                for part in &import.path[1..] {
                    target.push(part);
                }
                target.set_extension("nerv");
                target
            }
        } else {
            let mut target = path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            for part in &import.path {
                target.push(part);
            }
            target.set_extension("nerv");
            target
        };
        if !target.exists() && dependency.is_none() {
            if let Some((root, _)) = package {
                target = root.join("src");
                for part in &import.path {
                    target.push(part);
                }
                target.set_extension("nerv");
            }
        }
        load(&target, modules, visited, loading, sources, imports).map_err(|mut error| {
            if error.span.is_none() && !sources.files.iter().any(|file| file.path == error.path) {
                error.path = path.clone();
                error = error
                    .at(import.span)
                    .note(format!("while importing {}", target.display()));
            } else if error.message == "import cycle" {
                error.path = path.clone();
                error = error
                    .at(import.span)
                    .note("remove an import to break the cycle");
            }
            error
        })?;
    }
    loading.remove(&path);
    visited.insert(path.clone());
    modules.push(LoadedModule {
        source: id,
        path,
        module,
    });
    Ok(())
}

fn load_standard(
    name: &str,
    modules: &mut Vec<LoadedModule>,
    visited: &mut HashSet<PathBuf>,
    sources: &mut SourceMap,
) -> Result<(), Error> {
    let path = PathBuf::from(format!("<stdlib>/{name}.nerv"));
    if !visited.insert(path.clone()) {
        return Ok(());
    }
    if name == "std" {
        load_standard("core", modules, visited, sources)?;
    }
    let text = if name == "core" {
        include_str!("../../../stdlib/core.nerv")
    } else {
        include_str!("../../../stdlib/std.nerv")
    };
    let source = sources.add(path.clone(), text.to_owned());
    let module =
        syntax::parse_source(text, source).map_err(|error| frontend_error(&path, source, error))?;
    modules.push(LoadedModule {
        source,
        path,
        module,
    });
    Ok(())
}

fn check(modules: &[LoadedModule]) -> Result<(), Errors> {
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
                )
                .at(structure.span)
                .into());
            }
        }
        for enumeration in &loaded.module.enums {
            if enumeration.variants.is_empty() {
                return Err(error_at(
                    &loaded.path,
                    enumeration.line,
                    enumeration.column,
                    "enum needs at least one variant",
                )
                .at(enumeration.span)
                .into());
            }

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
                    )
                    .at(variant.span)
                    .into());
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
                )
                .at(trait_decl.span)
                .into());
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
                    )
                    .at(implementation.span)
                    .into());
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
                )
                .at(function.span)
                .into());
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
                    )
                    .at(implementation.span)
                    .into());
                }
            }
        }
    }
    let mut errors = Vec::new();
    for loaded in modules {
        for function in all_functions(&loaded.module) {
            if let Err(error) = check_function(
                &loaded.path,
                function,
                &functions,
                &structs,
                &variants,
                &implementations,
            ) {
                errors.push(error.at(function.span));
                // A function is a recovery boundary: don't invent types for failed
                // bindings, or report dependent errors from the rest of its body.
                if errors.len() == 20 {
                    errors
                        .last_mut()
                        .unwrap()
                        .notes
                        .push("stopped after 20 errors; fix these before checking again".into());
                    return Err(Errors(errors));
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Errors(errors))
    }
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
            )
            .at(parameter.span));
        }
    }
    let mut last = Type::Void;
    let mut returns = false;
    for statement in &function.body {
        last = (|| {
            Ok(match &statement.kind {
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
                    if actual == Type::Void {
                        return Err(error_at(path, *line, *column, "cannot bind a void value"));
                    }
                    if let Some(expected) = ty
                        && *expected != actual
                        && !implements(&actual, expected, implementations)
                    {
                        return Err(error_at(
                            path,
                            *line,
                            *column,
                            format!("expected {expected:?}, found {actual:?}"),
                        )
                        .at(value.span)
                        .note("the initializer must match the binding's type annotation"));
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
                        return Err(error_at(path, 0, 0, "assignment type mismatch")
                            .at(value.span)
                            .note(format!("expected {:?}, found {actual:?}", expected.ty)));
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
                        || expr_type(path, value, &values, functions, structs, variants)?
                            != *element
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
                    if expr_type(path, condition, &values, functions, structs, variants)?
                        != Type::Bool
                    {
                        return Err(
                            error_at(path, 0, 0, "while condition must be bool").at(condition.span)
                        );
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
                    if function.result == Type::Void {
                        return Err(error_at(
                            path,
                            function.line,
                            function.column,
                            "void functions do not return a value",
                        )
                        .at(value.span));
                    }
                    let actual = expr_type(path, value, &values, functions, structs, variants)?;
                    if actual != function.result {
                        return Err(error_at(
                            path,
                            function.line,
                            function.column,
                            format!("expected {:?}, found {actual:?}", function.result),
                        )
                        .at(value.span)
                        .note(format!(
                            "function `{}` declares result {:?}",
                            function.name, function.result
                        )));
                    }
                    returns = true;
                    Type::Void
                }
                Statement::Expr(value) => {
                    expr_type(path, value, &values, functions, structs, variants)?
                }
            })
        })()
        .map_err(|error: Error| error.at(statement.span))?;
    }
    if function.result != Type::Void && !returns && last != function.result {
        return Err(error_at(
            path,
            function.line,
            function.column,
            format!("expected {:?}, found {last:?}", function.result),
        )
        .at(function
            .body
            .last()
            .map_or(function.span, |statement| match &statement.kind {
                Statement::Expr(value) => value.span,
                _ => statement.span,
            }))
        .note(format!(
            "function `{}` declares result {:?}",
            function.name, function.result
        )));
    }
    Ok(())
}

fn expr_type(
    path: &Path,
    expr: &SpannedExpr,
    values: &HashMap<String, Binding>,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
) -> Result<Type, Error> {
    expr_type_inner(path, expr, values, functions, structs, variants)
        .map_err(|error| error.at(expr.span))
}

fn expr_type_inner(
    path: &Path,
    expr: &SpannedExpr,
    values: &HashMap<String, Binding>,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
) -> Result<Type, Error> {
    match &expr.kind {
        Expr::Int(_) => Ok(Type::Int),
        Expr::Float(_) => Ok(Type::Float),
        Expr::String(_) => Ok(Type::String),
        Expr::Bool(_) => Ok(Type::Bool),
        Expr::Array(elements) => {
            let element = elements
                .first()
                .map(|e| expr_type(path, e, values, functions, structs, variants))
                .transpose()?
                .unwrap_or(Type::Int);
            for value in elements {
                if expr_type(path, value, values, functions, structs, variants)? != element {
                    return Err(
                        error_at(path, 0, 0, "array elements must have the same type")
                            .at(value.span),
                    );
                }
            }
            Ok(Type::Array(Box::new(element)))
        }
        Expr::ArrayRepeat { value, length } => {
            if expr_type(path, length, values, functions, structs, variants)? != Type::Int {
                return Err(error_at(path, 0, 0, "array length must be i64").at(length.span));
            }
            Ok(Type::Array(Box::new(expr_type(
                path, value, values, functions, structs, variants,
            )?)))
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
            .ok_or_else(|| {
                error_at(path, 0, 0, format!("unknown name `{name}`"))
                    .note("names must be declared in this scope before use; check the spelling")
            }),
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
                return Err(error_at(path, 0, 0, "lambda result type mismatch")
                    .at(body.span)
                    .note(format!("expected {result:?}, found {actual:?}")));
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
                return Err(error_at(path, 0, 0, "wrong argument count").note(format!(
                    "method `{name}` expects {} arguments, found {}",
                    function.params.len().saturating_sub(1),
                    args.len()
                )));
            }
            if function.params[0].ty != Type::Named(type_name.clone()) {
                return Err(error_at(path, 0, 0, "invalid method receiver"));
            }
            for (arg, parameter) in args.iter().zip(function.params.iter().skip(1)) {
                let actual = expr_type(path, arg, values, functions, structs, variants)?;
                if actual != parameter.ty {
                    return Err(error_at(
                        path,
                        parameter.line,
                        parameter.column,
                        "argument type mismatch",
                    )
                    .at(arg.span)
                    .note(format!(
                        "parameter `{}` expects {:?}, found {actual:?}",
                        parameter.name, parameter.ty
                    )));
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
                return Err(error_at(path, 0, 0, "if condition must be bool").at(condition.span));
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
                (UnaryOp::Neg, Type::Int | Type::Integer { .. } | Type::Float | Type::Float32)
                | (UnaryOp::Not, Type::Bool) => Ok(ty),
                _ => Err(error_at(path, 0, 0, "invalid unary operation")),
            }
        }
        Expr::Binary { left, op, right } => {
            let right_span = right.span;
            let left = expr_type(path, left, values, functions, structs, variants)?;
            let right = expr_type(path, right, values, functions, structs, variants)?;
            match op {
                BinaryOp::And | BinaryOp::Or if left == Type::Bool && right == Type::Bool => {
                    Ok(Type::Bool)
                }
                BinaryOp::Eq | BinaryOp::NotEq if left == right => {
                    let supported = match &left {
                        Type::Int
                        | Type::Integer { .. }
                        | Type::Float
                        | Type::Float32
                        | Type::Bool
                        | Type::String
                        | Type::Array(_) => true,
                        Type::Named(name) => {
                            let members = variants
                                .values()
                                .filter(|info| info.enumeration == *name)
                                .collect::<Vec<_>>();
                            !members.is_empty() && members.iter().all(|info| info.payload.is_none())
                        }
                        _ => false,
                    };
                    if supported {
                        Ok(Type::Bool)
                    } else {
                        Err(error_at(
                            path,
                            0,
                            0,
                            "equality is not supported for this type",
                        ))
                    }
                }
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
                _ => Err(error_at(path, 0, 0, "invalid binary operation")
                    .at(right_span)
                    .note(format!(
                        "operator {op:?} cannot combine {left:?} and {right:?}"
                    ))),
            }
        }
        Expr::Call { callee, args } => {
            let name = callee_name(callee);
            let argument_exprs = args;
            let args = args
                .iter()
                .map(|arg| expr_type(path, arg, values, functions, structs, variants))
                .collect::<Result<Vec<_>, _>>()?;
            let Some(name) = name else {
                let Type::Function { params, result } =
                    expr_type(path, callee, values, functions, structs, variants)?
                else {
                    return Err(error_at(path, 0, 0, "unsupported call target"));
                };
                if args != params {
                    return Err(error_at(path, 0, 0, "function argument type mismatch")
                        .note(format!("expected {params:?}, found {args:?}")));
                }
                return Ok(*result);
            };
            if let Some(result) = runtime_type(&name, &args) {
                return result.ok_or_else(|| error_at(path, 0, 0, "invalid runtime call")
                    .note(format!("`{name}` does not accept argument types {args:?}; check the builtin signature")));
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
                    return Err(error_at(path, 0, 0, "function argument type mismatch")
                        .note(format!("expected {params:?}, found {args:?}")));
                }
                return Ok((**result).clone());
            }
            let function = functions.get(name.as_str()).ok_or_else(|| {
                error_at(path, 0, 0, format!("unknown function `{name}`"))
                    .at(callee.span)
                    .note("check the spelling and import the module that defines this function")
            })?;
            if args.len() != function.params.len() {
                return Err(
                    error_at(path, function.line, function.column, "wrong argument count")
                        .at(expr.span)
                        .note(format!(
                            "function `{name}` expects {} arguments, found {}",
                            function.params.len(),
                            args.len()
                        )),
                );
            }
            for ((arg, parameter), source_arg) in
                args.iter().zip(&function.params).zip(argument_exprs)
            {
                if *arg != parameter.ty {
                    return Err(error_at(
                        path,
                        parameter.line,
                        parameter.column,
                        "argument type mismatch",
                    )
                    .at(source_arg.span)
                    .note(format!(
                        "parameter `{}` expects {:?}, found {arg:?}",
                        parameter.name, parameter.ty
                    )));
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
                let actual = expr_type(path, value, values, functions, structs, variants)?;
                if actual != field.ty {
                    return Err(error_at(path, 0, 0, "struct field type mismatch")
                        .at(value.span)
                        .note(format!(
                            "field `{name}` expects {:?}, found {actual:?}",
                            field.ty
                        )));
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
                return Err(error_at(path, 0, 0, "array index must be an integer").at(index.span));
            }
            Ok(*element)
        }
    }
}

fn check_field_assignment(
    path: &Path,
    base: &SpannedExpr,
    name: &str,
    value: &SpannedExpr,
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
        return Err(error_at(path, 0, 0, "field assignment type mismatch")
            .at(value.span)
            .note(format!(
                "field `{name}` expects {:?}, found {actual:?}",
                field.ty
            )));
    }
    Ok(())
}

fn require_mutable(
    path: &Path,
    base: &SpannedExpr,
    values: &HashMap<String, Binding>,
) -> Result<(), Error> {
    let Expr::Name(name) = &base.kind else {
        return Err(error_at(path, 0, 0, "assignment target must be a binding"));
    };
    let binding = values
        .get(name)
        .ok_or_else(|| error_at(path, 0, 0, format!("unknown name `{name}`")))?;
    if binding.mutable {
        Ok(())
    } else {
        Err(error_at(path, 0, 0, format!("cannot assign to `{name}`"))
            .at(base.span)
            .note("declare the binding with `let mut` to allow assignment"))
    }
}

fn callee_name(expr: &SpannedExpr) -> Option<String> {
    match &expr.kind {
        Expr::Name(name) => Some(name.clone()),
        Expr::Field { base, name } => Some(format!("{}.{}", callee_name(base)?, name)),
        _ => None,
    }
}

fn runtime_type(name: &str, args: &[Type]) -> Option<Option<Type>> {
    if let Some(ty) = syntax::numeric_type(name) {
        return Some(if args.len() == 1 && numeric(&args[0]) {
            Some(ty)
        } else {
            None
        });
    }
    let strings = || Type::Array(Box::new(Type::String));
    match (name, args) {
        (
            "nerv.io.read_text" | "nerv.io.list_directory" | "nerv.io.environment",
            [Type::String],
        ) => return Some(Some(strings())),
        ("nerv.io.arguments" | "nerv.io.read_stdin", []) => return Some(Some(strings())),
        ("nerv.io.write_text", [Type::String, Type::String])
        | ("nerv.io.create_directory", [Type::String]) => return Some(Some(Type::String)),
        ("nerv.io.stderr", [Type::String]) | ("nerv.io.sleep_ms", [Type::Int]) => {
            return Some(Some(Type::Void));
        }
        ("nerv.io.now_ms", []) | ("nerv.arrays.len", [Type::Array(_)]) => {
            return Some(Some(Type::Int));
        }
        _ => {}
    }
    Some(match (name, args) {
        (
            "print" | "println",
            [
                Type::Int
                | Type::Integer { .. }
                | Type::Float
                | Type::Float32
                | Type::Bool
                | Type::String,
            ],
        ) => Some(Type::Void),
        ("assert", [Type::Bool]) => Some(Type::Void),
        ("i64", [Type::Int | Type::Float]) => Some(Type::Int),
        ("f64", [Type::Int | Type::Float]) => Some(Type::Float),
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
            | "assert"
            | "i64"
            | "f64"
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
    body: &[SpannedStatement],
    values: &mut HashMap<String, Binding>,
    functions: &HashMap<&str, &Function>,
    structs: &HashMap<&str, &StructDecl>,
    variants: &HashMap<&str, VariantInfo>,
) -> Result<Type, Error> {
    let mut last = Type::Void;
    for statement in body {
        last = (|| {
            Ok(match &statement.kind {
                Statement::Let {
                    name,
                    mutable,
                    ty,
                    value,
                    ..
                } => {
                    let actual = expr_type(path, value, values, functions, structs, variants)?;
                    if actual == Type::Void {
                        return Err(error_at(path, 0, 0, "cannot bind a void value"));
                    }
                    if let Some(expected) = ty
                        && *expected != actual
                    {
                        return Err(error_at(path, 0, 0, "let binding type mismatch")
                            .at(value.span)
                            .note(format!("expected {expected:?}, found {actual:?}")));
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
                        return Err(error_at(path, 0, 0, "assignment type mismatch")
                            .at(value.span)
                            .note(format!("expected {:?}, found {actual:?}", expected.ty)));
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
                    if expr_type(path, condition, values, functions, structs, variants)?
                        != Type::Bool
                    {
                        return Err(
                            error_at(path, 0, 0, "while condition must be bool").at(condition.span)
                        );
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
                Statement::Return(_) => {
                    return Err(error_at(
                        path,
                        0,
                        0,
                        "return is only supported directly in a function body",
                    ));
                }
                Statement::Expr(value) => {
                    expr_type(path, value, values, functions, structs, variants)?
                }
            })
        })()
        .map_err(|error: Error| error.at(statement.span))?;
    }
    Ok(last)
}

fn numeric(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int | Type::Integer { .. } | Type::Float | Type::Float32
    )
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
        span: None,
        source: None,
        notes: Vec::new(),
        path: path.to_path_buf(),
        line,
        column,
        message: message.into(),
    }
}
fn frontend_error(path: &Path, source: SourceId, error: syntax::FrontendError) -> Error {
    match error {
        syntax::FrontendError::Lex(error) => {
            error_at(path, error.line, error.column, error.to_string()).at(Span {
                source,
                start: error.span.start,
                end: error.span.end,
            })
        }
        syntax::FrontendError::Parse(error) => {
            error_at(path, error.line, error.column, error.message).at(error.span)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn test_directory() -> PathBuf {
        std::env::temp_dir().join(format!(
            "nerv-sema-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn loaded_sources_and_cloned_modules_retain_ids() {
        let root = test_directory();
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("helper.nerv"), "fn answer() -> i64 => 42\n").unwrap();
        fs::write(
            root.join("main.nerv"),
            "import helper\nimport helper\nfn main() -> i64 => answer()\n",
        )
        .unwrap();
        let program = check_path(root.join("main.nerv")).unwrap();
        assert_eq!(program.sources.files.len(), 2);
        assert_eq!(program.modules.len(), 2);
        for loaded in &program.modules {
            assert_eq!(loaded.source, loaded.module.source);
            assert_eq!(loaded.path, program.sources.get(loaded.source).path);
            let cloned = loaded.module.clone();
            assert_eq!(cloned.functions[0].span.source, loaded.source);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn errors_are_bounded_and_keep_the_original_source() {
        let root = test_directory();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("main.nerv");
        let source = (0..25)
            .map(|index| format!("fn bad_{index}() -> i64 => false\n"))
            .collect::<String>();
        fs::write(&path, source).unwrap();
        let errors = check_path(&path).unwrap_err();
        assert_eq!(errors.0.len(), 20);
        assert!(
            errors
                .0
                .iter()
                .all(|error| error.span.is_some() && error.line > 0 && error.column > 0)
        );
        fs::write(&path, "changed after checking").unwrap();
        let rendered = errors.to_string();
        assert!(rendered.contains("fn bad_0() -> i64 => false"));
        assert!(!rendered.contains("changed after checking"));
        assert!(rendered.contains("stopped after 20 errors"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checks_imported_functions() {
        let root = test_directory();
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
        let root = test_directory();
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
        let root = test_directory();
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
        let root = test_directory();
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
