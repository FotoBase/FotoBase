//! Google sign-in for the Photos plugin (TASK-08-012): OAuth 2.0 for installed
//! apps — system browser, PKCE (S256) and a loopback redirect — plus token
//! storage and refresh.
//!
//! The app opens [`AuthorizationRequest::url`] in the system browser, waits
//! for the redirect on a [`LoopbackReceiver`] and hands the code to
//! [`GoogleAuth::complete`]. Tokens live behind the [`TokenStore`] seam so
//! the OS keychain / credential store can hold them; this crate never writes
//! them anywhere else. Nothing here logs or prints a token, and a request
//! carrying one is never redirected to another host.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use super::{http_client, transport_error, GooglePhotosConfig};
use crate::ExportError;

/// What the plugin asks for: add photos (`appendonly`, the only way to
/// upload) and see what this app itself uploaded (needed to find its albums
/// again). Neither scope can read the user's other photos — the plugin is
/// upload-only by construction (PLAN §10).
pub const SCOPES: [&str; 2] = [
    "https://www.googleapis.com/auth/photoslibrary.appendonly",
    "https://www.googleapis.com/auth/photoslibrary.readonly.appcreateddata",
];

/// Refresh this long before the access token actually expires.
const EXPIRY_LEEWAY_SECS: u64 = 60;

/// The tokens of one signed-in Google account.
#[derive(Clone, PartialEq, Eq)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Seconds since the Unix epoch.
    pub expires_at_unix: u64,
}

impl OAuthTokens {
    /// For credential stores that keep one string per entry.
    pub fn to_json(&self) -> String {
        json!({
            "access_token": self.access_token,
            "refresh_token": self.refresh_token,
            "expires_at_unix": self.expires_at_unix,
        })
        .to_string()
    }

    pub fn from_json(text: &str) -> Result<Self, ExportError> {
        let invalid = || ExportError::Fatal("the stored Google sign-in is unreadable".into());
        let value: Value = serde_json::from_str(text).map_err(|_| invalid())?;
        Ok(Self {
            access_token: value["access_token"].as_str().ok_or_else(invalid)?.to_owned(),
            refresh_token: value["refresh_token"].as_str().ok_or_else(invalid)?.to_owned(),
            expires_at_unix: value["expires_at_unix"].as_u64().ok_or_else(invalid)?,
        })
    }
}

impl std::fmt::Debug for OAuthTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthTokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at_unix", &self.expires_at_unix)
            .finish()
    }
}

/// Where the tokens are kept — the OS keychain / credential store in the app.
pub trait TokenStore: Send + Sync {
    fn load(&self) -> Result<Option<OAuthTokens>, ExportError>;
    fn save(&self, tokens: &OAuthTokens) -> Result<(), ExportError>;
    fn clear(&self) -> Result<(), ExportError>;
}

/// Keeps the tokens in memory only: tests, and sessions that must not
/// remember the sign-in.
#[derive(Default)]
pub struct MemoryTokenStore {
    tokens: Mutex<Option<OAuthTokens>>,
}

impl MemoryTokenStore {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<OAuthTokens>> {
        self.tokens.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl TokenStore for MemoryTokenStore {
    fn load(&self) -> Result<Option<OAuthTokens>, ExportError> {
        Ok(self.lock().clone())
    }

    fn save(&self, tokens: &OAuthTokens) -> Result<(), ExportError> {
        *self.lock() = Some(tokens.clone());
        Ok(())
    }

    fn clear(&self) -> Result<(), ExportError> {
        *self.lock() = None;
        Ok(())
    }
}

/// A sign-in in progress: open [`AuthorizationRequest::url`] in the browser.
pub struct AuthorizationRequest {
    url: Url,
    state: String,
    verifier: String,
    redirect_uri: String,
}

impl AuthorizationRequest {
    /// The Google consent page to open in the system browser.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The `state` the redirect must echo back.
    pub fn state(&self) -> &str {
        &self.state
    }
}

impl std::fmt::Debug for AuthorizationRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorizationRequest")
            .field("redirect_uri", &self.redirect_uri)
            .field("verifier", &"<redacted>")
            .finish()
    }
}

#[derive(Clone)]
pub struct GoogleAuth {
    config: GooglePhotosConfig,
    store: Arc<dyn TokenStore>,
    client: Client,
}

impl GoogleAuth {
    pub fn new(
        config: GooglePhotosConfig,
        store: Arc<dyn TokenStore>,
    ) -> Result<Self, ExportError> {
        Ok(Self { config, store, client: http_client()? })
    }

    pub fn is_signed_in(&self) -> Result<bool, ExportError> {
        Ok(self.store.load()?.is_some())
    }

    /// Forgets the tokens. (The grant itself can be withdrawn in the Google
    /// account settings.)
    pub fn sign_out(&self) -> Result<(), ExportError> {
        self.store.clear()
    }

    /// Starts a sign-in whose browser redirect lands on `redirect_uri`
    /// (normally [`LoopbackReceiver::redirect_uri`]).
    pub fn authorization_request(
        &self,
        redirect_uri: &str,
    ) -> Result<AuthorizationRequest, ExportError> {
        Url::parse(redirect_uri)
            .map_err(|_| ExportError::InvalidConfig("the redirect address is not a URL".into()))?;
        let verifier = random_token(32)?;
        let state = random_token(16)?;
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

        let mut url = self.config.auth_endpoint().clone();
        url.query_pairs_mut()
            .append_pair("client_id", self.config.client_id())
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("response_type", "code")
            .append_pair("scope", &SCOPES.join(" "))
            .append_pair("state", &state)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("access_type", "offline")
            .append_pair("prompt", "consent");
        Ok(AuthorizationRequest { url, state, verifier, redirect_uri: redirect_uri.to_owned() })
    }

    /// Finishes a sign-in: checks the echoed `state`, exchanges `code` for
    /// tokens and keeps them only if Google granted every scope the plugin
    /// needs.
    pub async fn complete(
        &self,
        request: &AuthorizationRequest,
        code: &str,
        state: &str,
    ) -> Result<(), ExportError> {
        if state != request.state {
            return Err(ExportError::Fatal(
                "the sign-in answer did not belong to this request; start the sign-in again".into(),
            ));
        }
        let answer = self
            .token_request(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &request.redirect_uri),
                ("code_verifier", &request.verifier),
            ])
            .await?;

        let granted = answer["scope"].as_str().unwrap_or_default();
        if !SCOPES.iter().all(|needed| granted.split(' ').any(|s| s == *needed)) {
            return Err(ExportError::Fatal(
                "Google Photos access was not fully granted: allow Fotobase to add photos and \
                 to see the photos it adds, then sign in again"
                    .into(),
            ));
        }
        let refresh_token = answer["refresh_token"].as_str().ok_or_else(|| {
            ExportError::Fatal(
                "Google did not allow Fotobase to stay signed in; sign in again".into(),
            )
        })?;
        self.store.save(&OAuthTokens {
            access_token: access_token_of(&answer)?,
            refresh_token: refresh_token.to_owned(),
            expires_at_unix: expiry_of(&answer),
        })
    }

    /// A usable access token, refreshed first when it is (about to be)
    /// expired or `force_refresh` says Google no longer accepts it.
    pub(crate) async fn access_token(&self, force_refresh: bool) -> Result<String, ExportError> {
        let Some(tokens) = self.store.load()? else {
            return Err(ExportError::Fatal("Google Photos is not connected; sign in first".into()));
        };
        if !force_refresh && tokens.expires_at_unix > unix_now() + EXPIRY_LEEWAY_SECS {
            return Ok(tokens.access_token);
        }
        let answer = self
            .token_request(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &tokens.refresh_token),
            ])
            .await
            .map_err(|e| match e {
                ExportError::Fatal(_) if is_revoked(&e) => ExportError::Fatal(
                    "Google no longer accepts the saved sign-in (access revoked or expired); \
                     sign in again"
                        .into(),
                ),
                other => other,
            })?;
        let refreshed = OAuthTokens {
            access_token: access_token_of(&answer)?,
            refresh_token: answer["refresh_token"]
                .as_str()
                .map_or(tokens.refresh_token, str::to_owned),
            expires_at_unix: expiry_of(&answer),
        };
        self.store.save(&refreshed)?;
        Ok(refreshed.access_token)
    }

    async fn token_request(&self, params: &[(&str, &str)]) -> Result<Value, ExportError> {
        let body = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("client_id", self.config.client_id());
            if let Some(secret) = self.config.client_secret() {
                form.append_pair("client_secret", secret);
            }
            for (name, value) in params {
                form.append_pair(name, value);
            }
            form.finish()
        };
        let response = self
            .client
            .post(self.config.token_endpoint().clone())
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|e| transport_error(e, "Google sign-in"))?;

        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if status.is_success() {
            return Ok(body);
        }
        let error = body["error"].as_str().unwrap_or("unknown_error");
        Err(match status {
            s if s.is_redirection() => ExportError::Fatal(
                "Google's sign-in address redirected; Fotobase does not follow redirects".into(),
            ),
            StatusCode::REQUEST_TIMEOUT | StatusCode::TOO_MANY_REQUESTS => {
                ExportError::Transient(format!("Google sign-in is busy (HTTP {status})"))
            }
            s if s.is_server_error() => {
                ExportError::Transient(format!("Google sign-in failed (HTTP {status})"))
            }
            _ if error == "invalid_grant" => {
                ExportError::Fatal(format!("Google rejected the sign-in ({error}); sign in again"))
            }
            _ if error == "invalid_client" => ExportError::Fatal(
                "Google rejected the app's client id or secret (invalid_client)".into(),
            ),
            _ => ExportError::Fatal(format!("Google sign-in failed ({error}, HTTP {status})")),
        })
    }
}

fn is_revoked(error: &ExportError) -> bool {
    error.to_string().contains("invalid_grant")
}

fn access_token_of(answer: &Value) -> Result<String, ExportError> {
    answer["access_token"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| ExportError::Fatal("Google answered the sign-in without a token".into()))
}

fn expiry_of(answer: &Value) -> u64 {
    unix_now() + answer["expires_in"].as_u64().unwrap_or(3600)
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// URL-safe random string from `bytes` bytes of OS randomness.
fn random_token(bytes: usize) -> Result<String, ExportError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf)
        .map_err(|e| ExportError::Fatal(format!("no secure random numbers available: {e}")))?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

// ---------------------------------------------------------- loopback receiver

const PAGE: &str = "<!doctype html><meta charset=utf-8><title>Fotobase</title>\
<body style=\"font-family:sans-serif;text-align:center;margin-top:4em\">\
<h2>Fotobase is connected to Google Photos</h2>\
<p>It can add photos to Google Photos (upload only). You can close this tab.</p>";
const PAGE_DENIED: &str = "<!doctype html><meta charset=utf-8><title>Fotobase</title>\
<body style=\"font-family:sans-serif;text-align:center;margin-top:4em\">\
<h2>Google Photos was not connected</h2><p>You can close this tab and try again in Fotobase.</p>";

/// Receives the browser redirect of the sign-in on `127.0.0.1` (a random
/// port), the way Google's installed-app flow expects.
pub struct LoopbackReceiver {
    listener: TcpListener,
    redirect_uri: String,
}

enum Visit {
    Code(String),
    Denied(String),
    Other,
}

impl LoopbackReceiver {
    pub async fn bind() -> Result<Self, ExportError> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let redirect_uri = format!("http://127.0.0.1:{}/", listener.local_addr()?.port());
        Ok(Self { listener, redirect_uri })
    }

    /// The redirect address to give [`GoogleAuth::authorization_request`].
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// Waits for the redirect carrying `expected_state` and returns its
    /// authorization code. Stray requests (favicons, other states) are
    /// answered and ignored.
    pub async fn wait_for_code(
        self,
        expected_state: &str,
        timeout: Duration,
    ) -> Result<String, ExportError> {
        let accept = async {
            loop {
                let (mut stream, _) = self.listener.accept().await?;
                let visit = tokio::time::timeout(
                    Duration::from_secs(5),
                    handle_visit(&mut stream, expected_state),
                )
                .await
                .unwrap_or(Visit::Other);
                match visit {
                    Visit::Code(code) => return Ok(code),
                    Visit::Denied(reason) => {
                        return Err(ExportError::Fatal(format!(
                            "Google sign-in was denied or cancelled ({reason})"
                        )))
                    }
                    Visit::Other => {}
                }
            }
        };
        tokio::time::timeout(timeout, accept).await.unwrap_or_else(|_| {
            Err(ExportError::Fatal("timed out waiting for the Google sign-in to finish".into()))
        })
    }
}

async fn handle_visit(stream: &mut TcpStream, expected_state: &str) -> Visit {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head.lines().next().and_then(|l| {
        let mut parts = l.split(' ');
        (parts.next() == Some("GET")).then(|| parts.next()).flatten()
    });
    let params: Option<(String, BTreeMap<String, String>)> = target
        .and_then(|t| Url::parse(&format!("http://127.0.0.1{t}")).ok())
        .map(|u| (u.path().to_owned(), u.query_pairs().into_owned().collect()));

    let (status, page, visit) = match params {
        Some((path, q))
            if path == "/" && q.get("state").map(String::as_str) == Some(expected_state) =>
        {
            match (q.get("code"), q.get("error")) {
                (Some(code), _) => (200, PAGE, Visit::Code(code.clone())),
                (None, Some(error)) => (400, PAGE_DENIED, Visit::Denied(error.clone())),
                (None, None) => (400, PAGE_DENIED, Visit::Other),
            }
        }
        Some((path, _)) if path == "/" => (400, PAGE_DENIED, Visit::Other),
        _ => (404, "", Visit::Other),
    };
    let reply = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
    visit
}
