use std::{
    collections::{HashMap, HashSet, VecDeque},
    io::{self, Write},
    slice,
    sync::atomic::{AtomicU64, Ordering},
};

pub struct RuntimeSymbol {
    pub name: &'static str,
    pub address: *const (),
}

pub struct NervString(String);

pub struct NervArray(Vec<i64>);

struct NervVec(Vec<i64>);

struct NervMap(HashMap<String, i64>);

struct NervSet(HashSet<i64>);

struct NervDeque(VecDeque<i64>);

static RANDOM_STATE: AtomicU64 = AtomicU64::new(0x9e3779b97f4a7c15);

pub fn symbols() -> Vec<RuntimeSymbol> {
    vec![
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
    ]
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_i64(value: i64) {
    print!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_i64(value: i64) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_f64(value: f64) {
    print!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_f64(value: f64) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_print_bool(value: bool) {
    print!("{value}");
    let _ = io::stdout().flush();
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_println_bool(value: bool) {
    println!("{value}");
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_print_string(value: *const NervString) {
    if let Some(value) = unsafe { value.as_ref() } {
        print!("{}", value.0);
        let _ = io::stdout().flush();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_println_string(value: *const NervString) {
    if let Some(value) = unsafe { value.as_ref() } {
        println!("{}", value.0);
    } else {
        println!();
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_string_new(bytes: *const u8, length: i64) -> *mut NervString {
    let length = usize::try_from(length).unwrap_or(0);
    let bytes = if bytes.is_null() {
        &[]
    } else {
        unsafe { slice::from_raw_parts(bytes, length) }
    };
    Box::into_raw(Box::new(NervString(
        String::from_utf8_lossy(bytes).into_owned(),
    )))
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
    Box::into_raw(Box::new(NervString(format!("{left}{right}"))))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_string_free(value: *mut NervString) {
    if !value.is_null() {
        unsafe { drop(Box::from_raw(value)) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_array_i64_new(length: i64, initial: i64) -> *mut NervArray {
    let length = usize::try_from(length).unwrap_or(0);
    Box::into_raw(Box::new(NervArray(vec![initial; length])))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_len(array: *const NervArray) -> i64 {
    unsafe { array.as_ref() }
        .map(|array| array.0.len() as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_data(array: *mut NervArray) -> *mut i64 {
    unsafe { array.as_mut() }
        .map(|array| array.0.as_mut_ptr())
        .unwrap_or(std::ptr::null_mut())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_get(array: *const NervArray, index: i64) -> i64 {
    let index = usize::try_from(index).ok();
    unsafe { array.as_ref() }
        .and_then(|array| index.and_then(|index| array.0.get(index)))
        .copied()
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_set(array: *mut NervArray, index: i64, value: i64) {
    let index = usize::try_from(index).ok();
    if let (Some(array), Some(index)) = (unsafe { array.as_mut() }, index)
        && let Some(slot) = array.0.get_mut(index)
    {
        *slot = value;
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_free(array: *mut NervArray) {
    if !array.is_null() {
        unsafe { drop(Box::from_raw(array)) };
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_map(
    array: *const i64,
    length: i64,
    callback: extern "C" fn(i64) -> i64,
) -> *mut i64 {
    let length = usize::try_from(length).unwrap_or(0);
    if array.is_null() || length == 0 {
        return std::ptr::null_mut();
    }
    let values = unsafe { slice::from_raw_parts(array, length) }
        .iter()
        .map(|value| callback(*value))
        .collect::<Vec<_>>();
    values.leak().as_mut_ptr()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_reduce(
    array: *const i64,
    length: i64,
    initial: i64,
    callback: extern "C" fn(i64, i64) -> i64,
) -> i64 {
    let length = usize::try_from(length).unwrap_or(0);
    if array.is_null() || length == 0 {
        return initial;
    }
    unsafe { slice::from_raw_parts(array, length) }
        .iter()
        .fold(initial, |value, next| callback(value, *next))
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
    Box::into_raw(Box::new(NervVec(Vec::new()))) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_vec_push(handle: i64, value: i64) {
    if let Some(values) = unsafe { handle_mut::<NervVec>(handle) } {
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
        unsafe { drop(Box::from_raw(handle as usize as *mut NervVec)) };
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
    Box::into_raw(Box::new(NervMap(HashMap::new()))) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_map_insert(handle: i64, key: *const NervString, value: i64) -> i64 {
    let Some(key) = (unsafe { key.as_ref() }) else {
        return -1;
    };
    let Some(map) = (unsafe { handle_mut::<NervMap>(handle) }) else {
        return -1;
    };
    map.0.insert(key.0.clone(), value);
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
        unsafe { drop(Box::from_raw(handle as usize as *mut NervMap)) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_set_new() -> i64 {
    Box::into_raw(Box::new(NervSet(HashSet::new()))) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_set_insert(handle: i64, value: i64) -> i64 {
    let Some(set) = (unsafe { handle_mut::<NervSet>(handle) }) else {
        return -1;
    };
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
        unsafe { drop(Box::from_raw(handle as usize as *mut NervSet)) };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_deque_new() -> i64 {
    Box::into_raw(Box::new(NervDeque(VecDeque::new()))) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_push_back(handle: i64, value: i64) {
    if let Some(deque) = unsafe { handle_mut::<NervDeque>(handle) } {
        deque.0.push_back(value);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_deque_push_front(handle: i64, value: i64) {
    if let Some(deque) = unsafe { handle_mut::<NervDeque>(handle) } {
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
        unsafe { drop(Box::from_raw(handle as usize as *mut NervDeque)) };
    }
}

fn string(value: String) -> *mut NervString {
    Box::into_raw(Box::new(NervString(value)))
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
            .map(|value| value.0.to_uppercase())
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_to_lower(value: *const NervString) -> *mut NervString {
    string(
        unsafe { value.as_ref() }
            .map(|value| value.0.to_lowercase())
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_trim(value: *const NervString) -> *mut NervString {
    string(
        unsafe { value.as_ref() }
            .map(|value| value.0.trim().to_owned())
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
    if from.is_empty() {
        return string(value.to_owned());
    }
    string(value.replace(from, to))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_str_repeat(value: *const NervString, count: i64) -> *mut NervString {
    let value = unsafe { value.as_ref() }
        .map(|value| value.0.as_str())
        .unwrap_or("");
    string(value.repeat(usize::try_from(count).unwrap_or(0)))
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
    string(value[start..end].to_owned())
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
    if separator.is_empty() {
        return string(value.to_owned());
    }
    string(value.split(separator).collect::<Vec<_>>().join("\n"))
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
        return string(value.to_owned());
    }
    string(
        value
            .split_once(separator)
            .map(|(left, right)| format!("{left}\n{right}"))
            .unwrap_or_else(|| value.to_owned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
