//! Offline-verifiable license keys.
//!
//! A license key is `PFAN1-<payload>.<signature>`, both base64url — the
//! payload is compact JSON (email, optional expiry, seat count) signed with
//! an Ed25519 key. Verification only needs the public key below, embedded in
//! every shipped binary, so activation works fully offline with no server.
//!
//! The matching private key is never in this repository — it lives with
//! whoever issues licenses (see `tools/license-keygen`). Losing it means
//! generating a new keypair and re-signing outstanding licenses; leaking it
//! means anyone can mint valid keys, so it must stay offline.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

const KEY_PREFIX: &str = "PFAN1-";

/// Ed25519 public key (base64url, 32 bytes), paired with the offline signing
/// key held by whoever issues licenses. Safe to embed — a public key cannot
/// be used to forge signatures, only to verify them.
pub const PUBLIC_KEY_B64: &str = "uDqJDiWeBFtbvDfiwBsBx1NEABJIhqPRN9Gps2rfZuk";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    pub email: String,
    /// Unix seconds; `None` is a perpetual (lifetime) license.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    pub seats: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LicenseStatus {
    Valid {
        email: String,
        expires: Option<u64>,
    },
    Expired {
        email: String,
        expired_at: u64,
    },
    /// Malformed, tampered, or signed with a different key.
    Invalid(String),
}

impl LicenseStatus {
    pub fn is_active(&self) -> bool {
        matches!(self, LicenseStatus::Valid { .. })
    }
}

/// Verify a license key against the embedded public key.
///
/// `now_unix` is passed in (rather than read internally) so this stays a
/// pure function — callers supply `SystemTime::now()`.
pub fn verify_key(key: &str, now_unix: u64) -> LicenseStatus {
    let Some(rest) = key.trim().strip_prefix(KEY_PREFIX) else {
        return LicenseStatus::Invalid(format!("key must start with '{KEY_PREFIX}'"));
    };
    let Some((payload_b64, sig_b64)) = rest.split_once('.') else {
        return LicenseStatus::Invalid("malformed key (expected payload.signature)".into());
    };

    let Ok(payload_bytes) = URL_SAFE_NO_PAD.decode(payload_b64) else {
        return LicenseStatus::Invalid("bad payload encoding".into());
    };
    let Ok(sig_bytes) = URL_SAFE_NO_PAD.decode(sig_b64) else {
        return LicenseStatus::Invalid("bad signature encoding".into());
    };
    let Ok(sig_arr) = <[u8; 64]>::try_from(sig_bytes.as_slice()) else {
        return LicenseStatus::Invalid("signature must be 64 bytes".into());
    };
    let signature = Signature::from_bytes(&sig_arr);

    let Ok(pk_bytes) = URL_SAFE_NO_PAD.decode(PUBLIC_KEY_B64) else {
        return LicenseStatus::Invalid("internal: embedded public key is malformed".into());
    };
    let Ok(pk_arr) = <[u8; 32]>::try_from(pk_bytes.as_slice()) else {
        return LicenseStatus::Invalid("internal: embedded public key has the wrong length".into());
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&pk_arr) else {
        return LicenseStatus::Invalid("internal: embedded public key is invalid".into());
    };

    if verifying_key.verify(&payload_bytes, &signature).is_err() {
        return LicenseStatus::Invalid(
            "signature does not match — this key was altered or is not genuine".into(),
        );
    }

    let Ok(payload) = serde_json::from_slice::<LicensePayload>(&payload_bytes) else {
        return LicenseStatus::Invalid("payload is not valid JSON".into());
    };

    match payload.exp {
        Some(exp) if exp < now_unix => LicenseStatus::Expired {
            email: payload.email,
            expired_at: exp,
        },
        exp => LicenseStatus::Valid {
            email: payload.email,
            expires: exp,
        },
    }
}

/// Free trial length before the menu-bar app and daemon fan control require a
/// license. The CLI's read-only commands (`temps`, `status`, `fans`, …) are
/// never gated — only the always-on menu-bar widget and persistent fan
/// control are the paid product.
pub const TRIAL_DAYS: u64 = 14;

/// Whether the menu-bar app / daemon are entitled to run, and why.
#[derive(Debug, Clone, PartialEq)]
pub enum Entitlement {
    Licensed { email: String },
    Trial { days_left: u64 },
    TrialExpired,
}

impl Entitlement {
    pub fn allowed(&self) -> bool {
        !matches!(self, Entitlement::TrialExpired)
    }
}

/// Resolve entitlement from a stored license key (if any) and the trial's
/// start time. `first_run_unix` should already be set by the caller on first
/// launch (persisted to config) before calling this — `None` here is treated
/// as "trial starts now," but does not persist anything itself.
pub fn check_entitlement(
    license_key: Option<&str>,
    first_run_unix: Option<u64>,
    now_unix: u64,
) -> Entitlement {
    if let Some(key) = license_key {
        if let LicenseStatus::Valid { email, .. } = verify_key(key, now_unix) {
            return Entitlement::Licensed { email };
        }
        // Expired or invalid keys fall through to the trial clock rather
        // than hard-failing — a bad paste shouldn't lock out a trial user.
    }

    match first_run_unix {
        None => Entitlement::Trial {
            days_left: TRIAL_DAYS,
        },
        Some(fr) => {
            let elapsed_days = now_unix.saturating_sub(fr) / 86_400;
            if elapsed_days >= TRIAL_DAYS {
                Entitlement::TrialExpired
            } else {
                Entitlement::Trial {
                    days_left: TRIAL_DAYS - elapsed_days,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn test_keypair() -> SigningKey {
        // Deterministic seed so this test never needs a CSPRNG dependency.
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn sign(signing_key: &SigningKey, payload: &LicensePayload) -> String {
        let json = serde_json::to_vec(payload).unwrap();
        let sig = signing_key.sign(&json);
        format!(
            "{KEY_PREFIX}{}.{}",
            URL_SAFE_NO_PAD.encode(json),
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    fn with_test_key(key: &str, now_unix: u64) -> LicenseStatus {
        // verify_key() checks against the real embedded PUBLIC_KEY_B64, so
        // these unit tests exercise the same logic against a local keypair
        // via a private verify path mirroring verify_key's internals.
        let signing_key = test_keypair();
        let public = signing_key.verifying_key();
        let rest = key.strip_prefix(KEY_PREFIX).unwrap();
        let (payload_b64, sig_b64) = rest.split_once('.').unwrap();
        let payload_bytes = URL_SAFE_NO_PAD.decode(payload_b64).unwrap();
        let sig_bytes = URL_SAFE_NO_PAD.decode(sig_b64).unwrap();
        let sig_arr: [u8; 64] = sig_bytes.try_into().unwrap();
        let signature = Signature::from_bytes(&sig_arr);
        if public.verify(&payload_bytes, &signature).is_err() {
            return LicenseStatus::Invalid("bad signature".into());
        }
        let payload: LicensePayload = serde_json::from_slice(&payload_bytes).unwrap();
        match payload.exp {
            Some(exp) if exp < now_unix => LicenseStatus::Expired {
                email: payload.email,
                expired_at: exp,
            },
            exp => LicenseStatus::Valid {
                email: payload.email,
                expires: exp,
            },
        }
    }

    #[test]
    fn valid_lifetime_key() {
        let signing_key = test_keypair();
        let payload = LicensePayload {
            email: "buyer@example.com".into(),
            exp: None,
            seats: 1,
        };
        let key = sign(&signing_key, &payload);
        let status = with_test_key(&key, 1_700_000_000);
        assert!(status.is_active());
    }

    #[test]
    fn expired_key() {
        let signing_key = test_keypair();
        let payload = LicensePayload {
            email: "buyer@example.com".into(),
            exp: Some(1_000),
            seats: 1,
        };
        let key = sign(&signing_key, &payload);
        let status = with_test_key(&key, 2_000);
        assert!(matches!(status, LicenseStatus::Expired { .. }));
    }

    #[test]
    fn tampered_payload_fails() {
        let signing_key = test_keypair();
        let payload = LicensePayload {
            email: "buyer@example.com".into(),
            exp: None,
            seats: 1,
        };
        let key = sign(&signing_key, &payload);
        // Flip one base64url character in the middle of the payload segment
        // (avoiding the last character, whose bit constraints can make a
        // flip decode-invalid rather than merely different) — the payload
        // decodes to different bytes, so the Ed25519 signature must fail to
        // verify against it.
        let (prefix_and_payload, sig) = key.rsplit_once('.').unwrap();
        let mut chars: Vec<char> = prefix_and_payload.chars().collect();
        let mid = chars.len() / 2;
        chars[mid] = if chars[mid] == 'A' { 'B' } else { 'A' };
        let tampered = format!("{}.{}", chars.into_iter().collect::<String>(), sig);
        let status = with_test_key(&tampered, 1_700_000_000);
        assert!(matches!(status, LicenseStatus::Invalid(_)));
    }

    #[test]
    fn garbage_key_is_invalid() {
        assert!(matches!(
            verify_key("not-a-real-key", 0),
            LicenseStatus::Invalid(_)
        ));
    }

    #[test]
    fn trial_counts_down_and_expires() {
        let day = 86_400;
        assert_eq!(
            check_entitlement(None, None, 1_000_000),
            Entitlement::Trial {
                days_left: TRIAL_DAYS
            }
        );
        assert_eq!(
            check_entitlement(None, Some(1_000_000), 1_000_000 + 5 * day),
            Entitlement::Trial {
                days_left: TRIAL_DAYS - 5
            }
        );
        assert_eq!(
            check_entitlement(None, Some(1_000_000), 1_000_000 + TRIAL_DAYS * day),
            Entitlement::TrialExpired
        );
    }

    #[test]
    fn invalid_key_falls_back_to_trial_not_lockout() {
        let status = check_entitlement(Some("garbage"), Some(0), 1);
        assert!(status.allowed());
        assert!(matches!(status, Entitlement::Trial { .. }));
    }
}
