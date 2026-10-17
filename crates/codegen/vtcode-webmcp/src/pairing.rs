use crate::error::{Result, WebmcpError};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;
use vtcode_commons::validation::is_valid_origin;

const PAIRING_CODE_HEX_DIGITS: usize = 12;
const MAX_FAILED_PAIRING_ATTEMPTS: u8 = 5;

#[derive(Debug)]
struct PendingPairing {
    code: String,
    origin: Option<String>,
    expires_at: Instant,
    used: bool,
    failed_attempts: u8,
}

#[derive(Debug)]
struct PairingState {
    pending: Option<PendingPairing>,
    sessions: HashMap<String, PairingSessionState>,
}

#[derive(Debug)]
struct PairingSessionState {
    origin: String,
    expires_at: Instant,
}

/// A terminal-displayable, short-lived pairing code.
#[derive(Clone)]
pub struct PairingDisplay {
    code: String,
    expires_at: Instant,
}

impl PairingDisplay {
    /// Returns the code that must be entered by the browser.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Returns the remaining lifetime of the code.
    pub fn expires_in(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

impl std::fmt::Debug for PairingDisplay {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingDisplay")
            .field("expires_in", &self.expires_in())
            .finish_non_exhaustive()
    }
}

/// An authenticated, in-memory browser session.
#[derive(Clone)]
pub struct PairingSession {
    token: String,
    origin: String,
    expires_at: Instant,
}

impl PairingSession {
    /// Returns the bearer token. Callers should keep it in memory only.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns the origin bound to this session.
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// Returns the remaining inactivity-lease lifetime.
    pub fn expires_in(&self) -> Duration {
        self.expires_at.saturating_duration_since(Instant::now())
    }
}

impl std::fmt::Debug for PairingSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PairingSession")
            .field("origin", &self.origin)
            .field("expires_in", &self.expires_in())
            .finish_non_exhaustive()
    }
}

/// Manages one-time pairing and revocable in-memory sessions.
#[derive(Clone)]
pub struct PairingManager {
    allowed_origins: Arc<HashSet<String>>,
    ttl: Duration,
    state: Arc<Mutex<PairingState>>,
}

impl PairingManager {
    /// Create a manager with an explicit origin allowlist and inactivity lease.
    pub fn new<I, S>(allowed_origins: I, ttl: Duration) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        if ttl.is_zero() || ttl > Duration::from_secs(3600) {
            return Err(WebmcpError::InvalidRequest("pairing ttl must be between 1 and 3600 seconds".to_string()));
        }

        let origins = allowed_origins.into_iter().map(Into::into).collect::<Vec<String>>();
        if origins.iter().any(|origin| !is_valid_origin(origin)) {
            return Err(WebmcpError::InvalidRequest("WebMCP origins must be exact non-wildcard origins".to_string()));
        }

        Ok(Self {
            allowed_origins: Arc::new(origins.into_iter().collect()),
            ttl,
            state: Arc::new(Mutex::new(PairingState { pending: None, sessions: HashMap::new() })),
        })
    }

    /// Starts a fresh pairing code, invalidating any older unconsumed code.
    pub fn begin_pairing(&self) -> PairingDisplay {
        self.begin_pairing_inner(None, false)
    }

    fn begin_pairing_inner(&self, origin: Option<String>, revoke_sessions: bool) -> PairingDisplay {
        let code = Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(PAIRING_CODE_HEX_DIGITS)
            .collect::<String>()
            .to_ascii_uppercase();
        let expires_at = Instant::now() + self.ttl;
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if revoke_sessions {
            state.sessions.clear();
        }
        state.pending = Some(PendingPairing {
            code: code.clone(),
            origin,
            expires_at,
            used: false,
            failed_attempts: 0,
        });
        PairingDisplay { code, expires_at }
    }

    /// Starts a code bound to a specific allowed origin.
    pub fn begin_pairing_for_origin(&self, origin: impl Into<String>) -> Result<PairingDisplay> {
        let origin = origin.into();
        self.ensure_origin_allowed(&origin)?;
        Ok(self.begin_pairing_inner(Some(origin), false))
    }

    /// Revoke all browser sessions and issue a fresh code in one state update.
    pub fn replace_pairing_for_origin(&self, origin: impl Into<String>) -> Result<PairingDisplay> {
        let origin = origin.into();
        self.ensure_origin_allowed(&origin)?;
        Ok(self.begin_pairing_inner(Some(origin), true))
    }

    /// Consumes a code and creates a session token bound to the browser origin.
    pub fn pair(&self, code: &str, origin: &str) -> Result<PairingSession> {
        self.ensure_origin_allowed(origin)?;
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let pending = state.pending.as_mut().ok_or(WebmcpError::PairingExpired)?;
        if pending.expires_at <= now {
            state.pending = None;
            return Err(WebmcpError::PairingExpired);
        }
        if pending.used {
            return Err(WebmcpError::PairingUsed);
        }
        if pending.code != code {
            pending.failed_attempts = pending.failed_attempts.saturating_add(1);
            if pending.failed_attempts >= MAX_FAILED_PAIRING_ATTEMPTS {
                state.pending = None;
            }
            return Err(WebmcpError::PairingExpired);
        }
        if pending.origin.as_deref().is_some_and(|expected| expected != origin) {
            return Err(WebmcpError::OriginRejected(origin.to_string()));
        }

        pending.used = true;
        let token = Uuid::new_v4().simple().to_string();
        let expires_at = now + self.ttl;
        drop(
            state
                .sessions
                .insert(token.clone(), PairingSessionState { origin: origin.to_string(), expires_at }),
        );
        Ok(PairingSession { token, origin: origin.to_string(), expires_at })
    }

    /// Validates a session token and its origin binding without extending it.
    pub fn validate(&self, token: &str, origin: &str) -> Result<()> {
        self.validate_session(token, origin, false)
    }

    /// Validates a session and extends its inactivity deadline.
    ///
    /// Pairing codes remain one-time and expire according to the configured
    /// TTL. An authenticated browser session may remain connected longer than
    /// that TTL as long as it continues making authenticated requests.
    pub fn refresh(&self, token: &str, origin: &str) -> Result<()> {
        self.validate_session(token, origin, true)
    }

    fn validate_session(&self, token: &str, origin: &str, refresh: bool) -> Result<()> {
        self.ensure_origin_allowed(origin)?;
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let valid = state
            .sessions
            .get(token)
            .is_some_and(|session| session.expires_at > now && session.origin == origin);
        if !valid {
            drop(state.sessions.remove(token));
            return Err(WebmcpError::Unauthorized);
        }
        if refresh && let Some(session) = state.sessions.get_mut(token) {
            session.expires_at = now + self.ttl;
        }
        Ok(())
    }

    /// Rehydrates an existing in-memory session for a reconnecting socket.
    pub fn resume(&self, token: &str, origin: &str) -> Result<PairingSession> {
        self.refresh(token, origin)?;
        let state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let session = state.sessions.get(token).ok_or(WebmcpError::Unauthorized)?;
        Ok(PairingSession {
            token: token.to_string(),
            origin: session.origin.clone(),
            expires_at: session.expires_at,
        })
    }

    /// Revokes one session token.
    pub fn revoke(&self, token: &str) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sessions.remove(token).is_some()
    }

    /// Revokes every active browser session and pending code.
    pub fn revoke_all(&self) {
        let mut state = self.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sessions.clear();
        state.pending = None;
    }

    /// Returns whether the origin is explicitly allowed.
    pub fn is_origin_allowed(&self, origin: &str) -> bool {
        self.allowed_origins.contains(origin)
    }

    fn ensure_origin_allowed(&self, origin: &str) -> Result<()> {
        if origin.is_empty() || !self.allowed_origins.contains(origin) {
            return Err(WebmcpError::OriginRejected(origin.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_is_one_time_and_origin_bound() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_secs(60)).expect("manager");
        let display = manager.begin_pairing();
        assert_eq!(display.code().len(), 12);
        let session = manager.pair(display.code(), "https://example.test").expect("pair");
        assert!(manager.validate(session.token(), "https://example.test").is_ok());
        assert!(matches!(manager.pair(display.code(), "https://example.test"), Err(WebmcpError::PairingUsed)));
        assert!(matches!(
            manager.validate(session.token(), "https://other.test"),
            Err(WebmcpError::OriginRejected(_))
        ));
    }

    #[test]
    fn configured_origins_can_keep_independent_sessions_active() {
        let manager = PairingManager::new(
            ["https://vtcode.vinhnx.chatgpt.site", "https://vinhnx.github.io"],
            Duration::from_secs(60),
        )
        .expect("manager");

        let chatgpt_pairing = manager
            .begin_pairing_for_origin("https://vtcode.vinhnx.chatgpt.site")
            .expect("ChatGPT Site pairing");
        let chatgpt_session = manager
            .pair(chatgpt_pairing.code(), "https://vtcode.vinhnx.chatgpt.site")
            .expect("ChatGPT Site session");

        let github_pairing = manager
            .begin_pairing_for_origin("https://vinhnx.github.io")
            .expect("GitHub Pages pairing");
        let github_session = manager
            .pair(github_pairing.code(), "https://vinhnx.github.io")
            .expect("GitHub Pages session");

        assert!(
            manager
                .validate(chatgpt_session.token(), "https://vtcode.vinhnx.chatgpt.site")
                .is_ok()
        );
        assert!(manager.validate(github_session.token(), "https://vinhnx.github.io").is_ok());
        assert!(matches!(
            manager.validate(chatgpt_session.token(), "https://vinhnx.github.io"),
            Err(WebmcpError::Unauthorized)
        ));
    }

    #[test]
    fn expired_pairing_cannot_be_reused() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_millis(1)).expect("manager");
        let display = manager.begin_pairing();
        std::thread::sleep(Duration::from_millis(3));
        assert!(matches!(manager.pair(display.code(), "https://example.test"), Err(WebmcpError::PairingExpired)));
    }

    #[test]
    fn revocation_invalidates_session() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_secs(60)).expect("manager");
        let display = manager.begin_pairing();
        let session = manager.pair(display.code(), "https://example.test").expect("pair");
        assert!(manager.revoke(session.token()));
        assert!(matches!(manager.validate(session.token(), "https://example.test"), Err(WebmcpError::Unauthorized)));
    }

    #[test]
    fn replacing_pairing_revokes_sessions_and_preserves_origin_validation() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_secs(60)).expect("manager");
        let display = manager.begin_pairing();
        let session = manager.pair(display.code(), "https://example.test").expect("pair");

        let replacement = manager
            .replace_pairing_for_origin("https://example.test")
            .expect("replacement pairing");
        assert!(matches!(manager.validate(session.token(), "https://example.test"), Err(WebmcpError::Unauthorized)));
        assert!(matches!(manager.pair(display.code(), "https://example.test"), Err(WebmcpError::PairingExpired)));
        assert!(manager.pair(replacement.code(), "https://example.test").is_ok());
    }

    #[test]
    fn rejected_replacement_does_not_revoke_current_session() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_secs(60)).expect("manager");
        let display = manager.begin_pairing();
        let session = manager.pair(display.code(), "https://example.test").expect("pair");

        assert!(matches!(
            manager.replace_pairing_for_origin("https://other.test"),
            Err(WebmcpError::OriginRejected(_))
        ));
        assert!(manager.validate(session.token(), "https://example.test").is_ok());
    }

    #[test]
    fn reconnect_can_resume_without_reusing_the_pairing_code() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_secs(60)).expect("manager");
        let display = manager.begin_pairing();
        let session = manager.pair(display.code(), "https://example.test").expect("pair");
        let resumed = manager.resume(session.token(), "https://example.test").expect("resume");
        assert_eq!(resumed.token(), session.token());
        assert!(matches!(manager.pair(display.code(), "https://example.test"), Err(WebmcpError::PairingUsed)));
    }

    #[test]
    fn wildcard_and_malformed_origins_are_rejected() {
        for origin in [
            "*",
            "https://*",
            " https://example.test ",
            "localhost",
            "ftp://example.test",
            "https://example.test/path",
            "https://user@example.test",
        ] {
            assert!(PairingManager::new([origin], Duration::from_secs(60)).is_err(), "accepted {origin}");
        }
    }

    #[test]
    fn repeated_invalid_codes_expire_the_pending_code() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_secs(60)).expect("manager");
        let display = manager.begin_pairing();
        for _ in 0..5 {
            assert!(matches!(manager.pair("000000000000", "https://example.test"), Err(WebmcpError::PairingExpired)));
        }
        assert!(matches!(manager.pair(display.code(), "https://example.test"), Err(WebmcpError::PairingExpired)));
    }

    #[test]
    fn refreshing_an_active_session_extends_only_its_inactivity_deadline() {
        let manager = PairingManager::new(["https://example.test"], Duration::from_millis(400)).expect("manager");
        let display = manager.begin_pairing();
        let session = manager.pair(display.code(), "https://example.test").expect("pair");

        std::thread::sleep(Duration::from_millis(150));
        manager.refresh(session.token(), "https://example.test").expect("refresh");
        std::thread::sleep(Duration::from_millis(150));
        assert!(manager.validate(session.token(), "https://example.test").is_ok());

        std::thread::sleep(Duration::from_millis(450));
        assert!(matches!(manager.validate(session.token(), "https://example.test"), Err(WebmcpError::Unauthorized)));
    }
}
