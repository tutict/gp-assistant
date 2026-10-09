//! OS-backed secrets. Never expose a read-secret IPC command.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex, OnceLock};

const PREFIX: &str = "gp-assistant.llm.";
const STORE_ERROR: &str = "Credential store unavailable; key retained for this session only.";
trait Backend: Send + Sync {
    fn write(&self, reference: &str, secret: &str) -> Result<(), ()>;
    fn read(&self, reference: &str) -> Result<Option<String>, ()>;
    fn delete(&self, reference: &str) -> Result<(), ()>;
}
#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub(crate) struct CredentialStatus {
    pub credential_ref: String,
    pub has_key: bool,
}
struct Vault {
    backend: Arc<dyn Backend>,
    lock: Mutex<()>,
}
fn validate_ref(reference: &str) -> Result<(), String> {
    let suffix = reference.strip_prefix(PREFIX).unwrap_or_default();
    if suffix.is_empty()
        || suffix.len() > 96
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Invalid credential reference.".into());
    }
    Ok(())
}
fn validate_secret(secret: &str) -> Result<(), String> {
    // Windows CRED_MAX_CREDENTIAL_BLOB_SIZE is 2560 bytes. Apply equally on Android.
    if secret.trim().is_empty() || secret.len() > 2560 || secret.contains(['\r', '\n', '\0']) {
        return Err("Invalid API key (maximum 2560 UTF-8 bytes).".into());
    }
    Ok(())
}
impl Vault {
    fn put(&self, reference: &str, secret: &str) -> Result<CredentialStatus, String> {
        validate_ref(reference)?;
        validate_secret(secret)?;
        let _guard = self.lock.lock().map_err(|_| STORE_ERROR)?;
        self.backend
            .write(reference, secret)
            .map_err(|_| STORE_ERROR)?;
        // Verification is native: neither this read nor any IPC result exposes the key.
        let readback = self.backend.read(reference).map_err(|_| STORE_ERROR)?;
        if readback.as_deref() != Some(secret) {
            return Err(STORE_ERROR.into());
        }
        Ok(CredentialStatus {
            credential_ref: reference.into(),
            has_key: true,
        })
    }
    fn status(&self, reference: &str) -> Result<CredentialStatus, String> {
        validate_ref(reference)?;
        let _guard = self.lock.lock().map_err(|_| STORE_ERROR)?;
        let secret = self.backend.read(reference).map_err(|_| STORE_ERROR)?;
        if let Some(ref secret) = secret {
            validate_secret(secret)?;
        }
        Ok(CredentialStatus {
            credential_ref: reference.into(),
            has_key: secret.is_some(),
        })
    }
    fn delete(&self, reference: &str) -> Result<CredentialStatus, String> {
        validate_ref(reference)?;
        let _guard = self.lock.lock().map_err(|_| STORE_ERROR)?;
        self.backend
            .delete(reference)
            .map_err(|_| "Credential deletion failed; configuration retained.")?;
        if self
            .backend
            .read(reference)
            .map_err(|_| STORE_ERROR)?
            .is_some()
        {
            return Err("Credential deletion verification failed.".into());
        }
        Ok(CredentialStatus {
            credential_ref: reference.into(),
            has_key: false,
        })
    }
    fn resolve(&self, reference: &str) -> Result<String, String> {
        validate_ref(reference)?;
        let _guard = self.lock.lock().map_err(|_| STORE_ERROR)?;
        let secret = self
            .backend
            .read(reference)
            .map_err(|_| STORE_ERROR)?
            .ok_or("Stored credential is missing; re-enter the API key.")?;
        validate_secret(&secret)?;
        Ok(secret)
    }
}
static VAULT: OnceLock<Vault> = OnceLock::new();
fn vault() -> Result<&'static Vault, String> {
    VAULT.get().ok_or_else(|| STORE_ERROR.into())
}

/// Resolve only in native request construction. Never mutate/return the request payload.
pub(crate) fn resolve_api_key(payload: &Value) -> Result<String, String> {
    if let Some(reference) = payload.get("credential_ref") {
        let reference = reference.as_str().ok_or("Invalid credential reference.")?;
        validate_ref(reference)?;
        return vault()?.resolve(reference);
    }
    let key = payload
        .get("api_key")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    // Session keys retain the existing provider limit; persistence has the OS limit above.
    if key.len() > 8192 || key.contains(['\r', '\n', '\0']) {
        return Err("Invalid API key.".into());
    }
    Ok(key.into())
}

#[tauri::command]
pub(crate) async fn api_credential_put(
    credential_ref: String,
    secret: String,
) -> Result<CredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(move || vault()?.put(&credential_ref, &secret))
        .await
        .map_err(|_| STORE_ERROR.to_string())?
}
#[tauri::command]
pub(crate) async fn api_credential_status(
    credential_ref: String,
) -> Result<CredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(move || vault()?.status(&credential_ref))
        .await
        .map_err(|_| STORE_ERROR.to_string())?
}
#[tauri::command]
pub(crate) async fn api_credential_delete(
    credential_ref: String,
) -> Result<CredentialStatus, String> {
    tauri::async_runtime::spawn_blocking(move || vault()?.delete(&credential_ref))
        .await
        .map_err(|_| STORE_ERROR.to_string())?
}

pub(crate) fn init<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("llm-credentials")
        .invoke_handler(|invoke| {
            invoke
                .resolver
                .reject("Credential mobile bridge is native-only.");
            true // false would let Tauri forward JS calls directly to Kotlin, including read!
        })
        .setup(|_app, _api| {
            #[cfg(target_os = "windows")]
            let backend: Arc<dyn Backend> = Arc::new(windows::WindowsBackend);
            #[cfg(target_os = "android")]
            let backend: Arc<dyn Backend> = Arc::new(android::AndroidBackend(
                _api.register_android_plugin("com.gpassistant.credentials", "CredentialPlugin")?,
            ));
            #[cfg(any(target_os = "windows", target_os = "android"))]
            VAULT
                .set(Vault {
                    backend,
                    lock: Mutex::new(()),
                })
                .map_err(|_| std::io::Error::other("Credential store already initialized"))?;
            // Unsupported OS: no file/key fallback. Commands fail closed.
            Ok(())
        })
        .build()
}

#[cfg(target_os = "windows")]
mod windows {
    use super::Backend;
    use windows_sys::Win32::{
        Foundation::{GetLastError, ERROR_NOT_FOUND},
        Security::Credentials::*,
    };
    pub(super) struct WindowsBackend;
    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }
    impl Backend for WindowsBackend {
        fn write(&self, reference: &str, secret: &str) -> Result<(), ()> {
            let mut target = wide(reference);
            let mut user = wide("gp-assistant");
            let credential = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: target.as_mut_ptr(),
                CredentialBlobSize: secret.len() as u32,
                CredentialBlob: secret.as_ptr() as *mut u8,
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: user.as_mut_ptr(),
                ..unsafe { std::mem::zeroed() }
            };
            // CredWrite copies the supplied blob before returning. No disk fallback.
            if unsafe { CredWriteW(&credential, 0) } == 0 {
                Err(())
            } else {
                Ok(())
            }
        }
        fn read(&self, reference: &str) -> Result<Option<String>, ()> {
            let target = wide(reference);
            let mut credential = std::ptr::null_mut();
            if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
                return if unsafe { GetLastError() } == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(())
                };
            }
            // The OS owns the returned allocation; release it even on decoding failure.
            let result = unsafe {
                let c = &*credential;
                if c.CredentialBlobSize == 0
                    || c.CredentialBlobSize > 2560
                    || c.CredentialBlob.is_null()
                {
                    Err(())
                } else {
                    String::from_utf8(
                        std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                            .to_vec(),
                    )
                    .map(Some)
                    .map_err(|_| ())
                }
            };
            unsafe {
                CredFree(credential.cast());
            }
            result
        }
        fn delete(&self, reference: &str) -> Result<(), ()> {
            let target = wide(reference);
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0
                || unsafe { GetLastError() } == ERROR_NOT_FOUND
            {
                Ok(())
            } else {
                Err(())
            }
        }
    }
}
#[cfg(target_os = "android")]
mod android {
    use super::*;
    pub(super) struct AndroidBackend<R: tauri::Runtime>(pub tauri::plugin::PluginHandle<R>);
    impl<R: tauri::Runtime> Backend for AndroidBackend<R> {
        fn write(&self, r: &str, s: &str) -> Result<(), ()> {
            self.0
                .run_mobile_plugin::<Value>(
                    "write",
                    serde_json::json!({"reference": r, "secret": s}),
                )
                .map(|_| ())
                .map_err(|_| ())
        }
        fn read(&self, r: &str) -> Result<Option<String>, ()> {
            // Native bridge only; not registered in Tauri's JS invoke handler.
            #[derive(Deserialize)]
            struct ReadResult {
                secret: Option<String>,
            }
            self.0
                .run_mobile_plugin::<ReadResult>("read", serde_json::json!({"reference": r}))
                .map(|v| v.secret)
                .map_err(|_| ())
        }
        fn delete(&self, r: &str) -> Result<(), ()> {
            self.0
                .run_mobile_plugin::<Value>("delete", serde_json::json!({"reference": r}))
                .map(|_| ())
                .map_err(|_| ())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Fake {
        values: Mutex<HashMap<String, String>>,
        fail_write: bool,
        corrupt_read: bool,
        fail_delete: bool,
    }
    impl Backend for Fake {
        fn write(&self, r: &str, s: &str) -> Result<(), ()> {
            if self.fail_write {
                return Err(());
            }
            self.values.lock().unwrap().insert(r.into(), s.into());
            Ok(())
        }
        fn read(&self, r: &str) -> Result<Option<String>, ()> {
            if self.corrupt_read {
                return Ok(Some("different-test-secret".into()));
            }
            Ok(self.values.lock().unwrap().get(r).cloned())
        }
        fn delete(&self, r: &str) -> Result<(), ()> {
            if self.fail_delete {
                return Err(());
            }
            self.values.lock().unwrap().remove(r);
            Ok(())
        }
    }
    fn vault(fake: Fake) -> Vault {
        Vault {
            backend: Arc::new(fake),
            lock: Mutex::new(()),
        }
    }
    const REF: &str = "gp-assistant.llm.test-provider";
    const SECRET: &str = "synthetic-test-key-not-a-user-secret";
    #[test]
    fn write_verify_resolve_status_and_delete_without_returning_secret() {
        let v = vault(Fake::default());
        let result = v.put(REF, SECRET).unwrap();
        assert!(result.has_key);
        assert!(!serde_json::to_string(&result).unwrap().contains(SECRET));
        assert_eq!(v.resolve(REF).unwrap(), SECRET);
        assert!(v.status(REF).unwrap().has_key);
        assert!(!v.delete(REF).unwrap().has_key);
        assert!(v.resolve(REF).is_err());
        assert!(!v.delete(REF).unwrap().has_key);
    }
    #[test]
    fn failed_write_or_verification_never_claims_success() {
        for fake in [
            Fake {
                fail_write: true,
                ..Fake::default()
            },
            Fake {
                corrupt_read: true,
                ..Fake::default()
            },
        ] {
            let error = vault(fake).put(REF, SECRET).unwrap_err();
            assert_eq!(error, STORE_ERROR);
            assert!(!error.contains(SECRET));
        }
    }
    #[test]
    fn invalid_references_and_secrets_fail_closed() {
        let v = vault(Fake::default());
        for r in ["", "other-app", "gp-assistant.llm.../escape"] {
            assert!(v.put(r, SECRET).is_err());
            assert!(v.status(r).is_err());
            assert!(v.delete(r).is_err());
        }
        for s in ["", "\r\n", "key\nheader"] {
            assert!(v.put(REF, s).is_err());
        }
        assert!(v.put(REF, &"x".repeat(2561)).is_err());
    }
    #[test]
    fn deletion_failure_is_not_reported_as_clear() {
        let v = vault(Fake {
            fail_delete: true,
            ..Fake::default()
        });
        v.put(REF, SECRET).unwrap();
        assert!(v.delete(REF).is_err());
        assert_eq!(v.resolve(REF).unwrap(), SECRET);
    }
    #[test]
    fn native_request_errors_are_redacted_and_payload_is_unchanged() {
        let payload =
            serde_json::json!({"credential_ref": "gp-assistant.llm.missing", "api_key": SECRET});
        let before = payload.clone();
        let error = resolve_api_key(&payload).unwrap_err();
        assert!(!error.contains(SECRET));
        assert_eq!(payload, before);
    }
    #[test]
    fn reference_errors_do_not_fall_back_to_inline_keys() {
        assert!(resolve_api_key(
            &serde_json::json!({"credential_ref": "invalid", "api_key": SECRET})
        )
        .is_err());
        assert_eq!(
            resolve_api_key(&serde_json::json!({"api_key": SECRET})).unwrap(),
            SECRET
        );
    }
}
