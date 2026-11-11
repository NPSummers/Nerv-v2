use super::*;
use nerv_syntax::parse;

fn run_main_with_mode(program: &CheckedProgram, mode: BuildMode) -> Result<i64, Error> {
    let _lock = JIT_LOCK.lock().unwrap();
    let _execution = nerv_runtime::Execution::new();
    let mut jit = Jit::new()?;
    jit.add_module(compiler::generate(program, mode, false)?)?;
    jit.run(&symbol(program.main().unwrap()), true)
}

#[test]
fn division_narrowing_preserves_signed_and_boundary_results() {
    let operands = [
        0_i64,
        1,
        22,
        i32::MAX as i64,
        1_i64 << 31,
        u32::MAX as i64,
        (u32::MAX as i64) + 1,
        -1,
        -22,
        i64::MIN,
        i64::MAX,
    ];
    let mut source = String::from(
        "fn quotient(a: i64, b: i64) -> i64 => a / b\nfn remainder(a: i64, b: i64) -> i64 => a % b\nfn main() -> i64\n",
    );
    for a in operands {
        for b in operands.into_iter().filter(|b| *b != 0) {
            let literal = |n: i64| {
                if n == i64::MIN {
                    "(-9223372036854775807 - 1)".to_string()
                } else {
                    format!("({n})")
                }
            };
            source.push_str(&format!(
                "    assert(quotient({}, {}) == {})\n    assert(remainder({}, {}) == {})\n",
                literal(a),
                literal(b),
                literal(a.wrapping_div(b)),
                literal(a),
                literal(b),
                literal(a.wrapping_rem(b))
            ));
        }
    }
    source.push_str("    return 42\n");
    let checked = nerv_sema::check_modules(&[parse(&source).unwrap()]).unwrap();
    for mode in [BuildMode::Debug, BuildMode::Release] {
        assert_eq!(run_main_with_mode(&checked, mode).unwrap(), 42);
    }
}

#[test]
fn inline_array_access_preserves_aliases_across_calls() {
    let source = "fn update(input: []i64) -> i64\n    let mut values = input\n    values[0] = 40\n    return 2\nfn main() -> i64\n    let mut values: []i64 = [1]\n    let alias = values\n    values[0] = update(alias) + alias[0]\n    return values[0]\n";
    let checked = nerv_sema::check_modules(&[parse(source).unwrap()]).unwrap();
    for mode in [BuildMode::Debug, BuildMode::Release] {
        assert_eq!(run_main_with_mode(&checked, mode).unwrap(), 42);
    }
}

fn run(source: &str) -> i64 {
    let module = parse(source).unwrap();
    let checked = nerv_sema::check_modules(&[module]).unwrap();
    run_main(&checked).unwrap()
}

#[test]
fn reciprocal_loops_match_wrapping_i64_division_and_remainder() {
    let mut values = vec![
        0_i64,
        1,
        2,
        3,
        21,
        22,
        23,
        65535,
        65536,
        i32::MAX as i64,
        1_i64 << 31,
        (1_i64 << 32) - 2,
        u32::MAX as i64,
        1_i64 << 32,
        -1,
        -22,
        i64::MIN,
        i64::MAX,
    ];
    let mut state = 0x12345678_u32;
    for _ in 0..12 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        values.push(state as i64);
    }
    let divisors = values
        .iter()
        .copied()
        .filter(|value| *value != 0)
        .collect::<Vec<_>>();
    let expected = divisors
        .iter()
        .flat_map(|d| {
            values
                .iter()
                .flat_map(move |n| [n.wrapping_div(*d), n.wrapping_rem(*d)])
        })
        .collect::<Vec<_>>();
    let list = |values: &[i64]| {
        values
            .iter()
            .map(|n| {
                if *n == i64::MIN {
                    "(-9223372036854775807 - 1)".to_string()
                } else {
                    n.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let source = format!(
        "fn main() -> i64\n    let numerators: []i64 = [{}]\n    let divisors: []i64 = [{}]\n    let expected: []i64 = [{}]\n    for row in 0..{}\n        let divisor = divisors[row]\n        for column in 0..{}\n            let numerator = numerators[column]\n            let offset = (row * {} + column) * 2\n            assert(numerator / divisor == expected[offset])\n            assert(numerator % divisor == expected[offset + 1])\n    return 42\n",
        list(&values),
        list(&divisors),
        list(&expected),
        divisors.len(),
        values.len(),
        values.len()
    );
    let checked = nerv_sema::check_modules(&[parse(&source).unwrap()]).unwrap();
    for mode in [BuildMode::Debug, BuildMode::Release] {
        assert_eq!(run_main_with_mode(&checked, mode).unwrap(), 42);
    }
}

#[test]
fn bounded_reciprocals_match_exact_division_at_proof_boundaries() {
    let divisors = [
        1_i64,
        2,
        3,
        7,
        22,
        31,
        255,
        65535,
        65537,
        i32::MAX as i64,
        1_i64 << 31,
        u32::MAX as i64,
        1_i64 << 32,
        -1,
        -22,
        i64::MIN,
        i64::MAX,
    ];
    let literal = |n: i64| {
        if n == i64::MIN {
            "(-9223372036854775807 - 1)".to_string()
        } else {
            n.to_string()
        }
    };
    let mut source = format!(
        "fn main() -> i64\n    let divisors: []i64 = [{}]\n",
        divisors
            .iter()
            .map(|n| literal(*n))
            .collect::<Vec<_>>()
            .join(", ")
    );
    // For d=3 the rounded reciprocal's error is 2: the exactness predicate
    // changes at B=2^31. Exercise just below, exactly at, and above that bound.
    for (case, start) in [
        0_i64,
        65520,
        (1_i64 << 31) - 64,
        (1_i64 << 31) - 63,
        (1_i64 << 31) - 32,
        (1_i64 << 32) - 64,
    ]
    .into_iter()
    .enumerate()
    {
        let end = start + 63;
        let expected = divisors
            .iter()
            .flat_map(|d| {
                [
                    (start..=end).map(|n| n / d).sum::<i64>(),
                    (start..=end).map(|n| n % d).sum::<i64>(),
                ]
            })
            .map(literal)
            .collect::<Vec<_>>()
            .join(", ");
        source.push_str(&format!("    let expected{case}: []i64 = [{expected}]\n    for row in 0..{}\n        let divisor = divisors[row]\n        let mut quotient = 0\n        let mut remainder = 0\n        for n in {start}..={end}\n            quotient = quotient + n / divisor\n            remainder = remainder + n % divisor\n        assert(quotient == expected{case}[row * 2])\n        assert(remainder == expected{case}[row * 2 + 1])\n", divisors.len()));
    }
    source.push_str("    return 42\n");
    let checked = nerv_sema::check_modules(&[parse(&source).unwrap()]).unwrap();
    for mode in [BuildMode::Debug, BuildMode::Release] {
        assert_eq!(run_main_with_mode(&checked, mode).unwrap(), 42);
    }
    assert!(optimized_ir(&source).contains("div.error_bound"));
}

#[test]
fn range_facts_do_not_survive_rebinding_or_wrapping() {
    let source = "fn main() -> i64\n    let mut values: []i64 = [1, 2, 3]\n    let mut divisor = 2\n    let mut total = 0\n    for i in 0..3\n        total = total + values[0] / divisor\n        if i == 0\n            values = [24]\n            divisor = 3\n        else\n            divisor = divisor\n    let mut x = 9223372036854775807\n    x = x + 1\n    assert(x < 0)\n    return total\n";
    let checked = nerv_sema::check_modules(&[parse(source).unwrap()]).unwrap();
    for mode in [BuildMode::Debug, BuildMode::Release] {
        assert_eq!(run_main_with_mode(&checked, mode).unwrap(), 16);
    }
}

fn optimized_ir(source: &str) -> String {
    let checked = nerv_sema::check_modules(&[parse(source).unwrap()]).unwrap();
    let generated = compiler::generate(&checked, BuildMode::Release, false).unwrap();
    unsafe {
        let text = llvm_sys::core::LLVMPrintModuleToString(generated.module);
        let result = CStr::from_ptr(text).to_string_lossy().into_owned();
        llvm_sys::core::LLVMDisposeMessage(text);
        result
    }
}

#[test]
fn fixed_length_map_and_reduction_are_vectorizable() {
    let source = "fn main() -> i64\n    let input = nerv.arrays.new(4096, nerv.random(1) + 2)\n    let mut output = nerv.arrays.new(4096, 0)\n    for i in 0..4096\n        output[i] = input[i] * 3 + 7\n    let mut total = 0\n    for i in 0..4096\n        total = total + output[i]\n    return total\n";
    assert_eq!(run(source), 4096 * 13);
    let ir = optimized_ir(source);
    assert!(
        !ir.contains("array.in_bounds"),
        "proven accesses should not retain bounds comparisons"
    );
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    assert!(
        ir.contains("vector.body"),
        "expected target-costed vectorization: {ir}"
    );
}

#[test]
fn dot_stencil_and_division_loops_preserve_results() {
    let source = include_str!("../../nerv-cli/tests/fixtures/run/loop_optimizations/main.nerv");
    assert_eq!(run(source), 42);
    let ir = optimized_ir(source);
    #[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
    assert!(ir.contains("vector.body"));
}

#[test]
fn short_loops_do_not_pay_for_reciprocal_setup() {
    let source = "fn main() -> i64\n    let divisor = nerv.random(1) + 22\n    let mut total = 0\n    for i in 0..4\n        total = total + i % divisor\n    return total\n";
    assert_eq!(run(source), 6);
    assert!(!optimized_ir(source).contains("div.reciprocal"));
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

#[test]
fn repeated_invocations_release_all_region_allocations() {
    let source = include_str!("../../nerv-cli/tests/fixtures/run/region_lifetimes/main.nerv");
    let checked = nerv_sema::check_modules(&[parse(source).unwrap()]).unwrap();
    for _ in 0..20 {
        assert_eq!(super::run_main(&checked).unwrap(), 42);
        assert_eq!(nerv_runtime::active_region_allocations(), 0);
    }
}

#[test]
fn rejected_duplicate_modules_do_not_double_free_llvm_owned_memory() {
    let _lock = JIT_LOCK.lock().unwrap();
    let program = nerv_sema::check_modules(&[parse("fn main() -> i64 => 42\n").unwrap()]).unwrap();
    let mut jit = Jit::new().unwrap();
    jit.add_module(compiler::generate(&program, BuildMode::Release, false).unwrap())
        .unwrap();
    let duplicate = compiler::generate(&program, BuildMode::Release, false).unwrap();
    assert!(jit.add_module(duplicate).is_err());
    assert_eq!(jit.run(&symbol(program.main().unwrap()), true).unwrap(), 42);
}

#[test]
fn matches_compound_enum_payloads() {
    assert_eq!(run(include_str!("../../../example/enum-payloads.nerv")), 42);
}

#[test]
fn specializes_generic_functions_structs_and_enums() {
    assert_eq!(run(include_str!("../../../example/generics.nerv")), 42);
}

#[test]
fn runs_distinct_numeric_types_and_conversions() {
    assert_eq!(run(include_str!("../../../example/numeric-types.nerv")), 42);
}
