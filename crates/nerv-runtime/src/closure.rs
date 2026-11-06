use super::*;
use std::ffi::c_void;

#[repr(C)]
pub struct Closure {
    pub code: *const c_void,
    pub environment: *mut c_void,
    pub mutable_captures: i64,
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_closure_new(
    code: *const c_void,
    environment: *mut c_void,
    mutable_captures: i64,
) -> *mut Closure {
    allocate(Closure {
        code,
        environment,
        mutable_captures,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_closure_code(value: *const Closure) -> *const c_void {
    unsafe { (*value).code }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_closure_environment(value: *const Closure) -> *mut c_void {
    unsafe { (*value).environment }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_map_closure(
    array: *const NervArray,
    length: i64,
    callback: *const Closure,
) -> *mut NervArray {
    let callback = unsafe { &*callback };
    let call: unsafe extern "C" fn(*mut c_void, i64) -> i64 =
        unsafe { std::mem::transmute(callback.code) };
    if length < 0 {
        runtime_error("invalid array length");
    }
    if length > unsafe { (*array).header.length } {
        runtime_error("array length out of bounds");
    }
    let mut values = Vec::new();
    for index in 0..length {
        let value = unsafe { nerv_array_i64_get(array, index) };
        values.push(unsafe { call(callback.environment, value) });
    }
    super::array(values)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_i64_reduce_closure(
    array: *const NervArray,
    length: i64,
    initial: i64,
    callback: *const Closure,
) -> i64 {
    let callback = unsafe { &*callback };
    let call: unsafe extern "C" fn(*mut c_void, i64, i64) -> i64 =
        unsafe { std::mem::transmute(callback.code) };
    if length < 0 {
        runtime_error("invalid array length");
    }
    if length > unsafe { (*array).header.length } {
        runtime_error("array length out of bounds");
    }
    let mut value = initial;
    for index in 0..length {
        let next = unsafe { nerv_array_i64_get(array, index) };
        value = unsafe { call(callback.environment, value, next) };
    }
    value
}

pub(super) fn symbols() -> Vec<RuntimeSymbol> {
    vec![
        RuntimeSymbol {
            name: "nerv_closure_new",
            address: nerv_closure_new as *const (),
        },
        RuntimeSymbol {
            name: "nerv_closure_code",
            address: nerv_closure_code as *const (),
        },
        RuntimeSymbol {
            name: "nerv_closure_environment",
            address: nerv_closure_environment as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_map_closure",
            address: nerv_array_i64_map_closure as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_i64_reduce_closure",
            address: nerv_array_i64_reduce_closure as *const (),
        },
    ]
}
