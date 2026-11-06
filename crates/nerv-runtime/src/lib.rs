mod standard;

use std::{
    alloc::{Layout, alloc},
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    io::{self, Write},
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
    slice,
    sync::atomic::{AtomicU64, Ordering},
};

pub struct RuntimeSymbol {
    pub name: &'static str,
    pub address: *const (),
}

/// Opaque UTF-8 string handle. Its Rust layout is never accessed by generated code.
pub struct NervString(String);

/// Read-only C-layout prefix of an array handle (runtime ABI v2).
/// Neither the pointer nor the length changes during the allocation's lifetime.
#[repr(C)]
pub struct ArrayHeader {
    pub data: *mut i64,
    pub length: i64,
}

#[repr(C)]
pub struct NervArray {
    header: ArrayHeader,
    // Owns the backing allocation. Never resized or accessed through references
    // after publishing header.data; native operations use the cached raw pointer.
    _storage: Vec<i64>,
}

fn array(mut values: Vec<i64>) -> *mut NervArray {
    let header = ArrayHeader {
        data: values.as_mut_ptr(),
        length: values.len() as i64,
    };
    allocate(NervArray {
        header,
        _storage: values,
    })
}

#[repr(C)]
struct TypedArray {
    data: *mut u64,
    length: i64,
    storage: Vec<u64>,
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_array_new(length: i64, stride: i64) -> *mut std::ffi::c_void {
    let bytes = length
        .checked_mul(stride)
        .filter(|_| length >= 0 && stride >= 0)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or_else(|| runtime_error("array allocation failed"));
    let words = bytes
        .checked_add(7)
        .map(|n| n / 8)
        .unwrap_or_else(|| runtime_error("array allocation failed"));
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(words)
        .unwrap_or_else(|_| runtime_error("array allocation failed"));
    storage.resize(words, 0u64);
    allocate(TypedArray {
        data: storage.as_mut_ptr(),
        length,
        storage,
    })
    .cast()
}

struct NervVec(Vec<i64>);

struct NervMap(HashMap<String, i64>);

struct NervSet(HashSet<i64>);

struct NervDeque(VecDeque<i64>);

static RANDOM_STATE: AtomicU64 = AtomicU64::new(0x9e3779b97f4a7c15);

struct Allocation {
    pointer: *mut (),
    drop: unsafe fn(*mut ()),
}

thread_local! {
    static ALLOCATIONS: RefCell<Vec<Vec<Allocation>>> = const { RefCell::new(Vec::new()) };
}

pub struct Execution {
    depth: usize,
    // Regions and their raw pointers belong to the creating thread.
    _thread: PhantomData<Rc<()>>,
}

impl Execution {
    pub fn new() -> Self {
        let depth = ALLOCATIONS.with(|regions| {
            let mut regions = regions.borrow_mut();
            regions
                .try_reserve(1)
                .unwrap_or_else(|_| runtime_error("allocation failed"));
            regions.push(Vec::new());
            regions.len()
        });
        Self {
            depth,
            _thread: PhantomData,
        }
    }
}

impl Default for Execution {
    fn default() -> Self {
        Self::new()
    }
}

/// Diagnostic count of live allocations owned by regions on this thread.
pub fn active_region_allocations() -> usize {
    ALLOCATIONS.with(|regions| regions.borrow().iter().map(Vec::len).sum())
}

impl Drop for Execution {
    fn drop(&mut self) {
        let allocations = ALLOCATIONS.with(|regions| {
            let mut regions = regions.borrow_mut();
            assert_eq!(
                regions.len(),
                self.depth,
                "execution regions must be dropped in stack order"
            );
            regions.pop().unwrap()
        });
        for allocation in allocations.into_iter().rev() {
            unsafe { (allocation.drop)(allocation.pointer) };
        }
    }
}

unsafe fn drop_allocation<T>(pointer: *mut ()) {
    unsafe { drop(Box::from_raw(pointer.cast::<T>())) };
}

fn allocate<T: 'static>(value: T) -> *mut T {
    // Box::new aborts on OOM; allocate explicitly so the ABI reports exit status 1.
    let layout = Layout::new::<T>();
    let pointer = if layout.size() == 0 {
        NonNull::<T>::dangling().as_ptr()
    } else {
        unsafe { alloc(layout).cast::<T>() }
    };
    if pointer.is_null() {
        runtime_error("allocation failed");
    }
    unsafe { pointer.write(value) };
    ALLOCATIONS.with(|allocations| {
        if let Some(allocations) = allocations.borrow_mut().last_mut() {
            allocations
                .try_reserve(1)
                .unwrap_or_else(|_| runtime_error("allocation failed"));
            allocations.push(Allocation {
                pointer: pointer.cast(),
                drop: drop_allocation::<T>,
            });
        }
    });
    pointer
}

unsafe fn release<T>(pointer: *mut T) {
    if pointer.is_null() {
        return;
    }
    ALLOCATIONS.with(|allocations| {
        // An explicit native free may target an allocation in a parent region.
        for allocations in allocations.borrow_mut().iter_mut().rev() {
            if let Some(index) = allocations
                .iter()
                .position(|entry| entry.pointer == pointer.cast())
            {
                allocations.swap_remove(index);
                break;
            }
        }
    });
    unsafe { drop(Box::from_raw(pointer)) };
}

fn runtime_error(message: &str) -> ! {
    let _ = writeln!(io::stderr().lock(), "runtime error: {message}");
    std::process::exit(1)
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_fail(code: i64) {
    runtime_error(match code {
        1 => "integer division by zero",
        3 => "array index out of bounds",
        4 => "invalid array",
        5 => "float is outside the target integer range",
        _ => "assertion failed",
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_f64_to_i64(value: f64) -> i64 {
    if !value.is_finite()
        || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&value)
    {
        runtime_error("float is outside the i64 range");
    }
    value.trunc() as i64
}

/// Version of the C ABI used by generated host objects and the JIT.
pub const ABI_VERSION: u32 = 2;

/// Referenced by generated objects: linking ABI-v2 inline array code against an
/// older runtime must fail at link time, not silently misinterpret a Rust layout.
#[unsafe(no_mangle)]
pub extern "C" fn nerv_runtime_abi_v2() {}

/// Standalone entry adapters deliberately preserve the JIT's printed result and
/// process status: a returned integer is output, not an operating-system exit code.
#[unsafe(no_mangle)]
pub extern "C" fn nerv_start_i64(entry: extern "C" fn() -> i64) -> i32 {
    let _execution = Execution::new();
    let result = entry();
    nerv_println_i64(result);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_start_void(entry: extern "C" fn()) -> i32 {
    let _execution = Execution::new();
    entry();
    nerv_println_i64(0);
    0
}

pub fn symbols() -> Vec<RuntimeSymbol> {
    let mut symbols = vec![
        RuntimeSymbol {
            name: "nerv_array_new",
            address: nerv_array_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_runtime_abi_v2",
            address: nerv_runtime_abi_v2 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_start_i64",
            address: nerv_start_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_start_void",
            address: nerv_start_void as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fail",
            address: nerv_fail as *const (),
        },
        RuntimeSymbol {
            name: "nerv_f64_to_i64",
            address: nerv_f64_to_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_at",
            address: nerv_array_i64_at as *const (),
        },
        RuntimeSymbol {
            name: "nerv_print_u64",
            address: nerv_print_u64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_println_u64",
            address: nerv_println_u64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_print_i64",
            address: nerv_print_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_println_i64",
            address: nerv_println_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_print_f64",
            address: nerv_print_f64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_println_f64",
            address: nerv_println_f64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_print_bool",
            address: nerv_print_bool as *const (),
        },
        RuntimeSymbol {
            name: "nerv_println_bool",
            address: nerv_println_bool as *const (),
        },
        RuntimeSymbol {
            name: "nerv_print_string",
            address: nerv_print_string as *const (),
        },
        RuntimeSymbol {
            name: "nerv_println_string",
            address: nerv_println_string as *const (),
        },
        RuntimeSymbol {
            name: "nerv_string_new",
            address: nerv_string_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_string_len",
            address: nerv_string_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_string_concat",
            address: nerv_string_concat as *const (),
        },
        RuntimeSymbol {
            name: "nerv_string_free",
            address: nerv_string_free as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_new",
            address: nerv_array_i64_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_len",
            address: nerv_array_i64_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_data",
            address: nerv_array_i64_data as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_get",
            address: nerv_array_i64_get as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_set",
            address: nerv_array_i64_set as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_free",
            address: nerv_array_i64_free as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_map",
            address: nerv_array_i64_map as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_reduce",
            address: nerv_array_i64_reduce as *const (),
        },
        RuntimeSymbol {
            name: "nerv_sqrt",
            address: nerv_sqrt as *const (),
        },
        RuntimeSymbol {
            name: "nerv_pow",
            address: nerv_pow as *const (),
        },
        RuntimeSymbol {
            name: "nerv_abs_i64",
            address: nerv_abs_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_abs_f64",
            address: nerv_abs_f64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_random_i64",
            address: nerv_random_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_new",
            address: nerv_vec_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_push",
            address: nerv_vec_push as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_pop",
            address: nerv_vec_pop as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_get",
            address: nerv_vec_get as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_set",
            address: nerv_vec_set as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_len",
            address: nerv_vec_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_is_empty",
            address: nerv_vec_is_empty as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_insert",
            address: nerv_vec_insert as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_remove",
            address: nerv_vec_remove as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_clear",
            address: nerv_vec_clear as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_free",
            address: nerv_vec_free as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_contains",
            address: nerv_vec_contains as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_index_of",
            address: nerv_vec_index_of as *const (),
        },
        RuntimeSymbol {
            name: "nerv_vec_as_ptr",
            address: nerv_vec_as_ptr as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_new",
            address: nerv_map_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_insert",
            address: nerv_map_insert as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_get",
            address: nerv_map_get as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_remove",
            address: nerv_map_remove as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_contains_key",
            address: nerv_map_contains_key as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_len",
            address: nerv_map_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_map_free",
            address: nerv_map_free as *const (),
        },
        RuntimeSymbol {
            name: "nerv_set_new",
            address: nerv_set_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_set_insert",
            address: nerv_set_insert as *const (),
        },
        RuntimeSymbol {
            name: "nerv_set_contains",
            address: nerv_set_contains as *const (),
        },
        RuntimeSymbol {
            name: "nerv_set_remove",
            address: nerv_set_remove as *const (),
        },
        RuntimeSymbol {
            name: "nerv_set_len",
            address: nerv_set_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_set_free",
            address: nerv_set_free as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_new",
            address: nerv_deque_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_push_back",
            address: nerv_deque_push_back as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_push_front",
            address: nerv_deque_push_front as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_pop_back",
            address: nerv_deque_pop_back as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_pop_front",
            address: nerv_deque_pop_front as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_len",
            address: nerv_deque_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_deque_free",
            address: nerv_deque_free as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_is_empty",
            address: nerv_str_is_empty as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_contains",
            address: nerv_str_contains as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_starts_with",
            address: nerv_str_starts_with as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_ends_with",
            address: nerv_str_ends_with as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_to_upper",
            address: nerv_str_to_upper as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_to_lower",
            address: nerv_str_to_lower as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_trim",
            address: nerv_str_trim as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_replace",
            address: nerv_str_replace as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_repeat",
            address: nerv_str_repeat as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_slice",
            address: nerv_str_slice as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_find",
            address: nerv_str_find as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_char_count",
            address: nerv_str_char_count as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_char_at",
            address: nerv_str_char_at as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_split",
            address: nerv_str_split as *const (),
        },
        RuntimeSymbol {
            name: "nerv_str_split_once",
            address: nerv_str_split_once as *const (),
        },
    ];
    symbols.extend(standard::symbols());
    symbols
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_u64(value: u64) {
    output(format_args!("{value}"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_u64(value: u64) {
    output(format_args!("{value}\n"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_i64(value: i64) {
    output(format_args!("{value}"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_i64(value: i64) {
    output(format_args!("{value}\n"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_f64(value: f64) {
    output(format_args!("{value}"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_f64(value: f64) {
    output(format_args!("{value}\n"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_bool(value: bool) {
    output(format_args!("{value}"));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_bool(value: bool) {
    output(format_args!("{value}\n"));
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_print_string(value: *const NervString) {
    if let Some(value) = unsafe { value.as_ref() } {
        output(format_args!("{}", value.0));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_println_string(value: *const NervString) {
    if let Some(value) = unsafe { value.as_ref() } {
        output(format_args!("{}\n", value.0));
    } else {
        output(format_args!("\n"));
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_string_new(bytes: *const u8, length: i64) -> *mut NervString {
    let length = usize::try_from(length).unwrap_or_else(|_| runtime_error("invalid string length"));
    let bytes = if length == 0 {
        &[]
    } else {
        if bytes.is_null() {
            runtime_error("invalid string bytes");
        }
        unsafe { slice::from_raw_parts(bytes, length) }
    };
    let text = std::str::from_utf8(bytes).unwrap_or_else(|_| runtime_error("invalid UTF-8 string"));
    string(copy_string(text))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_string_len(value: *const NervString) -> i64 {
    unsafe { value.as_ref() }
        .map(|value| value.0.len() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_string_concat(
    left: *const NervString,
    right: *const NervString,
) -> *mut NervString {
    let left = unsafe { left.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let right = unsafe { right.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let mut result = copy_string(left);
    append(&mut result, right);
    string(result)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_string_free(value: *mut NervString) {
    unsafe { release(value) };
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_array_i64_new(length: i64, initial: i64) -> *mut NervArray {
    let length = usize::try_from(length).unwrap_or_else(|_| runtime_error("negative array length"));
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .unwrap_or_else(|_| runtime_error("array allocation failed"));
    values.resize(length, initial);
    array(values)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_len(array: *const NervArray) -> i64 {
    unsafe { array.as_ref() }
        .map(|array| array.header.length)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_data(array: *mut NervArray) -> *mut i64 {
    unsafe { array.as_ref() }
        .map(|array| array.header.data)
        .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_get(array: *const NervArray, index: i64) -> i64 {
    let array = unsafe { array.as_ref() }.unwrap_or_else(|| runtime_error("invalid array"));
    if index < 0 || index >= array.header.length {
        runtime_error("array index out of bounds");
    }
    unsafe { array.header.data.add(index as usize).read() }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_set(array: *mut NervArray, index: i64, value: i64) {
    unsafe { *nerv_array_i64_at(array, index) = value };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_at(array: *mut NervArray, index: i64) -> *mut i64 {
    let array = unsafe { array.as_ref() }.unwrap_or_else(|| runtime_error("invalid array"));
    if index < 0 || index >= array.header.length {
        runtime_error("array index out of bounds");
    }
    unsafe { array.header.data.add(index as usize) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_free(array: *mut NervArray) {
    unsafe { release(array) };
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_map(
    array: *const NervArray,
    length: i64,
    callback: extern "C" fn(i64) -> i64,
) -> *mut NervArray {
    let available = unsafe { array.as_ref() }
        .unwrap_or_else(|| runtime_error("invalid array"))
        .header
        .length as usize;
    let length = usize::try_from(length).unwrap_or_else(|_| runtime_error("invalid array length"));
    if length > available {
        runtime_error("array length out of bounds");
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(length)
        .unwrap_or_else(|_| runtime_error("array allocation failed"));
    for index in 0..length {
        // End the Rust borrow before invoking native code: a callback may mutate
        // a shared array handle, but may not free it during this operation.
        let value = unsafe { (*array).header.data.add(index).read() };
        values.push(callback(value));
    }
    self::array(values)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_reduce(
    array: *const NervArray,
    length: i64,
    initial: i64,
    callback: extern "C" fn(i64, i64) -> i64,
) -> i64 {
    let available = unsafe { array.as_ref() }
        .unwrap_or_else(|| runtime_error("invalid array"))
        .header
        .length as usize;
    let length = usize::try_from(length).unwrap_or_else(|_| runtime_error("invalid array length"));
    if length > available {
        runtime_error("array length out of bounds");
    }
    let mut result = initial;
    for index in 0..length {
        let next = unsafe { (*array).header.data.add(index).read() };
        result = callback(result, next);
    }
    result
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_sqrt(value: f64) -> f64 {
    value.sqrt()
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_pow(left: f64, right: f64) -> f64 {
    left.powf(right)
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_abs_i64(value: i64) -> i64 {
    value.saturating_abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_abs_f64(value: f64) -> f64 {
    value.abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_random_i64(maximum: i64) -> i64 {
    if maximum <= 0 {
        return 0;
    }
    let mut state = RANDOM_STATE.load(Ordering::Relaxed);
    loop {
        let mut next = state;
        next ^= next >> 12;
        next ^= next << 25;
        next ^= next >> 27;
        match RANDOM_STATE.compare_exchange_weak(state, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return ((next.wrapping_mul(2685821657736338717)) % maximum as u64) as i64,
            Err(current) => state = current,
        }
    }
}

unsafe fn handle_mut<T>(handle: i64) -> Option<&'static mut T> {
    unsafe { (handle as usize as *mut T).as_mut() }
}

unsafe fn handle_ref<T>(handle: i64) -> Option<&'static T> {
    unsafe { (handle as usize as *const T).as_ref() }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_vec_new() -> i64 {
    allocate(NervVec(Vec::new())) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_push(handle: i64, value: i64) {
    if let Some(values) = unsafe { handle_mut::<NervVec>(handle) } {
        values
            .0
            .try_reserve(1)
            .unwrap_or_else(|_| runtime_error("allocation failed"));
        values.0.push(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_pop(handle: i64) -> i64 {
    unsafe { handle_mut::<NervVec>(handle) }
        .and_then(|values| values.0.pop())
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_get(handle: i64, index: i64) -> i64 {
    let index = usize::try_from(index).ok();
    unsafe { handle_ref::<NervVec>(handle) }
        .and_then(|values| index.and_then(|index| values.0.get(index)))
        .copied()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_set(handle: i64, index: i64, value: i64) {
    let index = usize::try_from(index).ok();
    if let (Some(values), Some(index)) = (unsafe { handle_mut::<NervVec>(handle) }, index)
        && let Some(slot) = values.0.get_mut(index)
    {
        *slot = value;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_len(handle: i64) -> i64 {
    unsafe { handle_ref::<NervVec>(handle) }
        .map(|values| values.0.len() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_is_empty(handle: i64) -> i64 {
    unsafe { handle_ref::<NervVec>(handle) }
        .map(|values| values.0.is_empty() as i64)
        .unwrap_or(1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_insert(handle: i64, index: i64, value: i64) {
    let index = usize::try_from(index).ok();
    if let (Some(values), Some(index)) = (unsafe { handle_mut::<NervVec>(handle) }, index)
        && index <= values.0.len()
    {
        values
            .0
            .try_reserve(1)
            .unwrap_or_else(|_| runtime_error("allocation failed"));
        values.0.insert(index, value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_remove(handle: i64, index: i64) -> i64 {
    let index = usize::try_from(index).ok();
    match (unsafe { handle_mut::<NervVec>(handle) }, index) {
        (Some(values), Some(index)) if index < values.0.len() => values.0.remove(index),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_clear(handle: i64) {
    if let Some(values) = unsafe { handle_mut::<NervVec>(handle) } {
        values.0.clear();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_free(handle: i64) {
    if handle != 0 {
        unsafe { release(handle as usize as *mut NervVec) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_contains(handle: i64, value: i64) -> i64 {
    unsafe { handle_ref::<NervVec>(handle) }
        .map(|values| values.0.contains(&value) as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_index_of(handle: i64, value: i64) -> i64 {
    unsafe { handle_ref::<NervVec>(handle) }
        .and_then(|values| values.0.iter().position(|item| *item == value))
        .map(|index| index as i64)
        .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_as_ptr(handle: i64) -> i64 {
    unsafe { handle_mut::<NervVec>(handle) }
        .map(|values| values.0.as_mut_ptr() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_map_new() -> i64 {
    allocate(NervMap(HashMap::new())) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_insert(handle: i64, key: *const NervString, value: i64) -> i64 {
    let Some(key) = (unsafe { key.as_ref() }) else {
        return -1;
    };
    let Some(map) = (unsafe { handle_mut::<NervMap>(handle) }) else {
        return -1;
    };
    map.0
        .try_reserve(1)
        .unwrap_or_else(|_| runtime_error("allocation failed"));
    map.0.insert(copy_string(&key.0), value);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_get(handle: i64, key: *const NervString) -> i64 {
    let Some(key) = (unsafe { key.as_ref() }) else {
        return 0;
    };
    unsafe { handle_ref::<NervMap>(handle) }
        .and_then(|map| map.0.get(&key.0))
        .copied()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_remove(handle: i64, key: *const NervString) -> i64 {
    let Some(key) = (unsafe { key.as_ref() }) else {
        return -1;
    };
    unsafe { handle_mut::<NervMap>(handle) }
        .and_then(|map| map.0.remove(&key.0))
        .map(|_| 0)
        .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_contains_key(handle: i64, key: *const NervString) -> i64 {
    let Some(key) = (unsafe { key.as_ref() }) else {
        return 0;
    };
    unsafe { handle_ref::<NervMap>(handle) }
        .map(|map| map.0.contains_key(&key.0) as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_len(handle: i64) -> i64 {
    unsafe { handle_ref::<NervMap>(handle) }
        .map(|map| map.0.len() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_free(handle: i64) {
    if handle != 0 {
        unsafe { release(handle as usize as *mut NervMap) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_set_new() -> i64 {
    allocate(NervSet(HashSet::new())) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_set_insert(handle: i64, value: i64) -> i64 {
    let Some(set) = (unsafe { handle_mut::<NervSet>(handle) }) else {
        return -1;
    };
    set.0
        .try_reserve(1)
        .unwrap_or_else(|_| runtime_error("allocation failed"));
    set.0.insert(value);
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_set_contains(handle: i64, value: i64) -> i64 {
    unsafe { handle_ref::<NervSet>(handle) }
        .map(|set| set.0.contains(&value) as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_set_remove(handle: i64, value: i64) -> i64 {
    unsafe { handle_mut::<NervSet>(handle) }
        .map(|set| if set.0.remove(&value) { 0 } else { -1 })
        .unwrap_or(-1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_set_len(handle: i64) -> i64 {
    unsafe { handle_ref::<NervSet>(handle) }
        .map(|set| set.0.len() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_set_free(handle: i64) {
    if handle != 0 {
        unsafe { release(handle as usize as *mut NervSet) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_deque_new() -> i64 {
    allocate(NervDeque(VecDeque::new())) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_push_back(handle: i64, value: i64) {
    if let Some(deque) = unsafe { handle_mut::<NervDeque>(handle) } {
        deque
            .0
            .try_reserve(1)
            .unwrap_or_else(|_| runtime_error("allocation failed"));
        deque.0.push_back(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_push_front(handle: i64, value: i64) {
    if let Some(deque) = unsafe { handle_mut::<NervDeque>(handle) } {
        deque
            .0
            .try_reserve(1)
            .unwrap_or_else(|_| runtime_error("allocation failed"));
        deque.0.push_front(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_pop_back(handle: i64) -> i64 {
    unsafe { handle_mut::<NervDeque>(handle) }
        .and_then(|deque| deque.0.pop_back())
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_pop_front(handle: i64) -> i64 {
    unsafe { handle_mut::<NervDeque>(handle) }
        .and_then(|deque| deque.0.pop_front())
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_len(handle: i64) -> i64 {
    unsafe { handle_ref::<NervDeque>(handle) }
        .map(|deque| deque.0.len() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_free(handle: i64) {
    if handle != 0 {
        unsafe { release(handle as usize as *mut NervDeque) };
    }
}

fn string(value: String) -> *mut NervString {
    allocate(NervString(value))
}

fn output(args: std::fmt::Arguments<'_>) {
    let mut stdout = io::stdout().lock();
    if stdout
        .write_fmt(args)
        .and_then(|()| stdout.flush())
        .is_err()
    {
        runtime_error("stdout write failed");
    }
}

fn append(result: &mut String, text: &str) {
    result
        .try_reserve(text.len())
        .unwrap_or_else(|_| runtime_error("string allocation failed"));
    result.push_str(text);
}

fn copy_string(text: &str) -> String {
    let mut result = String::new();
    append(&mut result, text);
    result
}

fn change_case(text: &str, upper: bool) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        // Unicode case mappings can expand one scalar into several scalars.
        if upper {
            for ch in ch.to_uppercase() {
                append(&mut result, ch.encode_utf8(&mut [0; 4]));
            }
        } else {
            for ch in ch.to_lowercase() {
                append(&mut result, ch.encode_utf8(&mut [0; 4]));
            }
        }
    }
    result
}

fn replace_string(value: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return copy_string(value);
    }
    let mut result = String::new();
    let mut start = 0;
    for (index, matched) in value.match_indices(from) {
        append(&mut result, &value[start..index]);
        append(&mut result, to);
        start = index + matched.len();
    }
    append(&mut result, &value[start..]);
    result
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_is_empty(value: *const NervString) -> i64 {
    unsafe { value.as_ref() }
        .map(|value| value.0.is_empty() as i64)
        .unwrap_or(1)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_contains(
    value: *const NervString,
    needle: *const NervString,
) -> i64 {
    match (unsafe { value.as_ref() }, unsafe { needle.as_ref() }) {
        (Some(value), Some(needle)) => value.0.contains(&needle.0) as i64,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_starts_with(
    value: *const NervString,
    prefix: *const NervString,
) -> i64 {
    match (unsafe { value.as_ref() }, unsafe { prefix.as_ref() }) {
        (Some(value), Some(prefix)) => value.0.starts_with(&prefix.0) as i64,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_ends_with(
    value: *const NervString,
    suffix: *const NervString,
) -> i64 {
    match (unsafe { value.as_ref() }, unsafe { suffix.as_ref() }) {
        (Some(value), Some(suffix)) => value.0.ends_with(&suffix.0) as i64,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_to_upper(value: *const NervString) -> *mut NervString {
    string(
        unsafe { value.as_ref() }
            .map(|value| change_case(&value.0, true))
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_to_lower(value: *const NervString) -> *mut NervString {
    string(
        unsafe { value.as_ref() }
            .map(|value| change_case(&value.0, false))
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_trim(value: *const NervString) -> *mut NervString {
    string(
        unsafe { value.as_ref() }
            .map(|value| copy_string(value.0.trim()))
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_replace(
    value: *const NervString,
    from: *const NervString,
    to: *const NervString,
) -> *mut NervString {
    let value = unsafe { value.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let from = unsafe { from.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let to = unsafe { to.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    string(replace_string(value, from, to))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_repeat(value: *const NervString, count: i64) -> *mut NervString {
    let value = unsafe { value.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let count = usize::try_from(count).unwrap_or(0);
    let length = value
        .len()
        .checked_mul(count)
        .unwrap_or_else(|| runtime_error("string allocation failed"));
    let mut result = String::new();
    result
        .try_reserve_exact(length)
        .unwrap_or_else(|_| runtime_error("string allocation failed"));
    if !value.is_empty() {
        for _ in 0..count {
            result.push_str(value);
        }
    }
    string(result)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_slice(
    value: *const NervString,
    start: i64,
    end: i64,
) -> *mut NervString {
    let value = unsafe { value.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let start = usize::try_from(start).unwrap_or(0).min(value.len());
    let end = usize::try_from(end).unwrap_or(0).min(value.len());
    if start >= end || !value.is_char_boundary(start) || !value.is_char_boundary(end) {
        return string(String::new());
    }
    string(copy_string(&value[start..end]))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_find(value: *const NervString, needle: *const NervString) -> i64 {
    match (unsafe { value.as_ref() }, unsafe { needle.as_ref() }) {
        (Some(value), Some(needle)) => value
            .0
            .find(&needle.0)
            .map(|index| index as i64)
            .unwrap_or(-1),
        _ => -1,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_char_count(value: *const NervString) -> i64 {
    unsafe { value.as_ref() }
        .map(|value| value.0.chars().count() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_char_at(value: *const NervString, index: i64) -> i64 {
    let index = usize::try_from(index).ok();
    unsafe { value.as_ref() }
        .and_then(|value| index.and_then(|index| value.0.chars().nth(index)))
        .map(|value| value as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_split(
    value: *const NervString,
    separator: *const NervString,
) -> *mut NervString {
    let value = unsafe { value.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let separator = unsafe { separator.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    string(replace_string(value, separator, "\n"))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_split_once(
    value: *const NervString,
    separator: *const NervString,
) -> *mut NervString {
    let value = unsafe { value.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    let separator = unsafe { separator.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    if separator.is_empty() {
        return string(copy_string(value));
    }
    string(
        value
            .split_once(separator)
            .map(|(left, right)| {
                let mut result = copy_string(left);
                append(&mut result, "\n");
                append(&mut result, right);
                result
            })
            .unwrap_or_else(|| copy_string(value)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_arrays_preserve_pointer_values_and_release_storage() {
        let before = active_region_allocations();
        {
            let _execution = Execution::new();
            let value = string("hello".to_owned());
            let array = nerv_array_new(2, std::mem::size_of::<*mut NervString>() as i64)
                .cast::<TypedArray>();
            unsafe {
                let data = (*array).data.cast::<*mut NervString>();
                data.write(value);
                data.add(1).write(value);
                assert_eq!((*data.read()).0, "hello");
                assert_eq!(standard::nerv_array_length(array.cast()), 2);
            }
            assert_eq!(active_region_allocations(), before + 2);
        }
        assert_eq!(active_region_allocations(), before);
    }

    #[test]
    fn array_prefix_matches_abi_and_remains_stable_through_element_writes() {
        assert_eq!(std::mem::offset_of!(NervArray, header), 0);
        assert_eq!(std::mem::offset_of!(ArrayHeader, data), 0);
        assert_eq!(
            std::mem::offset_of!(ArrayHeader, length),
            std::mem::size_of::<*mut i64>()
        );
        let _execution = Execution::new();
        unsafe {
            let array = nerv_array_i64_new(2, 1);
            let prefix = array.cast::<ArrayHeader>();
            let data = (*prefix).data;
            assert_eq!((*prefix).length, 2);
            data.add(1).write(42);
            assert_eq!(nerv_array_i64_get(array, 1), 42);
            nerv_array_i64_set(array, 0, 23);
            assert_eq!(data.read(), 23);
            assert_eq!((*prefix).data, data);
            assert_eq!((*prefix).length, 2);
            let empty = nerv_array_i64_new(0, 0);
            assert_eq!((*empty.cast::<ArrayHeader>()).length, 0);
        }
    }
    use std::{cell::Cell, rc::Rc};

    struct CountDrop(Rc<Cell<usize>>);

    impl Drop for CountDrop {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn execution_owns_allocations_until_it_finishes() {
        let dropped = Rc::new(Cell::new(0));
        {
            let _execution = Execution::new();
            let pointer = allocate(CountDrop(dropped.clone()));
            let alias = pointer;
            assert_eq!(pointer, alias);
            assert_eq!(dropped.get(), 0);
            let text = string("é🙂".to_owned());
            let array = nerv_array_i64_new(2, 42);
            unsafe {
                assert_eq!(nerv_string_len(text), 6);
                assert_eq!(nerv_array_i64_get(array, 1), 42);
            }
            ALLOCATIONS
                .with(|allocations| assert_eq!(allocations.borrow().last().unwrap().len(), 3));
        }
        assert_eq!(dropped.get(), 1);
        ALLOCATIONS.with(|allocations| assert!(allocations.borrow().is_empty()));
    }

    #[test]
    fn explicit_release_is_not_dropped_again_at_execution_end() {
        let dropped = Rc::new(Cell::new(0));
        {
            let _execution = Execution::new();
            let value = allocate(CountDrop(dropped.clone()));
            unsafe { release(value) };
            assert_eq!(dropped.get(), 1);
        }
        assert_eq!(dropped.get(), 1);
    }

    #[test]
    fn nested_execution_restores_its_parent_region() {
        let dropped = Rc::new(Cell::new(0));
        {
            let _outer = Execution::new();
            allocate(CountDrop(dropped.clone()));
            {
                let _inner = Execution::new();
                allocate(CountDrop(dropped.clone()));
            }
            assert_eq!(dropped.get(), 1);
            ALLOCATIONS
                .with(|allocations| assert_eq!(allocations.borrow().last().unwrap().len(), 1));
        }
        assert_eq!(dropped.get(), 2);
    }

    #[test]
    fn stores_array_values() {
        let array = nerv_array_i64_new(2, 3);
        unsafe {
            nerv_array_i64_set(array, 1, 7);
            assert_eq!(nerv_array_i64_len(array), 2);
            assert_eq!(nerv_array_i64_get(array, 1), 7);
            nerv_array_i64_free(array);
        }
    }

    #[test]
    fn concatenates_strings() {
        unsafe {
            let left = nerv_string_new(c"left".as_ptr().cast(), 4);
            let right = nerv_string_new(c"right".as_ptr().cast(), 5);
            let value = nerv_string_concat(left, right);
            assert_eq!(nerv_string_len(value), 9);
            nerv_string_free(left);
            nerv_string_free(right);
            nerv_string_free(value);
        }
    }

    #[test]
    fn supports_collections() {
        unsafe {
            let values = nerv_vec_new();
            nerv_vec_push(values, 3);
            nerv_vec_insert(values, 1, 7);
            assert_eq!(nerv_vec_get(values, 1), 7);
            assert_eq!(nerv_vec_index_of(values, 3), 0);
            nerv_vec_free(values);

            let map = nerv_map_new();
            let key = nerv_string_new(c"key".as_ptr().cast(), 3);
            assert_eq!(nerv_map_insert(map, key, 9), 0);
            assert_eq!(nerv_map_get(map, key), 9);
            nerv_map_free(map);
            nerv_string_free(key);

            let deque = nerv_deque_new();
            nerv_deque_push_back(deque, 2);
            nerv_deque_push_front(deque, 1);
            assert_eq!(nerv_deque_pop_front(deque), 1);
            nerv_deque_free(deque);
        }
    }

    #[test]
    fn supports_string_operations() {
        unsafe {
            let value = nerv_string_new(c"hello, world".as_ptr().cast(), 12);
            let needle = nerv_string_new(c"world".as_ptr().cast(), 5);
            assert_eq!(nerv_str_contains(value, needle), 1);
            assert_eq!(nerv_str_find(value, needle), 7);
            assert_eq!(nerv_str_char_at(value, 1), 'e' as i64);
            let upper = nerv_str_to_upper(value);
            assert_eq!(nerv_string_len(upper), 12);
            nerv_string_free(value);
            nerv_string_free(needle);
            nerv_string_free(upper);
        }
    }

    #[test]
    fn releasing_a_parent_allocation_from_a_nested_region_does_not_double_free() {
        let dropped = Rc::new(Cell::new(0));
        let outer = Execution::new();
        let pointer = allocate(CountDrop(dropped.clone()));
        {
            let _inner = Execution::new();
            unsafe { release(pointer) };
            assert_eq!(active_region_allocations(), 0);
        }
        drop(outer);
        assert_eq!(dropped.get(), 1);
    }

    #[test]
    fn regions_release_shared_handles_and_all_string_operations() {
        for _ in 0..20 {
            let execution = Execution::new();
            unsafe {
                let text = nerv_string_new("é🙂".as_ptr(), 6);
                let alias = text;
                let upper = nerv_str_to_upper(text);
                let lower = nerv_str_to_lower(upper);
                let repeated = nerv_str_repeat(lower, 3);
                let slice = nerv_str_slice(repeated, 0, 2);
                let replaced = nerv_str_replace(repeated, text, slice);
                let split = nerv_str_split(repeated, text);
                let once = nerv_str_split_once(repeated, text);
                let _trimmed = nerv_str_trim(text);
                assert_eq!(nerv_string_len(alias), 6);
                assert_eq!(nerv_string_len(replaced), 6);
                assert_eq!(nerv_string_len(split), 3);
                assert_eq!(nerv_string_len(once), 13);
                assert_eq!(active_region_allocations(), 9);
            }
            drop(execution);
            assert_eq!(active_region_allocations(), 0);
        }
    }

    #[test]
    fn callbacks_can_mutate_shared_array_storage_without_overlapping_rust_borrows() {
        thread_local! { static INPUT: Cell<*mut NervArray> = const { Cell::new(std::ptr::null_mut()) }; }
        extern "C" fn change(value: i64) -> i64 {
            INPUT.with(|input| unsafe { nerv_array_i64_set(input.get(), 1, 40) });
            value
        }
        extern "C" fn add(total: i64, value: i64) -> i64 {
            INPUT.with(|input| unsafe { nerv_array_i64_set(input.get(), 1, 41) });
            total + value
        }
        let _execution = Execution::new();
        let input = nerv_array_i64_new(2, 1);
        INPUT.with(|slot| slot.set(input));
        unsafe {
            let mapped = nerv_array_i64_map(input, 2, change);
            assert_eq!(nerv_array_i64_get(mapped, 1), 40);
            assert_eq!(nerv_array_i64_reduce(input, 2, 0, add), 42);
        }
        INPUT.with(|slot| slot.set(std::ptr::null_mut()));
    }

    #[test]
    fn array_get_respects_shared_native_references() {
        let _execution = Execution::new();
        let array = nerv_array_i64_new(2, 3);
        unsafe {
            let shared = &*array;
            assert_eq!(nerv_array_i64_get(shared, 0), 3);
            assert_eq!(shared.header.data.add(1).read(), 3);
            assert_eq!(nerv_array_i64_get(shared, 1), 3);
        }
    }

    #[test]
    fn abi_symbols_are_unique() {
        let mut names = HashSet::new();
        for symbol in symbols() {
            assert!(names.insert(symbol.name));
            assert!(!symbol.address.is_null());
        }
        assert_eq!(ABI_VERSION, 2);
    }

    #[cfg(not(miri))]
    #[test]
    fn runtime_failure_probe() {
        let Ok(case) = std::env::var("NERV_RUNTIME_FAILURE_TEST") else {
            return;
        };
        let _execution = Execution::new();
        match case.as_str() {
            "array" => {
                nerv_array_i64_new(i64::MAX, 0);
            }
            "string" => unsafe {
                let text = nerv_string_new(c"xx".as_ptr().cast(), 2);
                nerv_str_repeat(text, i64::MAX);
            },
            "utf8" => unsafe {
                nerv_string_new([255u8].as_ptr(), 1);
            },
            _ => panic!("unknown failure probe"),
        }
        panic!("runtime failure unexpectedly returned");
    }

    #[cfg(not(miri))]
    #[test]
    fn allocation_and_utf8_failures_exit_cleanly_without_unwinding_across_the_abi() {
        for (case, message) in [
            ("array", "array allocation failed"),
            ("string", "string allocation failed"),
            ("utf8", "invalid UTF-8 string"),
        ] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::runtime_failure_probe", "--nocapture"])
                .env("NERV_RUNTIME_FAILURE_TEST", case)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(1), "{case}: {output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(message),
                "{case}: {output:?}"
            );
            assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
        }
    }
}
