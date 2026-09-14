use crate::error::{OrbitError, Result};
use crate::ports::vault::VaultPort;
use std::ptr;

const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[link(name = "crypt32")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CryptProtectData(
        pDataIn: *const DataBlob,
        szDataDescr: *const u16,
        pOptionalEntropy: *const DataBlob,
        pvReserved: *mut std::ffi::c_void,
        pPromptStruct: *mut std::ffi::c_void,
        dwFlags: u32,
        pDataOut: *mut DataBlob,
    ) -> i32;

    fn CryptUnprotectData(
        pDataIn: *const DataBlob,
        ppszDataDescr: *mut *mut u16,
        pOptionalEntropy: *const DataBlob,
        pvReserved: *mut std::ffi::c_void,
        pPromptStruct: *mut std::ffi::c_void,
        dwFlags: u32,
        pDataOut: *mut DataBlob,
    ) -> i32;

    fn LocalFree(hMem: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    fn GetLastError() -> u32;
}

/// Windows native DPAPI hardware-bound vault implementation.
/// Enforces current user scope and forbids UI prompts.
#[derive(Default, Clone)]
pub struct DpapiVault;

impl VaultPort for DpapiVault {
    fn seal(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let in_blob = DataBlob {
            cb_data: plaintext.len() as u32,
            pb_data: plaintext.as_ptr() as *mut u8,
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        let success = unsafe {
            CryptProtectData(
                &in_blob,
                ptr::null(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };

        if success == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(OrbitError::Vault(format!(
                "DPAPI CryptProtectData failed with error code {err_code}"
            )));
        }

        if out_blob.pb_data.is_null() {
            if out_blob.cb_data == 0 {
                return Ok(Vec::new());
            } else {
                return Err(OrbitError::Vault(
                    "DPAPI CryptProtectData returned null data pointer with non-zero length".into(),
                ));
            }
        }

        if out_blob.cb_data == 0 {
            unsafe {
                LocalFree(out_blob.pb_data as *mut _);
            }
            return Ok(Vec::new());
        }

        let result = unsafe {
            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let vec = slice.to_vec();
            LocalFree(out_blob.pb_data as *mut _);
            vec
        };

        Ok(result)
    }

    fn unseal(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let in_blob = DataBlob {
            cb_data: ciphertext.len() as u32,
            pb_data: ciphertext.as_ptr() as *mut u8,
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: ptr::null_mut(),
        };

        let success = unsafe {
            CryptUnprotectData(
                &in_blob,
                ptr::null_mut(),
                ptr::null(),
                ptr::null_mut(),
                ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            )
        };

        if success == 0 {
            let err_code = unsafe { GetLastError() };
            return Err(OrbitError::Vault(format!(
                "DPAPI CryptUnprotectData failed with error code {err_code} (access denied or data corrupted)"
            )));
        }

        if out_blob.pb_data.is_null() {
            if out_blob.cb_data == 0 {
                return Ok(Vec::new());
            } else {
                return Err(OrbitError::Vault(
                    "DPAPI CryptUnprotectData returned null data pointer with non-zero length"
                        .into(),
                ));
            }
        }

        if out_blob.cb_data == 0 {
            unsafe {
                LocalFree(out_blob.pb_data as *mut _);
            }
            return Ok(Vec::new());
        }

        let result = unsafe {
            let slice = std::slice::from_raw_parts_mut(out_blob.pb_data, out_blob.cb_data as usize);
            let vec = slice.to_vec();
            for b in slice.iter_mut() {
                std::ptr::write_volatile(b, 0);
            }
            LocalFree(out_blob.pb_data as *mut _);
            vec
        };

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dpapi_roundtrip() {
        let vault = DpapiVault;
        let secret = b"super_secret_oauth_token_payload_2026";
        let sealed = vault.seal(secret).expect("DPAPI seal must succeed");
        assert_ne!(sealed, secret);

        let unsealed = vault.unseal(&sealed).expect("DPAPI unseal must succeed");
        assert_eq!(unsealed, secret);
    }
}
