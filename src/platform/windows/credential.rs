//! Current-user DPAPI. No machine scope, password, or custom cryptography.
use anyhow::{Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::*,
    },
    core::w,
};
fn crypt(bytes: &[u8], protect: bool) -> Result<Vec<u8>> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= 16384,
        "invalid DPAPI input size"
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if protect {
            CryptProtectData(
                &input,
                w!("Rustboard device identity"),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        } else {
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )?;
        }
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        for n in 0..output.cbData as usize {
            output.pbData.add(n).write_volatile(0);
        }
        LocalFree(Some(HLOCAL(output.pbData.cast())));
        Ok(result)
    }
}
pub fn store(_account: &str, key: &[u8]) -> Result<Value> {
    Ok(json!({"provider":"windows-dpapi","blob":STANDARD.encode(crypt(key,true)?)}))
}
pub fn load(reference: &Value) -> Result<Vec<u8>> {
    ensure!(
        reference["provider"] == "windows-dpapi",
        "identity requires Windows DPAPI"
    );
    let blob = STANDARD.decode(
        reference["blob"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("DPAPI blob must be base64 text"))?,
    )?;
    crypt(&blob, false)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dpapi_roundtrip_and_tampering() {
        let key = b"synthetic secret";
        let mut reference = store("test", key).unwrap();
        assert_eq!(load(&reference).unwrap(), key);
        let mut blob = STANDARD
            .decode(reference["blob"].as_str().unwrap())
            .unwrap();
        blob[30] ^= 1;
        reference["blob"] = STANDARD.encode(blob).into();
        assert!(load(&reference).is_err());
    }
}
