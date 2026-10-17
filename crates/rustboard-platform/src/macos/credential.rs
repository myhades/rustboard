//! Login Keychain through Security.framework; only the signed Agent accesses the item.
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::ffi::CString;
unsafe extern "C" {
    fn rb_keychain_store(account: *const libc::c_char, bytes: *const u8, length: usize) -> i32;
    fn rb_keychain_load(
        account: *const libc::c_char,
        bytes: *mut u8,
        capacity: usize,
        length: *mut usize,
    ) -> i32;
}
pub fn store(account: &str, key: &[u8]) -> Result<Value> {
    let name = CString::new(account)?;
    let status = unsafe { rb_keychain_store(name.as_ptr(), key.as_ptr(), key.len()) };
    ensure!(
        status == 0,
        "Keychain store failed (OSStatus {status}); identity migration not committed"
    );
    Ok(json!({"provider":"macos-keychain","account":account}))
}
pub fn load(reference: &Value) -> Result<Vec<u8>> {
    ensure!(
        reference["provider"] == "macos-keychain",
        "identity requires macOS Keychain"
    );
    let account = reference["account"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing Keychain account"))?;
    ensure!(
        account.len() == 64 && account.bytes().all(|c| c.is_ascii_hexdigit()),
        "invalid Keychain account"
    );
    let name = CString::new(account)?;
    let mut key = vec![0; 16384];
    let mut length = 0;
    let status =
        unsafe { rb_keychain_load(name.as_ptr(), key.as_mut_ptr(), key.len(), &mut length) };
    ensure!(
        status == 0 && length > 0 && length <= key.len(),
        "Keychain read failed (OSStatus {status}); unlock login Keychain and restart; no replacement identity generated"
    );
    key.truncate(length);
    Ok(key)
}
