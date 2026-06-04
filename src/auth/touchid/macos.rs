use anyhow::{Context, Result};
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFMutableDictionary;
use core_foundation::string::CFString;
use security_framework_sys::base::{errSecItemNotFound, errSecSuccess};
use security_framework_sys::item::*;
use security_framework_sys::keychain_item::*;

const KEYCHAIN_SERVICE: &str = "magelab";
const KEYCHAIN_ACCOUNT: &str = "refresh-bio";

/// Check if Touch ID hardware is available via LAContext
pub fn is_hardware_available() -> bool {
    use objc2_local_authentication::LAContext;

    let context = unsafe { LAContext::new() };
    unsafe {
        context
            .canEvaluatePolicy_error(
                objc2_local_authentication::LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            )
            .is_ok()
    }
}

/// Prompt the user with a standalone Touch ID dialog via LAContext
pub fn prompt_biometric(reason: &str) -> Result<()> {
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};

    let context = unsafe { LAContext::new() };
    let reason_ns = NSString::from_str(reason);

    let (tx, rx) = std::sync::mpsc::channel();

    let block = block2::RcBlock::new(move |success: Bool, error: *mut NSError| {
        if success.as_bool() {
            tx.send(Ok(())).ok();
        } else {
            let msg = if error.is_null() {
                "Touch ID verification failed.".to_string()
            } else {
                let code = unsafe { NSError::code(&*error) };
                match code {
                    -2 => "Touch ID verification cancelled.".to_string(),
                    -8 => "Touch ID is locked. Use your device passcode to unlock, then try again."
                        .to_string(),
                    _ => format!("Touch ID verification failed (code {}).", code),
                }
            };
            tx.send(Err(anyhow::anyhow!("{}", msg))).ok();
        }
    });

    unsafe {
        context.evaluatePolicy_localizedReason_reply(
            LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
            &reason_ns,
            &block,
        );
    }

    rx.recv().context("Touch ID callback not received")?
}

/// Helper to create a keychain query dictionary with the correct types.
unsafe fn base_query() -> CFMutableDictionary<CFString, CFType> {
    let mut query = CFMutableDictionary::<CFString, CFType>::new();
    query.add(
        &CFString::wrap_under_get_rule(kSecClass),
        &CFType::wrap_under_get_rule(kSecClassGenericPassword as *const _),
    );
    query.add(
        &CFString::wrap_under_get_rule(kSecAttrService),
        &CFString::from_static_string(KEYCHAIN_SERVICE).as_CFType(),
    );
    query.add(
        &CFString::wrap_under_get_rule(kSecAttrAccount),
        &CFString::from_static_string(KEYCHAIN_ACCOUNT).as_CFType(),
    );
    query
}

/// Delete legacy biometric Keychain item
pub fn delete_biometric_item() -> Result<()> {
    unsafe {
        let query = base_query();
        let status = SecItemDelete(query.as_concrete_TypeRef() as _);

        if status != errSecSuccess && status != errSecItemNotFound {
            anyhow::bail!(
                "Failed to delete biometric Keychain item (OSStatus {})",
                status
            );
        }
    }

    Ok(())
}
