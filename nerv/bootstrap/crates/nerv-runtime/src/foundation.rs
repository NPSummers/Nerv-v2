use super::{
    NervString, RuntimeSymbol,
    standard::{strings, text},
    string,
};
use std::{ffi::c_void, fs, io, path::Path};

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_split(
    value: *const NervString,
    separator: *const NervString,
) -> *mut c_void {
    strings(unsafe {
        text(value)
            .split(text(separator))
            .map(str::to_owned)
            .collect()
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_lines(value: *const NervString) -> *mut c_void {
    strings(unsafe { text(value).lines().map(str::to_owned).collect() })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_join(
    values: *const super::ArrayHeader,
    separator: *const NervString,
) -> *mut NervString {
    let array = unsafe { values.as_ref() }.expect("checked array");
    let values = unsafe {
        std::slice::from_raw_parts(
            array.data.cast::<*const NervString>(),
            array.length as usize,
        )
    };
    let parts: Vec<_> = values.iter().map(|value| unsafe { text(*value) }).collect();
    string(parts.join(unsafe { text(separator) }))
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_text_integer(value: i64) -> *mut NervString {
    string(value.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_text_float(value: f64) -> *mut NervString {
    string(value.to_string())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_valid_integer(value: *const NervString) -> i64 {
    i64::from(unsafe { text(value) }.parse::<i64>().is_ok())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_parse_integer(value: *const NervString) -> i64 {
    unsafe { text(value) }.parse().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_valid_float(value: *const NervString) -> i64 {
    i64::from(
        unsafe { text(value) }
            .parse::<f64>()
            .is_ok_and(f64::is_finite),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_parse_float(value: *const NervString) -> f64 {
    unsafe { text(value) }.parse().unwrap_or(0.0)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_format(
    template: *const NervString,
    values: *const super::ArrayHeader,
) -> *mut c_void {
    let array = unsafe { values.as_ref() }.expect("checked array");
    let values = unsafe {
        std::slice::from_raw_parts(
            array.data.cast::<*const NervString>(),
            array.length as usize,
        )
    };
    let mut result = String::new();
    let mut chars = unsafe { text(template) }.chars().peekable();
    let mut index = 0;
    let mut error = None;
    while let Some(ch) = chars.next() {
        match (ch, chars.peek().copied()) {
            ('{', Some('{')) | ('}', Some('}')) => {
                chars.next();
                result.push(ch);
            }
            ('{', Some('}')) => {
                chars.next();
                if let Some(value) = values.get(index) {
                    result.push_str(unsafe { text(*value) });
                    index += 1;
                } else {
                    error = Some("not enough format arguments");
                    break;
                }
            }
            ('{' | '}', _) => {
                error = Some("invalid format braces");
                break;
            }
            _ => result.push(ch),
        }
    }
    if error.is_none() && index != values.len() {
        error = Some("too many format arguments");
    }
    match error {
        Some(message) => strings(vec![String::new(), message.into()]),
        None => strings(vec![result, String::new()]),
    }
}

pub(super) fn error_kind(error: &io::Error) -> &'static str {
    match error.kind() {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::AlreadyExists => "already_exists",
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::InvalidData => "invalid_data",
        io::ErrorKind::NotADirectory => "not_a_directory",
        io::ErrorKind::IsADirectory => "is_a_directory",
        io::ErrorKind::DirectoryNotEmpty => "directory_not_empty",
        io::ErrorKind::Interrupted => "interrupted",
        _ => "other",
    }
}

fn fs_response(result: io::Result<String>) -> *mut c_void {
    match result {
        Ok(value) => strings(vec![value, String::new(), String::new()]),
        Err(error) => {
            let kind = error_kind(&error);
            strings(vec![String::new(), error.to_string(), kind.into()])
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_read_text(path: *const NervString) -> *mut c_void {
    fs_response(fs::read_to_string(unsafe { text(path) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_write_text(
    path: *const NervString,
    contents: *const NervString,
) -> *mut c_void {
    fs_response(fs::write(unsafe { text(path) }, unsafe { text(contents) }).map(|_| String::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_create_directory(path: *const NervString) -> *mut c_void {
    fs_response(fs::create_dir_all(unsafe { text(path) }).map(|_| String::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_rename(
    path: *const NervString,
    destination: *const NervString,
) -> *mut c_void {
    fs_response(
        fs::rename(unsafe { text(path) }, unsafe { text(destination) }).map(|_| String::new()),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_remove_file(path: *const NervString) -> *mut c_void {
    fs_response(fs::remove_file(unsafe { text(path) }).map(|_| String::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_remove_directory(path: *const NervString) -> *mut c_void {
    fs_response(fs::remove_dir(unsafe { text(path) }).map(|_| String::new()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_path_join(
    path: *const NervString,
    child: *const NervString,
) -> *mut NervString {
    string(
        Path::new(unsafe { text(path) })
            .join(unsafe { text(child) })
            .to_string_lossy()
            .into_owned(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_path_parent(path: *const NervString) -> *mut NervString {
    string(
        Path::new(unsafe { text(path) })
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_path_file_name(path: *const NervString) -> *mut NervString {
    string(
        Path::new(unsafe { text(path) })
            .file_name()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_path_is_absolute(path: *const NervString) -> i64 {
    i64::from(Path::new(unsafe { text(path) }).is_absolute())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_exists(path: *const NervString) -> *mut c_void {
    fs_response(
        Path::new(unsafe { text(path) })
            .try_exists()
            .map(|v| v.to_string()),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_path_current_directory() -> *mut c_void {
    fs_response(std::env::current_dir().map(|p| p.to_string_lossy().into_owned()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_text_hash(value: *const NervString) -> i64 {
    use std::hash::BuildHasher;
    static HASHER: std::sync::OnceLock<std::collections::hash_map::RandomState> =
        std::sync::OnceLock::new();
    (HASHER
        .get_or_init(std::collections::hash_map::RandomState::new)
        .hash_one(unsafe { text(value) })
        & i64::MAX as u64) as i64
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_path_extension(path: *const NervString) -> *mut NervString {
    string(
        Path::new(unsafe { text(path) })
            .extension()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_path_canonicalize(path: *const NervString) -> *mut c_void {
    fs_response(fs::canonicalize(unsafe { text(path) }).map(|p| p.to_string_lossy().into_owned()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_fs_metadata(path: *const NervString) -> *mut c_void {
    match fs::metadata(unsafe { text(path) }) {
        Ok(metadata) => {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|time| time.as_millis().min(i64::MAX as u128).to_string())
                .unwrap_or_else(|| "-1".into());
            strings(vec![
                metadata.len().to_string(),
                metadata.is_file().to_string(),
                metadata.is_dir().to_string(),
                modified,
                String::new(),
                String::new(),
            ])
        }
        Err(error) => strings(vec![
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            error.to_string(),
            error_kind(&error).into(),
        ]),
    }
}

pub(super) fn symbols() -> Vec<RuntimeSymbol> {
    vec![
        RuntimeSymbol {
            name: "nerv_path_extension",
            address: nerv_path_extension as *const (),
        },
        RuntimeSymbol {
            name: "nerv_path_canonicalize",
            address: nerv_path_canonicalize as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_metadata",
            address: nerv_fs_metadata as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_hash",
            address: nerv_text_hash as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_split",
            address: nerv_text_split as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_lines",
            address: nerv_text_lines as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_join",
            address: nerv_text_join as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_integer",
            address: nerv_text_integer as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_float",
            address: nerv_text_float as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_valid_integer",
            address: nerv_text_valid_integer as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_parse_integer",
            address: nerv_text_parse_integer as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_valid_float",
            address: nerv_text_valid_float as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_parse_float",
            address: nerv_text_parse_float as *const (),
        },
        RuntimeSymbol {
            name: "nerv_text_format",
            address: nerv_text_format as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_read_text",
            address: nerv_fs_read_text as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_write_text",
            address: nerv_fs_write_text as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_create_directory",
            address: nerv_fs_create_directory as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_rename",
            address: nerv_fs_rename as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_remove_file",
            address: nerv_fs_remove_file as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_remove_directory",
            address: nerv_fs_remove_directory as *const (),
        },
        RuntimeSymbol {
            name: "nerv_path_join",
            address: nerv_path_join as *const (),
        },
        RuntimeSymbol {
            name: "nerv_path_parent",
            address: nerv_path_parent as *const (),
        },
        RuntimeSymbol {
            name: "nerv_path_file_name",
            address: nerv_path_file_name as *const (),
        },
        RuntimeSymbol {
            name: "nerv_path_is_absolute",
            address: nerv_path_is_absolute as *const (),
        },
        RuntimeSymbol {
            name: "nerv_fs_exists",
            address: nerv_fs_exists as *const (),
        },
        RuntimeSymbol {
            name: "nerv_path_current_directory",
            address: nerv_path_current_directory as *const (),
        },
    ]
}
