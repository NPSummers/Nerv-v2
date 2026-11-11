#[cfg(not(target_pointer_width = "64"))]
compile_error!("Nerv native memory requires a 64-bit target");
use super::{NervArray, NervString, runtime_error, string};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryW(name: *const u16) -> *mut c_void;
    fn GetProcAddress(library: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(library: *mut c_void) -> i32;
}
#[cfg(unix)]
unsafe extern "C" {
    fn dlopen(name: *const c_char, mode: i32) -> *mut c_void;
    fn dlsym(library: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlclose(library: *mut c_void) -> i32;
}
fn text(value: *const NervString) -> &'static str {
    unsafe { super::standard::text(value) }
}
fn cstring(value: &str) -> CString {
    CString::new(value).unwrap_or_else(|_| runtime_error("invalid native argument"))
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_open(name: *const NervString) -> i64 {
    #[cfg(windows)]
    {
        let wide = text(name).encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        unsafe { LoadLibraryW(wide.as_ptr()) as i64 }
    }
    #[cfg(unix)]
    unsafe {
        dlopen(cstring(text(name)).as_ptr(), 2) as i64
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_symbol(library: i64, name: *const NervString) -> i64 {
    let name = cstring(text(name));
    #[cfg(windows)]
    unsafe {
        GetProcAddress(library as *mut c_void, name.as_ptr()) as i64
    }
    #[cfg(unix)]
    unsafe {
        dlsym(library as *mut c_void, name.as_ptr()) as i64
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_close(library: i64) {
    if library == 0 {
        return;
    }
    #[cfg(windows)]
    unsafe {
        FreeLibrary(library as *mut c_void);
    }
    #[cfg(unix)]
    unsafe {
        dlclose(library as *mut c_void);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_alloc(size: i64) -> i64 {
    if size < 0 || size > isize::MAX as i64 - 8 {
        runtime_error("invalid native argument");
    }
    let layout = Layout::from_size_align(size as usize + 8, 8).unwrap();
    unsafe {
        let base = alloc_zeroed(layout);
        if base.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        (base as *mut usize).write(size as usize + 8);
        base.add(8) as i64
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_free(address: i64) {
    if address == 0 {
        return;
    }
    unsafe {
        let base = (address as *mut u8).sub(8);
        let size = (base as *const usize).read();
        dealloc(base, Layout::from_size_align(size, 8).unwrap());
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_cstring(value: *const NervString) -> i64 {
    let bytes = cstring(text(value));
    let bytes = bytes.as_bytes_with_nul();
    let address = nerv_ffi_alloc(bytes.len() as i64);
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
    }
    address
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_text(address: i64) -> *mut NervString {
    if address == 0 {
        return string(String::new());
    }
    string(
        unsafe { CStr::from_ptr(address as *const c_char) }
            .to_string_lossy()
            .into_owned(),
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_load(address: i64) -> i64 {
    unsafe { (address as *const i64).read_unaligned() }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_store(address: i64, value: i64) {
    unsafe {
        (address as *mut i64).write_unaligned(value);
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_load_byte(address: i64) -> i64 {
    unsafe { *(address as *const u8) as i64 }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_store_byte(address: i64, value: i64) {
    unsafe {
        *(address as *mut u8) = value as u8;
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_run(executable: *const NervString, arguments: *const NervArray) -> i64 {
    if arguments.is_null() {
        return -1;
    }
    let arguments = unsafe { &*arguments };
    let arguments = unsafe {
        std::slice::from_raw_parts(arguments.header.data, arguments.header.length as usize)
    };
    std::process::Command::new(text(executable))
        .args(arguments.iter().map(|arg| text(*arg as *const NervString)))
        .status()
        .ok()
        .and_then(|status| status.code())
        .map_or(-1, i64::from)
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_bytes(value: *const NervString) -> i64 {
    let bytes = text(value).as_bytes();
    let address = nerv_ffi_alloc(bytes.len() as i64 + 1);
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), address as *mut u8, bytes.len());
    }
    address
}

fn digest(bytes: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut data = bytes.to_vec();
    data.push(128);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&(bytes.len() as u64 * 8).to_be_bytes());
    let mut state = [
        0x6a09e667u32,
        0xbb67ae85,
        0x3c6ef372,
        0xa54ff53a,
        0x510e527f,
        0x9b05688c,
        0x1f83d9ab,
        0x5be0cd19,
    ];
    for block in data.chunks_exact(64) {
        let mut words = [0u32; 64];
        for (index, word) in block.chunks_exact(4).enumerate() {
            words[index] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for index in 16..64 {
            let a = words[index - 15];
            let b = words[index - 2];
            let x = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
            let y = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
            words[index] = words[index - 16]
                .wrapping_add(x)
                .wrapping_add(words[index - 7])
                .wrapping_add(y);
        }
        let mut work = state;
        for index in 0..64 {
            let a = work[4].rotate_right(6) ^ work[4].rotate_right(11) ^ work[4].rotate_right(25);
            let choose = (work[4] & work[5]) ^ (!work[4] & work[6]);
            let first = work[7]
                .wrapping_add(a)
                .wrapping_add(choose)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let b = work[0].rotate_right(2) ^ work[0].rotate_right(13) ^ work[0].rotate_right(22);
            let majority = (work[0] & work[1]) ^ (work[0] & work[2]) ^ (work[1] & work[2]);
            work.rotate_right(1);
            work[4] = work[4].wrapping_add(first);
            work[0] = first.wrapping_add(b).wrapping_add(majority);
        }
        for index in 0..8 {
            state[index] = state[index].wrapping_add(work[index]);
        }
    }
    state.iter().map(|value| format!("{value:08x}")).collect()
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_hash(value: *const NervString) -> *mut NervString {
    string(digest(text(value).as_bytes()))
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_file_hash(value: *const NervString) -> *mut NervString {
    string(
        std::fs::read(text(value))
            .map(|bytes| digest(&bytes))
            .unwrap_or_default(),
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_copy(source: *const NervString, target: *const NervString) -> i64 {
    std::fs::copy(text(source), text(target)).map_or(-1, |_| 0)
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_executable() -> *mut NervString {
    string(
        std::env::current_exe()
            .map(|file| file.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_lock(value: *const NervString) -> i64 {
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(text(value))
    {
        Ok(file) if file.lock().is_ok() => Box::into_raw(Box::new(file)) as i64,
        _ => 0,
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn nerv_ffi_unlock(handle: i64) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut std::fs::File));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::digest;
    #[test]
    fn hashes_standard_vectors() {
        assert_eq!(
            digest(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
