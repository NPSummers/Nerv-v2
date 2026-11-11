//! Checked, resolved operations shared by compiler backends. No syntax expressions
//! or unresolved source names occur in this representation.
pub use nerv_syntax::{BinaryOp, SourceId, Span, UnaryOp};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SymbolId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Void,
    Bool,
    Int,
    Integer {
        bits: u32,
        signed: bool,
    },
    Float,
    Float32,
    String,
    Array(Box<Type>),
    Function {
        params: Vec<Type>,
        result: Box<Type>,
    },
    Named(SymbolId),
}

/// Only semantic analysis can construct a checked program. Backends get immutable
/// access, so they can rely on resolution and type invariants.
#[derive(Debug)]
pub struct CheckedProgram {
    pub(crate) functions: Vec<Function>,
    pub(crate) structs: Vec<Struct>,
    pub(crate) enums: Vec<Enum>,
    pub(crate) main: Option<SymbolId>,
    pub(crate) process_entry: bool,
    pub(crate) tests: Vec<SymbolId>,
    pub(crate) benches: Vec<SymbolId>,
}

impl CheckedProgram {
    pub fn set_process_entry(&mut self, enabled: bool) {
        self.process_entry = enabled;
    }

    pub fn process_entry(&self) -> bool {
        self.process_entry
    }

    pub fn functions(&self) -> &[Function] {
        &self.functions
    }
    pub fn structs(&self) -> &[Struct] {
        &self.structs
    }
    pub fn enums(&self) -> &[Enum] {
        &self.enums
    }
    pub fn main(&self) -> Option<SymbolId> {
        self.main
    }
    pub fn tests(&self) -> &[SymbolId] {
        &self.tests
    }
    pub fn benches(&self) -> &[SymbolId] {
        &self.benches
    }
    pub fn function(&self, id: SymbolId) -> &Function {
        self.functions
            .iter()
            .find(|function| function.id == id)
            .expect("checked function ID")
    }
}

#[derive(Debug, Clone)]
pub struct Binding {
    pub id: SymbolId,
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Function {
    pub id: SymbolId,
    pub name: String,
    pub params: Vec<Binding>,
    pub result: Type,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Struct {
    pub id: SymbolId,
    pub name: String,
    pub fields: Vec<Binding>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Enum {
    pub id: SymbolId,
    pub name: String,
    pub variants: Vec<Variant>,
    pub span: Span,
}

impl Enum {
    pub fn has_payload(&self) -> bool {
        self.variants
            .iter()
            .any(|variant| variant.payload.is_some())
    }
}

#[derive(Debug, Clone)]
pub struct Variant {
    pub id: SymbolId,
    pub name: String,
    pub tag: i64,
    pub payload: Option<Type>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Statement {
    pub span: Span,
    pub ty: Type,
    pub kind: StatementKind,
}

#[derive(Debug, Clone)]
pub enum StatementKind {
    Let {
        binding: Binding,
        value: Expr,
    },
    Assign {
        target: SymbolId,
        value: Expr,
    },
    AssignIndex {
        base: Expr,
        index: Expr,
        value: Expr,
    },
    AssignField {
        target: SymbolId,
        index: u32,
        value: Expr,
    },
    While {
        condition: Expr,
        body: Vec<Statement>,
    },
    For {
        binding: Binding,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Vec<Statement>,
    },
    Return(Expr),
    ReturnVoid,
    Break,
    Continue,
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub span: Span,
    pub ty: Type,
    pub kind: ExprKind,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Try {
        value: Box<Expr>,
        failure_enum: SymbolId,
        failure_tag: i64,
        has_error: bool,
    },
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Local(SymbolId),
    Function(SymbolId),
    Closure {
        function: SymbolId,
        captures: Vec<Expr>,
    },
    Array(Vec<Expr>),
    ArrayRepeat {
        value: Box<Expr>,
        length: Box<Expr>,
    },
    Struct {
        structure: SymbolId,
        fields: Vec<(u32, Expr)>,
    },
    Enum {
        enumeration: SymbolId,
        variant: SymbolId,
        tag: i64,
        payload: Option<Box<Expr>>,
    },
    Field {
        base: Box<Expr>,
        index: u32,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_body: Vec<Statement>,
        else_body: Vec<Statement>,
    },
    Match {
        value: Box<Expr>,
        enumeration: SymbolId,
        arms: Vec<MatchArm>,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        target: CallTarget,
        args: Vec<Expr>,
    },
    /// The runtime ABI symbol and overload are selected by semantic analysis.
    Runtime {
        symbol: String,
        args: Vec<Expr>,
    },
    Assert(Box<Expr>),
    Convert {
        conversion: Conversion,
        value: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
pub enum CallTarget {
    Direct(SymbolId),
    Indirect(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conversion {
    Numeric,
    Identity,
    IntToFloat,
    FloatToInt,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub variant: SymbolId,
    pub tag: i64,
    pub binding: Option<Binding>,
    pub body: Expr,
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check_modules;
    use nerv_syntax::{SourceId, parse, parse_source};

    fn checked(source: &str) -> CheckedProgram {
        check_modules(&[parse(source).unwrap()]).unwrap()
    }

    fn expression(statement: &Statement) -> &Expr {
        let StatementKind::Expr(expr) = &statement.kind else {
            panic!("expected expression")
        };
        expr
    }

    #[test]
    fn resolves_shadowing_to_distinct_stable_ids() {
        let source = "fn number() -> i64 => 7\nfn main() -> i64\n    let value = number()\n    let value = value + 1\n    if true\n        let value = value + 2\n        value\n    else\n        value\n";
        let first = checked(source);
        let second = checked(source);
        assert_eq!(
            first.functions().iter().map(|f| f.id).collect::<Vec<_>>(),
            second.functions().iter().map(|f| f.id).collect::<Vec<_>>()
        );
        let body = &first.function(first.main().unwrap()).body;
        let StatementKind::Let {
            binding: outer,
            value: call,
        } = &body[0].kind
        else {
            panic!()
        };
        let ExprKind::Call {
            target: CallTarget::Direct(number),
            ..
        } = call.kind
        else {
            panic!()
        };
        assert_eq!(first.function(number).name, "number");
        let StatementKind::Let {
            binding: shadow,
            value,
        } = &body[1].kind
        else {
            panic!()
        };
        assert_ne!(outer.id, shadow.id);
        assert_eq!(outer.ty, Type::Int);
        let ExprKind::Binary { left, .. } = &value.kind else {
            panic!()
        };
        assert!(matches!(left.kind, ExprKind::Local(id) if id == outer.id));
        let ExprKind::If {
            then_body,
            else_body,
            ..
        } = &expression(&body[2]).kind
        else {
            panic!()
        };
        let StatementKind::Let {
            binding: inner,
            value,
        } = &then_body[0].kind
        else {
            panic!()
        };
        let ExprKind::Binary { left, .. } = &value.kind else {
            panic!()
        };
        assert!(matches!(left.kind, ExprKind::Local(id) if id == shadow.id));
        assert!(matches!(expression(&then_body[1]).kind, ExprKind::Local(id) if id == inner.id));
        assert!(matches!(expression(&else_body[0]).kind, ExprKind::Local(id) if id == shadow.id));
    }

    #[test]
    fn resolves_fields_methods_constructors_matches_and_spans() {
        let source = "struct Pair\n    x: i64\n    y: i64\nenum Choice\n    Some(i64)\n    None\nimpl Pair\n    fn score(self) -> i64 => self.y\nfn main() -> i64\n    let pair = Pair { y: 2, x: 40 }\n    let choice = Some(pair:score())\n    match choice\n        Some(result) => result\n        None => 0\n";
        let program = check_modules(&[parse_source(source, SourceId(42)).unwrap()]).unwrap();
        let main = program.function(program.main().unwrap());
        assert_eq!(main.span.source, SourceId(42));
        let StatementKind::Let { binding, value } = &main.body[0].kind else {
            panic!()
        };
        let ExprKind::Struct { structure, fields } = &value.kind else {
            panic!()
        };
        assert_eq!(binding.ty, Type::Named(*structure));
        assert_eq!(
            fields.iter().map(|(index, _)| *index).collect::<Vec<_>>(),
            [1, 0]
        );
        assert_eq!(
            &source[value.span.start..value.span.end],
            "Pair { y: 2, x: 40 }"
        );
        let StatementKind::Let { value, .. } = &main.body[1].kind else {
            panic!()
        };
        let ExprKind::Enum {
            enumeration,
            variant,
            tag,
            payload: Some(payload),
        } = &value.kind
        else {
            panic!()
        };
        assert_eq!(*tag, 0);
        assert_eq!(value.ty, Type::Named(*enumeration));
        let ExprKind::Call {
            target: CallTarget::Direct(method),
            args,
        } = &payload.kind
        else {
            panic!()
        };
        assert_eq!(args.len(), 1);
        let method = program.function(*method);
        assert!(matches!(
            expression(&method.body[0]).kind,
            ExprKind::Field { index: 1, .. }
        ));
        let ExprKind::Match { arms, .. } = &expression(&main.body[2]).kind else {
            panic!()
        };
        assert_eq!(arms[0].variant, *variant);
        let binding = arms[0].binding.as_ref().unwrap();
        assert_eq!(binding.ty, Type::Int);
        assert!(matches!(arms[0].body.kind, ExprKind::Local(id) if id == binding.id));
    }

    #[test]
    fn lifts_lambdas_and_records_inferred_signatures_and_conversions() {
        let program = checked(
            "fn main() -> i64\n    let callback = fn(value: i64) -> i64 => value + 1\n    callback(i64(f64(41)))\n",
        );
        assert_eq!(program.functions().len(), 2);
        let main = program.function(program.main().unwrap());
        let StatementKind::Let { binding, value } = &main.body[0].kind else {
            panic!()
        };
        assert_eq!(
            binding.ty,
            Type::Function {
                params: vec![Type::Int],
                result: Box::new(Type::Int)
            }
        );
        let ExprKind::Closure {
            function: lambda, ..
        } = value.kind
        else {
            panic!()
        };
        assert_ne!(lambda, main.id);
        assert_eq!(program.function(lambda).params[0].ty, Type::Int);
        let ExprKind::Call {
            target: CallTarget::Indirect(callee),
            args,
        } = &expression(&main.body[1]).kind
        else {
            panic!()
        };
        assert_eq!(callee.ty, binding.ty);
        let ExprKind::Convert {
            conversion: Conversion::FloatToInt,
            value,
        } = &args[0].kind
        else {
            panic!()
        };
        assert!(matches!(
            value.kind,
            ExprKind::Convert {
                conversion: Conversion::IntToFloat,
                ..
            }
        ));
    }

    #[test]
    fn selects_runtime_abi_overloads_before_codegen() {
        let program = checked(
            "fn main() -> void\n    println(42)\n    println(1.0)\n    println(true)\n    println(\"Nerv\")\n",
        );
        let main = program.function(program.main().unwrap());
        let symbols = main
            .body
            .iter()
            .map(|statement| {
                let expr = expression(statement);
                assert_eq!(expr.ty, Type::Void);
                let ExprKind::Runtime { symbol, .. } = &expr.kind else {
                    panic!()
                };
                symbol.as_str()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            symbols,
            [
                "nerv_println_i64",
                "nerv_println_f64",
                "nerv_println_bool",
                "nerv_println_string"
            ]
        );
    }

    #[test]
    fn rejects_unlowerable_programs_before_constructing_checked_ir() {
        for (source, message) in [
            (
                "struct Bad\n    value: void\n",
                "struct field cannot have type void",
            ),
            ("struct Bad\n    next: Bad\n", "infinite size"),
            (
                "struct A\n    next: B\nstruct B\n    next: A\n",
                "infinite size",
            ),
            (
                "fn bad(value: Missing) -> void\n    println(1)\n",
                "unknown type",
            ),
            (
                "fn bad(callback: fn(void) -> i64) -> void\n    println(1)\n",
                "parameter cannot have type void",
            ),
            (
                "enum Bad\n    Text(void)\n",
                "payload cannot have type void",
            ),
            ("enum Bad\n    Next(Bad)\n", "infinite size"),
            (
                "struct A\n    next: B\nenum B\n    Next(A)\n",
                "infinite size",
            ),
            (
                "fn bad(values: []void) -> void\n    println(1)\n",
                "array element cannot have type void",
            ),
            (
                "fn main(value: i64) -> i64 => value\n",
                "`main` must have type",
            ),
            (
                "struct Pair\n    x: i64\n    y: i64\nfn main() -> i64\n    let pair = Pair { x: 1, x: 2 }\n    pair.x\n",
                "duplicate field",
            ),
            (
                "enum Flag\n    Off\nfn main() -> i64\n    let flag = Wrong.Off()\n    0\n",
                "unknown enum constructor",
            ),
            (
                "fn main() -> i64 => (fn(value: i64, value: i64) -> i64 => value)(1, 2)\n",
                "duplicate parameter",
            ),
        ] {
            let error = check_modules(&[parse(source).unwrap()]).unwrap_err();
            assert!(error.to_string().contains(message), "{source}: {error}");
        }
    }
}
