use super::*;
use std::{
    env, fs,
    io::Read,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub(super) fn strings(values: Vec<String>) -> *mut std::ffi::c_void {
    let length = values.len();
    let array = nerv_array_new(length as i64, std::mem::size_of::<*mut NervString>() as i64);
    let data = unsafe { (*array.cast::<TypedArray>()).data.cast::<*mut NervString>() };
    for (index, value) in values.into_iter().enumerate() {
        unsafe {
            data.add(index).write(string(value));
        }
    }
    array
}

pub(super) unsafe fn text<'a>(value: *const NervString) -> &'a str {
    unsafe { value.as_ref() }
        .map(|s| s.0.as_str())
        .unwrap_or("")
}

fn response(result: io::Result<String>) -> *mut std::ffi::c_void {
    match result {
        Ok(value) => strings(vec![value, String::new(), String::new()]),
        Err(error) => strings(vec![
            String::new(),
            error.to_string(),
            foundation::error_kind(&error).into(),
        ]),
    }
}

fn status(result: io::Result<()>) -> *mut NervString {
    string(result.err().map(|e| e.to_string()).unwrap_or_default())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_array_length(array: *const ArrayHeader) -> i64 {
    unsafe { array.as_ref() }.map(|a| a.length).unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_io_read_text(path: *const NervString) -> *mut std::ffi::c_void {
    response(fs::read_to_string(unsafe { text(path) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_io_write_text(
    path: *const NervString,
    contents: *const NervString,
) -> *mut NervString {
    status(fs::write(unsafe { text(path) }, unsafe { text(contents) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_io_create_directory(path: *const NervString) -> *mut NervString {
    status(fs::create_dir_all(unsafe { text(path) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_io_list_directory(path: *const NervString) -> *mut std::ffi::c_void {
    let result = fs::read_dir(unsafe { text(path) }).and_then(|entries| {
        entries
            .map(|entry| entry.map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect::<io::Result<Vec<_>>>()
    });
    match result {
        Ok(mut entries) => {
            entries.sort();
            entries.insert(0, String::new());
            strings(entries)
        }
        Err(error) => strings(vec![
            error.to_string(),
            foundation::error_kind(&error).into(),
        ]),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_io_read_stdin() -> *mut std::ffi::c_void {
    let mut input = String::new();
    response(io::stdin().read_to_string(&mut input).map(|_| input))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_io_environment(name: *const NervString) -> *mut std::ffi::c_void {
    match env::var(unsafe { text(name) }) {
        Ok(value) => strings(vec![value, String::new()]),
        Err(error) => strings(vec![String::new(), error.to_string()]),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_io_arguments() -> *mut std::ffi::c_void {
    strings(
        env::args_os()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_io_stderr(message: *const NervString) {
    let mut stderr = io::stderr().lock();
    if stderr
        .write_all(unsafe { text(message) }.as_bytes())
        .is_err()
        || stderr.flush().is_err()
    {
        runtime_error("could not write standard error");
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_io_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_io_sleep_ms(duration: i64) {
    if duration < 0 {
        runtime_error("negative duration");
    }
    std::thread::sleep(Duration::from_millis(duration as u64));
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_io_monotonic_ms() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn symbols() -> Vec<RuntimeSymbol> {
    vec![
        RuntimeSymbol {
            name: "nerv_io_monotonic_ms",
            address: nerv_io_monotonic_ms as *const (),
        },
        RuntimeSymbol {
            name: "nerv_array_length",
            address: nerv_array_length as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_read_text",
            address: nerv_io_read_text as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_write_text",
            address: nerv_io_write_text as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_create_directory",
            address: nerv_io_create_directory as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_list_directory",
            address: nerv_io_list_directory as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_read_stdin",
            address: nerv_io_read_stdin as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_environment",
            address: nerv_io_environment as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_arguments",
            address: nerv_io_arguments as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_stderr",
            address: nerv_io_stderr as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_now_ms",
            address: nerv_io_now_ms as *const (),
        },
        RuntimeSymbol {
            name: "nerv_io_sleep_ms",
            address: nerv_io_sleep_ms as *const (),
        },
    ]
}
