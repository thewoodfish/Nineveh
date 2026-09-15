//! Who's calling (ADR 0018): accounts signed in with GitHub, their sessions, and
//! project API keys.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, HeaderValue, USER_AGENT};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A session token's prefix: Studio's.
pub(crate) const SESSION_PREFIX: &str = "nvs_";
/// An API key's prefix: an app's, for one project.
pub(crate) const KEY_PREFIX: &str = "nvk_";
/// How long a session lasts.
pub(crate) const SESSION_DAYS: i32 = 30;

/// A new random token behind `prefix`, and the hash to store.
///
/// # Errors
///
/// If the OS has no randomness to give.
pub(crate) fn new_token(prefix: &str) -> Result<(String, [u8; 32]), AuthError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| AuthError::Random(e.to_string()))?;
    let token = format!("{prefix}{}", hex(&bytes));
    let hash = hash(&token);
    Ok((token, hash))
}

/// The SHA-256 of a presented token: what the store keeps and looks up.
#[must_use]
pub(crate) fn hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// A person as an identity provider knows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalUser {
    /// The provider's stable id for them.
    pub id: i64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
}

/// Where people sign in: GitHub in production, a fake in tests.
pub trait IdentityProvider: Send + Sync + 'static {
    /// The page to send someone to, carrying `state` back to the callback.
    fn authorize_url(&self, state: &str) -> String;

    /// The person a callback's `code` stands for.
    fn user<'a>(
        &'a self,
        code: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExternalUser, AuthError>> + Send + 'a>>;
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    #[error("the OS couldn't provide randomness: {0}")]
    Random(String),

    #[error("GitHub refused the sign-in: {0}")]
    Refused(String),

    #[error("couldn't reach GitHub: {0}")]
    Unreachable(String),

    #[error("GitHub answered in a way Nineveh doesn't understand: {0}")]
    Response(String),
}

impl AuthError {
    /// Only an unreachable GitHub can pass by trying again.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::Unreachable(_))
    }
}

/// How `nineveh up` is reached.
#[derive(Clone)]
pub enum Access {
    /// No sign-in; every project is reachable. Loopback only.
    Local,
    /// Sign-in with an identity provider; see ADR 0018.
    Hosted {
        provider: Arc<dyn IdentityProvider>,
        /// Where Studio is: sign-ins end at `{studio_url}/auth`.
        studio_url: String,
    },
}

impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => f.write_str("Local"),
            Self::Hosted { studio_url, .. } => f
                .debug_struct("Hosted")
                .field("studio_url", studio_url)
                .finish_non_exhaustive(),
        }
    }
}

/// GitHub's OAuth web flow, asking for no scopes: the public profile is enough.
pub struct GitHub {
    client_id: String,
    client_secret: SecretString,
    /// Where GitHub sends people back: `{public_url}/auth/github/callback`.
    redirect_uri: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for GitHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitHub")
            .field("client_id", &self.client_id)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

impl GitHub {
    /// A GitHub OAuth app's client, with the control plane reachable at `public_url`.
    ///
    /// # Errors
    ///
    /// If the HTTP client can't be built.
    pub fn new(
        client_id: String,
        client_secret: SecretString,
        public_url: &str,
    ) -> Result<Self, AuthError> {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| AuthError::Unreachable(e.to_string()))?
        .with_root_certificates(roots)
        .with_no_client_auth();
        let http = reqwest::Client::builder()
            .use_preconfigured_tls(tls)
            .user_agent(concat!("nineveh/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| AuthError::Unreachable(e.to_string()))?;
        Ok(Self {
            client_id,
            client_secret,
            redirect_uri: format!("{}/auth/github/callback", public_url.trim_end_matches('/')),
            http,
        })
    }

    async fn exchange(&self, code: &str) -> Result<ExternalUser, AuthError> {
        let unreachable = |e: reqwest::Error| AuthError::Unreachable(e.to_string());
        let token = self
            .http
            .post("https://github.com/login/oauth/access_token")
            .header(ACCEPT, "application/json")
            .json(&serde_json::json!({
                "client_id": self.client_id,
                "client_secret": self.client_secret.expose_secret(),
                "code": code,
                "redirect_uri": self.redirect_uri,
            }))
            .send()
            .await
            .map_err(unreachable)?
            .text()
            .await
            .map_err(unreachable)?;
        let token = SecretString::from(parse_token(&token)?);
        let mut bearer = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
            .map_err(|_| AuthError::Response("the token isn't a header value".into()))?;
        bearer.set_sensitive(true);
        let response = self
            .http
            .get("https://api.github.com/user")
            .header(AUTHORIZATION, bearer)
            .header(ACCEPT, "application/vnd.github+json")
            .header(USER_AGENT, concat!("nineveh/", env!("CARGO_PKG_VERSION")))
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
        let body = response.text().await.map_err(unreachable)?;
        if !status.is_success() {
            return Err(AuthError::Refused(format!("reading the user: {status}")));
        }
        parse_user(&body)
    }
}

impl IdentityProvider for GitHub {
    fn authorize_url(&self, state: &str) -> String {
        format!(
            "https://github.com/login/oauth/authorize?client_id={}&redirect_uri={}&state={}&allow_signup=true",
            encode(&self.client_id),
            encode(&self.redirect_uri),
            encode(state)
        )
    }

    fn user<'a>(
        &'a self,
        code: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExternalUser, AuthError>> + Send + 'a>> {
        Box::pin(self.exchange(code))
    }
}

/// Percent-encode a query value.
fn encode(value: &str) -> String {
    use std::fmt::Write as _;
    value.bytes().fold(String::new(), |mut out, b| {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
        out
    })
}

fn parse_token(body: &str) -> Result<String, AuthError> {
    #[derive(Deserialize)]
    struct Answer {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let answer: Answer =
        serde_json::from_str(body).map_err(|e| AuthError::Response(e.to_string()))?;
    match (answer.access_token, answer.error) {
        (Some(token), None) => Ok(token),
        (_, Some(error)) => Err(AuthError::Refused(
            answer.error_description.unwrap_or(error),
        )),
        (None, None) => Err(AuthError::Response("no access_token".into())),
    }
}

fn parse_user(body: &str) -> Result<ExternalUser, AuthError> {
    #[derive(Deserialize)]
    struct User {
        id: i64,
        login: String,
        name: Option<String>,
        avatar_url: Option<String>,
    }
    let user: User = serde_json::from_str(body).map_err(|e| AuthError::Response(e.to_string()))?;
    Ok(ExternalUser {
        id: user.id,
        login: user.login,
        name: user.name,
        avatar_url: user.avatar_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_stored_as_hashes() {
        let (a, hash_a) = new_token(KEY_PREFIX).unwrap();
        let (b, _) = new_token(KEY_PREFIX).unwrap();
        assert_ne!(a, b);
        assert!(a.starts_with("nvk_") && a.len() == 4 + 64, "{a}");
        assert_eq!(hash(&a), hash_a);
        assert_ne!(hash(&a), hash(&b));
    }

    #[test]
    fn reads_githubs_answers() {
        assert_eq!(
            parse_token(r#"{"access_token":"gho_x","token_type":"bearer","scope":""}"#).unwrap(),
            "gho_x"
        );
        assert!(matches!(
            parse_token(r#"{"error":"bad_verification_code","error_description":"The code passed is incorrect or expired."}"#),
            Err(AuthError::Refused(m)) if m.contains("expired")
        ));
        let user = parse_user(
            r#"{"login":"octocat","id":583231,"avatar_url":"https://avatars.githubusercontent.com/u/583231?v=4","name":"The Octocat","type":"User"}"#,
        )
        .unwrap();
        assert_eq!((user.id, user.login.as_str()), (583_231, "octocat"));
        assert_eq!(user.name.as_deref(), Some("The Octocat"));
    }

    #[test]
    fn the_authorize_url_carries_state_and_callback() {
        let github = GitHub::new(
            "Iv1.abc".into(),
            SecretString::from("secret"),
            "https://api.nineveh.dev/",
        )
        .unwrap();
        let url = github.authorize_url("s t");
        assert!(url.starts_with("https://github.com/login/oauth/authorize?client_id=Iv1.abc&"));
        assert!(
            url.contains("redirect_uri=https%3A%2F%2Fapi.nineveh.dev%2Fauth%2Fgithub%2Fcallback")
        );
        assert!(url.contains("state=s%20t"));
        assert!(!url.contains("secret"));
    }
}
