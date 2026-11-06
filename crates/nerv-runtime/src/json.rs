use super::{
    NervString, RuntimeSymbol,
    standard::{strings, text},
    string,
};
use serde_json::Value;
use std::ffi::c_void;

const LIMIT: usize = 4 * 1024 * 1024;

fn parse(source: &str) -> Result<Value, String> {
    if source.len() > LIMIT {
        return Err("JSON exceeds 4 MiB".into());
    }
    serde_json::from_str(source).map_err(|e| e.to_string())
}

fn response(value: Result<Value, String>) -> *mut c_void {
    match value {
        Ok(value) => {
            let encoded = value.to_string();
            match parse(&encoded) {
                Ok(_) => strings(vec![encoded, String::new()]),
                Err(error) => strings(vec![String::new(), error]),
            }
        }
        Err(error) => strings(vec![String::new(), error]),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_parse(source: *const NervString) -> *mut c_void {
    response(parse(unsafe { text(source) }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_get(
    source: *const NervString,
    key: *const NervString,
) -> *mut c_void {
    response(parse(unsafe { text(source) }).and_then(|value| {
        value
            .as_object()
            .ok_or("JSON value is not an object")?
            .get(unsafe { text(key) })
            .cloned()
            .ok_or_else(|| "JSON key not found".into())
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_index(source: *const NervString, index: i64) -> *mut c_void {
    response(parse(unsafe { text(source) }).and_then(|value| {
        let values = value.as_array().ok_or("JSON value is not an array")?;
        usize::try_from(index)
            .ok()
            .and_then(|i| values.get(i))
            .cloned()
            .ok_or_else(|| "JSON index out of bounds".into())
    }))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_kind(source: *const NervString) -> *mut NervString {
    let kind = match parse(unsafe { text(source) }) {
        Ok(Value::Null) => "null",
        Ok(Value::Bool(_)) => "boolean",
        Ok(Value::Number(_)) => "number",
        Ok(Value::String(_)) => "string",
        Ok(Value::Array(_)) => "array",
        Ok(Value::Object(_)) => "object",
        Err(_) => "invalid",
    };
    string(kind.to_owned())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_text(
    source: *const NervString,
    fallback: *const NervString,
) -> *mut NervString {
    string(
        parse(unsafe { text(source) })
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| unsafe { text(fallback) }.to_owned()),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_i64(source: *const NervString, fallback: i64) -> i64 {
    parse(unsafe { text(source) })
        .ok()
        .and_then(|v| v.as_i64())
        .unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_f64(source: *const NervString, fallback: f64) -> f64 {
    parse(unsafe { text(source) })
        .ok()
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite())
        .unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_bool(source: *const NervString, fallback: bool) -> bool {
    parse(unsafe { text(source) })
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(fallback)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_len(source: *const NervString) -> i64 {
    match parse(unsafe { text(source) }) {
        Ok(Value::Array(a)) => a.len() as i64,
        Ok(Value::Object(o)) => o.len() as i64,
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_string(value: *const NervString) -> *mut NervString {
    string(Value::String(unsafe { text(value) }.to_owned()).to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_json_integer(value: i64) -> *mut NervString {
    string(value.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_json_number(value: f64) -> *mut c_void {
    response(
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| "JSON numbers must be finite".into()),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_set(
    source: *const NervString,
    key: *const NervString,
    member: *const NervString,
) -> *mut c_void {
    response((|| {
        let mut value = parse(unsafe { text(source) })?;
        value
            .as_object_mut()
            .ok_or("JSON value is not an object")?
            .insert(
                unsafe { text(key) }.to_owned(),
                parse(unsafe { text(member) })?,
            );
        if value.to_string().len() > LIMIT {
            return Err("JSON exceeds 4 MiB".into());
        }
        Ok(value)
    })())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_json_push(
    source: *const NervString,
    member: *const NervString,
) -> *mut c_void {
    response((|| {
        let mut value = parse(unsafe { text(source) })?;
        value
            .as_array_mut()
            .ok_or("JSON value is not an array")?
            .push(parse(unsafe { text(member) })?);
        if value.to_string().len() > LIMIT {
            return Err("JSON exceeds 4 MiB".into());
        }
        Ok(value)
    })())
}

pub(super) fn symbols() -> Vec<RuntimeSymbol> {
    vec![
        RuntimeSymbol {
            name: "nerv_json_parse",
            address: nerv_json_parse as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_get",
            address: nerv_json_get as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_index",
            address: nerv_json_index as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_kind",
            address: nerv_json_kind as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_text",
            address: nerv_json_text as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_i64",
            address: nerv_json_i64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_f64",
            address: nerv_json_f64 as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_bool",
            address: nerv_json_bool as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_len",
            address: nerv_json_len as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_string",
            address: nerv_json_string as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_integer",
            address: nerv_json_integer as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_number",
            address: nerv_json_number as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_set",
            address: nerv_json_set as *const (),
        },
        RuntimeSymbol {
            name: "nerv_json_push",
            address: nerv_json_push as *const (),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_nested_json_and_preserves_large_numbers() {
        let text = r#"{"id":"18446744073709551615","number":18446744073709551616,"items":[null,true,"\u00e9"]}"#;
        let value = parse(text).unwrap();
        assert_eq!(value["number"].to_string(), "18446744073709551616");
        assert_eq!(value["items"][2], "\u{e9}");
        assert!(parse("{invalid}").is_err());
        assert!(parse("1 trailing").is_err());
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        assert!(parse(&nested).unwrap_err().contains("recursion limit"));
    }

    #[test]
    fn strings_round_trip_controls_unicode_and_nul() {
        let _execution = super::super::Execution::new();
        let original = "quote\" newline\n nul\0 unicode\u{e9}";
        let value = string(original.to_owned());
        let encoded = unsafe { nerv_json_string(value) };
        let decoded = unsafe { nerv_json_text(encoded, string("fallback".into())) };
        assert_eq!(unsafe { text(decoded) }, original);
        assert_eq!(
            parse(unsafe { text(encoded) }).unwrap(),
            Value::String(original.to_owned())
        );
    }
}
