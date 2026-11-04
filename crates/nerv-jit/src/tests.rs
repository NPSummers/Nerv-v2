use super::*;
use nerv_syntax::parse;

fn run(source: &str) -> i64 {
    let module = parse(source).unwrap();
    let checked = nerv_sema::check_modules(&[module]).unwrap();
    run_main(&checked).unwrap()
}

#[test]
fn creates_a_context() {
    assert!(!Context::new().as_raw().is_null());
}

#[test]
fn runs_functions_and_calls() {
    assert_eq!(
        run(
            "fn add(left: i64, right: i64) -> i64 => left + right\nfn main() -> i64\n    let value = add(20, 22)\n    return value\n"
        ),
        42
    );
}

#[test]
fn runs_conditional_control_flow() {
    assert_eq!(
        run(
            "fn choose(flag: bool, left: i64, right: i64) -> i64\n    if flag\n        left\n    else\n        right\nfn main() -> i64 => choose(true, 42, 0)\n"
        ),
        42
    );
}

#[test]
fn calls_the_runtime() {
    assert_eq!(
        run("fn main() -> i64\n    println(\"Nerv JIT\")\n    println(42)\n    return 0\n"),
        0
    );
}

#[test]
fn calls_runtime_namespaces() {
    assert_eq!(
        run(
            "fn main() -> i64\n    println(nerv.math.sqrt(9.0))\n    println(nerv.string.concat(\"Ne\", \"rv\"))\n    return nerv.string.len(\"Nerv\")\n"
        ),
        4
    );
}

#[test]
fn runs_struct_values() {
    assert_eq!(
        run(
            "struct Pair\n    left: i64\n    right: i64\nfn sum(pair: Pair) -> i64 => pair.left + pair.right\nfn main() -> i64 => sum(Pair { left: 19, right: 23 })\n"
        ),
        42
    );
}

#[test]
fn updates_struct_fields() {
    assert_eq!(
        run(
            "struct Pair\n    left: i64\n    right: i64\nfn main() -> i64\n    let mut pair = Pair { left: 19, right: 0 }\n    pair.right = 23\n    return pair.left + pair.right\n"
        ),
        42
    );
}

#[test]
fn lowers_non_capturing_lambdas() {
    assert_eq!(
        run(
            "fn main() -> i64\n    let callback: fn(i64) -> i64 = fn(value: i64) -> i64 => value + 1\n    return 42\n"
        ),
        42
    );
}

#[test]
fn calls_non_capturing_lambdas() {
    assert_eq!(
        run("fn main() -> i64 => (fn(value: i64) -> i64 => value + 1)(41)\n"),
        42
    );
}

#[test]
fn calls_function_values() {
    assert_eq!(
        run(
            "fn add_one(value: i64) -> i64 => value + 1\nfn apply(callback: fn(i64) -> i64, value: i64) -> i64 => callback(value)\nfn main() -> i64 => apply(add_one, 41)\n"
        ),
        42
    );
}

#[test]
fn maps_and_reduces_function_values() {
    assert_eq!(
        run(
            "fn double(value: i64) -> i64 => value * 2\nfn add(total: i64, value: i64) -> i64 => total + value\nfn main() -> i64\n    let values: []i64 = [1, 2, 3]\n    let doubled = nerv.arrays.map(values, 3, double)\n    return nerv.arrays.reduce(doubled, 3, 0, add)\n"
        ),
        12
    );
}

#[test]
fn runs_mutable_loops() {
    assert_eq!(
        run(
            "fn main() -> i64\n    let mut total = 0\n    for value in 1..=4\n        total = total + value\n    while total < 42\n        total = total + 1\n    return total\n"
        ),
        42
    );
}

#[test]
fn runs_void_main() {
    assert_eq!(run("fn main() -> void\n    println(\"Nerv\")\n"), 0);
}

#[test]
fn runs_fieldless_enums() {
    assert_eq!(
        run(
            "enum Flag\n    Off\n    On\nfn main() -> i64\n    let state: Flag = On()\n    if state == Off()\n        0\n    else\n        42\n"
        ),
        42
    );
}

#[test]
fn constructs_i64_enum_payloads() {
    assert_eq!(
        run(
            "enum Choice\n    Some(i64)\n    None\nfn make() -> Choice => Some(4)\nfn main() -> i64\n    make()\n    return 42\n"
        ),
        42
    );
}

#[test]
fn matches_enum_payloads() {
    assert_eq!(
        run(
            "enum Choice\n    Some(i64)\n    None\nfn main() -> i64\n    match Some(42)\n        Some(value) => value\n        None => 0\n"
        ),
        42
    );
}

#[test]
fn dispatches_implementation_methods() {
    assert_eq!(
        run(
            "trait Drawable\n    fn draw(self) -> i64\n\nenum Light\n    On\n\nimpl Light for Drawable\n    fn draw(self) -> i64 => 42\n\nfn main() -> i64\n    let light = On()\n    let drawable: Drawable = light\n    drawable:draw()\n"
        ),
        42
    );
}

#[test]
fn runs_i64_arrays() {
    assert_eq!(
        run(
            "fn main() -> i64\n    let mut values: []i64 = [1, 2, 3]\n    values[1] = 40\n    return values[0] + values[1] + values[2]\n"
        ),
        44
    );
}

#[test]
fn runs_heavy_loop_shape() {
    assert!((15..=18).contains(&run("fn main() -> i64\n    let r = nerv.random(4)\n    let mut values: []i64 = nerv.arrays.new(4, 0)\n    for i in 0..4\n        for j in 0..16\n            values[i] = values[i] + (j % 3)\n        values[i] = values[i] + r\n    return values[0]\n")));
}

#[test]
fn dispatches_methods_on_nominal_enums_with_identical_llvm_layouts() {
    assert_eq!(
        run(
            "enum First\n    A\nenum Second\n    B\nimpl First\n    fn value(self) -> i64 => 10\nimpl Second\n    fn value(self) -> i64 => 32\nfn main() -> i64\n    let first = A()\n    let second = B()\n    first:value() + second:value()\n"
        ),
        42
    );
}

#[test]
fn calls_returned_function_values_and_branch_local_callbacks() {
    assert_eq!(
        run(
            "fn choose(flag: bool) -> fn(i64) -> i64\n    if flag\n        fn(value: i64) -> i64 => value + 1\n    else\n        fn(value: i64) -> i64 => value + 2\nfn main() -> i64\n    if true\n        let callback = choose(true)\n        callback(41)\n    else\n        0\n"
        ),
        42
    );
    assert_eq!(
        run(
            "fn make() -> fn(i64) -> i64 => fn(value: i64) -> i64 => value + 1\nfn main() -> i64 => make()(41)\n"
        ),
        42
    );
}

#[test]
fn separates_runtime_symbols_from_user_functions() {
    assert_eq!(
        run(
            "fn nerv_println_i64(value: i64) -> i64 => value + 1\nfn main() -> i64\n    println(2)\n    nerv_println_i64(41)\n"
        ),
        42
    );
}

#[test]
fn preserves_function_namespace_when_a_non_callable_local_has_the_same_name() {
    assert_eq!(
        run("fn answer() -> i64 => 42\nfn main() -> i64\n    let answer = 0\n    answer()\n"),
        42
    );
}

#[test]
fn supports_forward_recursive_calls() {
    assert_eq!(
        run(
            "fn main() -> i64 => factorial(5)\nfn factorial(value: i64) -> i64\n    if value == 0\n        1\n    else\n        value * factorial(value - 1)\n"
        ),
        120
    );
}

#[test]
fn runs_resolved_test_and_benchmark_entry_groups() {
    let module =
        parse("test \"works\"\n    assert(true)\nbench \"fast\"\n    let answer = 42\n").unwrap();
    let checked = nerv_sema::check_modules(&[module]).unwrap();
    assert_eq!(run_tests(&checked).unwrap(), 1);
    assert_eq!(run_benches(&checked).unwrap(), 1);
}
