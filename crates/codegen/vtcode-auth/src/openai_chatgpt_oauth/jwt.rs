//! JWT claim parsing for ChatGPT ID and access tokens.

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    #[serde(default)]
    email: Option<String>,
    #[serde(rename = "https://api.openai.com/profile", default)]
    profile: Option<ProfileClaims>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    auth: Option<AuthClaims>,
}

#[derive(Debug, Deserialize)]
struct ProfileClaims {
    #[serde(default)]
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AuthClaims {
    #[serde(default)]
    chatgpt_plan_type: Option<String>,
    #[serde(default)]
    chatgpt_account_id: Option<String>,
}

#[derive(Debug)]
pub(crate) struct ParsedIdTokenClaims {
    pub(crate) email: Option<String>,
    pub(crate) account_id: Option<String>,
    pub(crate) plan: Option<String>,
}

pub(crate) fn parse_jwt_claims(jwt: &str) -> Result<ParsedIdTokenClaims> {
    let mut parts = jwt.split('.');
    let (_, payload_b64, _) = match (parts.next(), parts.next(), parts.next()) {
        (Some(header), Some(payload), Some(signature))
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty() =>
        {
            (header, payload, signature)
        }
        _ => bail!("invalid openai id token"),
    };

    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .context("failed to decode openai id token payload")?;
    let claims: IdTokenClaims = serde_json::from_slice(&payload).context("failed to parse openai id token payload")?;

    Ok(ParsedIdTokenClaims {
        email: claims.email.or_else(|| claims.profile.and_then(|profile| profile.email)),
        account_id: claims.auth.as_ref().and_then(|auth| auth.chatgpt_account_id.clone()),
        plan: claims.auth.and_then(|auth| auth.chatgpt_plan_type),
    })
}

/// Extract the standard `exp` (expiry) claim from a JWT, if present.
///
/// Returns `None` when the token is not a JWT or has no `exp` claim.
/// This is used to populate `expires_at` for Codex-imported sessions,
/// since Codex's `auth.json` does not store expiry separately.
pub(crate) fn parse_jwt_exp(jwt: &str) -> Option<u64> {
    let mut parts = jwt.split('.');
    let _ = parts.next()?;
    let payload_b64 = parts.next()?;
    if payload_b64.is_empty() {
        return None;
    }
    let payload = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
    #[derive(Deserialize)]
    struct ExpClaim {
        #[serde(default)]
        exp: Option<u64>,
    }
    let claims: ExpClaim = serde_json::from_slice(&payload).ok()?;
    claims.exp
}
