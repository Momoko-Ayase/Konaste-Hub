//! Passkey sign-in through Windows Hello / Windows Security (webauthn.dll).
//!
//! my.konami.net hands out standard WebAuthn request options. The Hub asks
//! Windows for an assertion as if it were a browser on that site, and returns
//! it in the JSON shape the official page posts (SimpleWebAuthn's
//! `AuthenticationResponseJSON`).
use crate::signin::{ORIGIN, PasskeyFailure, RP_ID};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use serde::Deserialize;

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "publicKey")]
    public_key: Options,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    challenge: String,
    timeout: Option<u32>,
    rp_id: Option<String>,
    user_verification: Option<String>,
    #[serde(default)]
    allow_credentials: Vec<Descriptor>,
}

#[derive(Deserialize)]
struct Descriptor {
    id: String,
}

/// What Windows is asked for, parsed and checked.
#[derive(Debug, PartialEq)]
pub struct Request {
    pub client_data: String,
    pub timeout_ms: u32,
    pub user_verification: u32,
    pub allow: Vec<Vec<u8>>,
}

const UV_REQUIRED: u32 = 1;
const UV_PREFERRED: u32 = 2;
const UV_DISCOURAGED: u32 = 3;

pub fn parse(options: &str) -> Option<Request> {
    let envelope: Envelope = serde_json::from_str(options).ok()?;
    let options = envelope.public_key;
    // Only the site the Hub signs in to.
    if options.rp_id.as_deref().is_some_and(|rp| rp != RP_ID) {
        return None;
    }
    let challenge = B64.decode(options.challenge.trim_end_matches('=')).ok()?;
    if challenge.len() < 16 {
        return None;
    }
    let allow = options.allow_credentials.iter().map(|c| B64.decode(c.id.trim_end_matches('=')).ok()).collect::<Option<Vec<_>>>()?;
    let client_data =
        format!(r#"{{"type":"webauthn.get","challenge":"{}","origin":"{ORIGIN}","crossOrigin":false}}"#, B64.encode(&challenge));
    Some(Request {
        client_data,
        timeout_ms: options.timeout.unwrap_or(300_000).clamp(30_000, 600_000),
        user_verification: match options.user_verification.as_deref() {
            Some("required") => UV_REQUIRED,
            Some("discouraged") => UV_DISCOURAGED,
            _ => UV_PREFERRED,
        },
        allow,
    })
}

/// The JSON posted back to my.konami.net.
pub fn response(
    request: &Request,
    credential: &[u8],
    authenticator_data: &[u8],
    signature: &[u8],
    user: &[u8],
    platform: Option<bool>,
) -> String {
    let id = B64.encode(credential);
    let mut out = serde_json::json!({
        "id": id,
        "rawId": id,
        "type": "public-key",
        "response": {
            "clientDataJSON": B64.encode(request.client_data.as_bytes()),
            "authenticatorData": B64.encode(authenticator_data),
            "signature": B64.encode(signature),
        },
        "clientExtensionResults": {},
    });
    if !user.is_empty() {
        out["response"]["userHandle"] = B64.encode(user).into();
    }
    if let Some(platform) = platform {
        out["authenticatorAttachment"] = (if platform { "platform" } else { "cross-platform" }).into();
    }
    out.to_string()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Mutex;
    use windows_sys::{
        Win32::{Foundation::HWND, Networking::WindowsWebServices::*},
        core::GUID,
    };

    /// The operation in progress, so closing the sign-in dialog can cancel it.
    static CURRENT: Mutex<Option<GUID>> = Mutex::new(None);

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// Blocks until Windows answers; run it off the UI thread. `owner` is the
    /// Hub window, so the Windows Security dialog opens over it.
    pub fn get(owner: isize, options: &str) -> Result<String, PasskeyFailure> {
        let request = parse(options).ok_or(PasskeyFailure::Failed)?;
        let api = unsafe { WebAuthNGetApiVersionNumber() };
        if api == 0 {
            return Err(PasskeyFailure::Failed);
        }
        let rp = wide(RP_ID);
        let hash = WEBAUTHN_HASH_ALGORITHM_SHA_256;
        let mut client_bytes = request.client_data.clone().into_bytes();
        let client = WEBAUTHN_CLIENT_DATA {
            dwVersion: WEBAUTHN_CLIENT_DATA_CURRENT_VERSION,
            cbClientDataJSON: client_bytes.len() as u32,
            pbClientDataJSON: client_bytes.as_mut_ptr(),
            pwszHashAlgId: hash,
        };
        let mut ids = request.allow.clone();
        let mut credentials: Vec<WEBAUTHN_CREDENTIAL> = ids
            .iter_mut()
            .map(|id| WEBAUTHN_CREDENTIAL {
                dwVersion: WEBAUTHN_CREDENTIAL_CURRENT_VERSION,
                cbId: id.len() as u32,
                pbId: id.as_mut_ptr(),
                pwszCredentialType: WEBAUTHN_CREDENTIAL_TYPE_PUBLIC_KEY,
            })
            .collect();
        let mut cancel: GUID = unsafe { std::mem::zeroed() };
        let cancellable = unsafe { WebAuthNGetCancellationId(&mut cancel) } == 0;
        let mut options: WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS = unsafe { std::mem::zeroed() };
        // The newest options layout this Windows understands.
        options.dwVersion = match api {
            1 => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_4,
            2 => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_5,
            3 => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_6,
            _ => WEBAUTHN_AUTHENTICATOR_GET_ASSERTION_OPTIONS_VERSION_7,
        };
        options.dwTimeoutMilliseconds = request.timeout_ms;
        options.CredentialList = WEBAUTHN_CREDENTIALS { cCredentials: credentials.len() as u32, pCredentials: credentials.as_mut_ptr() };
        options.dwAuthenticatorAttachment = WEBAUTHN_AUTHENTICATOR_ATTACHMENT_ANY;
        options.dwUserVerificationRequirement = request.user_verification;
        if cancellable {
            options.pCancellationId = &mut cancel;
            *CURRENT.lock().unwrap() = Some(cancel);
        }
        let mut assertion: *mut WEBAUTHN_ASSERTION = std::ptr::null_mut();
        let result = unsafe { WebAuthNAuthenticatorGetAssertion(owner as HWND, rp.as_ptr(), &client, &options, &mut assertion) };
        *CURRENT.lock().unwrap() = None;
        drop(credentials);
        if result != 0 || assertion.is_null() {
            // The user closed the dialog or let it time out: not an error.
            const NTE_USER_CANCELLED: i32 = 0x8009_0036_u32 as i32;
            const ERROR_CANCELLED: i32 = 0x8007_04C7_u32 as i32;
            const ERROR_TIMEOUT: i32 = 0x8007_05B4_u32 as i32;
            return Err(if matches!(result, NTE_USER_CANCELLED | ERROR_CANCELLED | ERROR_TIMEOUT) {
                PasskeyFailure::Cancelled
            } else {
                PasskeyFailure::Failed
            });
        }
        let slice = |ptr: *mut u8, len: u32| -> Vec<u8> {
            if ptr.is_null() || len == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(ptr, len as usize) }.to_vec() }
        };
        let out = unsafe {
            let a = &*assertion;
            let platform =
                (a.dwVersion >= WEBAUTHN_ASSERTION_VERSION_3).then_some(a.dwUsedTransport & WEBAUTHN_CTAP_TRANSPORT_INTERNAL != 0);
            let credential = slice(a.Credential.pbId, a.Credential.cbId);
            let data = slice(a.pbAuthenticatorData, a.cbAuthenticatorData);
            let signature = slice(a.pbSignature, a.cbSignature);
            let user = slice(a.pbUserId, a.cbUserId);
            WebAuthNFreeAssertion(assertion);
            (credential, data, signature, user, platform)
        };
        if out.0.is_empty() || out.1.is_empty() || out.2.is_empty() {
            return Err(PasskeyFailure::Failed);
        }
        Ok(response(&request, &out.0, &out.1, &out.2, &out.3, out.4))
    }

    /// Close the Windows Security dialog, if one is open.
    pub fn cancel() {
        if let Some(id) = *CURRENT.lock().unwrap() {
            unsafe { WebAuthNCancelCurrentOperation(&id) };
        }
    }
}

#[cfg(windows)]
pub use imp::{cancel, get};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_options_parse() {
        // The shape my.konami.net returned for an anonymous request.
        let options = r#"{"publicKey":{"challenge":"oYzehfT4810NXt9_xfqKt9AXSCpdUu9lOUMr2JlMOOs","timeout":300000,"rpId":"my.konami.net","userVerification":"preferred"}}"#;
        let request = parse(options).unwrap();
        assert_eq!(request.timeout_ms, 300_000);
        assert_eq!(request.user_verification, UV_PREFERRED);
        assert!(request.allow.is_empty());
        assert_eq!(
            request.client_data,
            r#"{"type":"webauthn.get","challenge":"oYzehfT4810NXt9_xfqKt9AXSCpdUu9lOUMr2JlMOOs","origin":"https://my.konami.net","crossOrigin":false}"#
        );
    }

    #[test]
    fn other_sites_and_short_challenges_are_refused() {
        assert!(parse(r#"{"publicKey":{"challenge":"oYzehfT4810NXt9_xfqKt9AXSCpdUu9lOUMr2JlMOOs","rpId":"evil.test"}}"#).is_none());
        assert!(parse(r#"{"publicKey":{"challenge":"AAAA"}}"#).is_none());
        assert!(parse("not json").is_none());
    }

    #[test]
    fn allowed_credentials_are_decoded() {
        let request = parse(r#"{"publicKey":{"challenge":"oYzehfT4810NXt9_xfqKt9AXSCpdUu9lOUMr2JlMOOs","allowCredentials":[{"id":"AQID","type":"public-key"}],"userVerification":"required"}}"#).unwrap();
        assert_eq!(request.allow, vec![vec![1, 2, 3]]);
        assert_eq!(request.user_verification, UV_REQUIRED);
    }

    #[test]
    fn response_matches_the_official_shape() {
        let request = parse(r#"{"publicKey":{"challenge":"oYzehfT4810NXt9_xfqKt9AXSCpdUu9lOUMr2JlMOOs"}}"#).unwrap();
        let json: serde_json::Value = serde_json::from_str(&response(&request, &[1, 2], &[3], &[4], &[], Some(true))).unwrap();
        assert_eq!(json["id"], "AQI");
        assert_eq!(json["rawId"], "AQI");
        assert_eq!(json["type"], "public-key");
        assert_eq!(json["authenticatorAttachment"], "platform");
        assert_eq!(json["response"]["authenticatorData"], "Aw");
        assert_eq!(json["response"]["signature"], "BA");
        assert!(json["response"].get("userHandle").is_none());
        let client = B64.decode(json["response"]["clientDataJSON"].as_str().unwrap()).unwrap();
        assert_eq!(client, request.client_data.as_bytes());
    }
}
