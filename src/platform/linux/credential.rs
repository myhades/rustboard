use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};

#[link(name = "libsecret-1.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn secret_schema_new(name: *const c_char, flags: i32, ...) -> *mut c_void;
    fn secret_schema_unref(schema: *mut c_void);
    fn secret_password_store_sync(
        schema: *const c_void,
        collection: *const c_char,
        label: *const c_char,
        password: *const c_char,
        cancellable: *mut c_void,
        error: *mut *mut c_void,
        ...
    ) -> i32;
    fn secret_password_lookup_sync(
        schema: *const c_void,
        cancellable: *mut c_void,
        error: *mut *mut c_void,
        ...
    ) -> *mut c_char;
    fn secret_password_free(password: *mut c_char);
}
#[link(name = "libglib-2.0.so.0", kind = "dylib", modifiers = "+verbatim")]
unsafe extern "C" {
    fn g_error_free(error: *mut c_void);
}

pub fn store(account: &str, key: &[u8]) -> Result<Value> {
    let name = CString::new(account)?;
    let mut encoded = STANDARD.encode(key).into_bytes();
    encoded.push(0);
    let mut error = ptr::null_mut();
    let ok = unsafe {
        let schema = secret_schema_new(
            c"org.rustboard.Identity".as_ptr(),
            0,
            c"account".as_ptr(),
            0i32,
            ptr::null::<c_char>(),
        );
        ensure!(!schema.is_null(), "cannot create credential schema");
        let ok = secret_password_store_sync(
            schema,
            c"default".as_ptr(),
            c"Rustboard device identity".as_ptr(),
            encoded.as_ptr().cast(),
            ptr::null_mut(),
            &mut error,
            c"account".as_ptr(),
            name.as_ptr(),
            ptr::null::<c_char>(),
        );
        secret_schema_unref(schema);
        if !error.is_null() {
            g_error_free(error);
        }
        ok
    };
    for byte in &mut encoded {
        unsafe {
            ptr::write_volatile(byte, 0);
        }
    }
    ensure!(ok != 0, "cannot save identity; unlock the desktop keyring");
    Ok(json!({"provider":"secret-service","account":account}))
}
pub fn load(reference: &Value) -> Result<Vec<u8>> {
    ensure!(
        reference["provider"] == "secret-service",
        "identity requires Secret Service"
    );
    let account = reference["account"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing credential account"))?;
    ensure!(
        account.len() == 64 && account.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid credential account"
    );
    let name = CString::new(account)?;
    let mut error = ptr::null_mut();
    unsafe {
        let schema = secret_schema_new(
            c"org.rustboard.Identity".as_ptr(),
            0,
            c"account".as_ptr(),
            0i32,
            ptr::null::<c_char>(),
        );
        ensure!(!schema.is_null(), "cannot create credential schema");
        let value = secret_password_lookup_sync(
            schema,
            ptr::null_mut(),
            &mut error,
            c"account".as_ptr(),
            name.as_ptr(),
            ptr::null::<c_char>(),
        );
        secret_schema_unref(schema);
        if !error.is_null() {
            g_error_free(error);
        }
        ensure!(
            !value.is_null(),
            "identity unavailable; unlock the desktop keyring"
        );
        let result = STANDARD.decode(CStr::from_ptr(value).to_bytes());
        secret_password_free(value);
        Ok(result?)
    }
}
