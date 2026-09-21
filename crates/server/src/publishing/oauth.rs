use super::*;
use axum::http::HeaderMap;
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient,
};
type Client = BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;
const UPLOAD: &str = "https://www.googleapis.com/auth/youtube.upload";
const READ: &str = "https://www.googleapis.com/auth/youtube.readonly";
impl Publisher {
    async fn oauth_client(&self) -> Result<Client> {
        Ok(
            BasicClient::new(ClientId::new(self.secret("client_id").await?))
                .set_client_secret(ClientSecret::new(self.secret("client_secret").await?))
                .set_auth_uri(AuthUrl::new(
                    "https://accounts.google.com/o/oauth2/v2/auth".into(),
                )?)
                .set_token_uri(TokenUrl::new(self.0.endpoints.token.clone())?)
                .set_auth_type(oauth2::AuthType::RequestBody),
        )
    }
    pub async fn authorize(&self, redirect: String) -> Result<(String, String)> {
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let client = self
            .oauth_client()
            .await?
            .set_redirect_uri(RedirectUrl::new(redirect.clone())?);
        let (url, state) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(UPLOAD.into()))
            .add_scope(Scope::new(READ.into()))
            .set_pkce_challenge(challenge)
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .url();
        sqlx::query("DELETE FROM oauth_attempts WHERE expires_at<unixepoch()")
            .execute(&self.0.db)
            .await?;
        sqlx::query("INSERT INTO oauth_attempts(state,verifier,redirect_uri,expires_at) VALUES(?,?,?,unixepoch()+600)")
            .bind(state.secret()).bind(self.0.vault.seal(verifier.secret())?).bind(redirect).execute(&self.0.db).await?;
        Ok((url.to_string(), state.secret().clone()))
    }
    pub async fn oauth_callback(
        &self,
        state: &str,
        code: Option<&str>,
        headers: &HeaderMap,
    ) -> Result<()> {
        let cookie = headers
            .get("cookie")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .split(';')
            .map(str::trim)
            .find_map(|s| s.strip_prefix("agentway_oauth="));
        if cookie != Some(state) || state.is_empty() {
            bail!("Authorization session does not match. Start Connect YouTube again.");
        }
        let _guard = self.0.mutation.lock().await;
        let attempt: Option<(String,String)> = sqlx::query_as("DELETE FROM oauth_attempts WHERE state=? AND expires_at>=unixepoch() RETURNING verifier,redirect_uri")
            .bind(state).fetch_optional(&self.0.db).await?;
        let (verifier, redirect) = attempt.ok_or_else(|| {
            anyhow::anyhow!("Authorization expired or already used. Start Connect YouTube again.")
        })?;
        let code = code.ok_or_else(|| {
            anyhow::anyhow!("YouTube authorization was declined. You can try connecting again.")
        })?;
        let token = self.oauth_client().await?.set_redirect_uri(RedirectUrl::new(redirect)?)
            .exchange_code(AuthorizationCode::new(code.into()))
            .set_pkce_verifier(PkceCodeVerifier::new(self.0.vault.open_secret(&verifier)?))
            .request_async(&self.0.client).await.map_err(|_| anyhow::anyhow!("Google could not complete authorization. Check the client credentials and try connecting again."))?;
        if let Some(scopes) = token.scopes()
            && ![UPLOAD, READ]
                .iter()
                .all(|needed| scopes.iter().any(|s| s.as_str() == *needed))
        {
            bail!("Allow both upload and channel-read permissions when connecting YouTube");
        }
        let refresh = token.refresh_token().ok_or_else(|| {
            anyhow::anyhow!("Google did not return offline access. Reconnect and grant consent.")
        })?;
        let response = self
            .0
            .client
            .get(&self.0.endpoints.channels)
            .query(&[("part", "snippet"), ("mine", "true")])
            .bearer_auth(token.access_token().secret())
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Could not contact YouTube to identify your channel"))?;
        if !response.status().is_success() {
            bail!(
                "Could not read your YouTube channel. Enable YouTube Data API v3 and grant channel-read access."
            );
        }
        let channel: Value = response.json().await?;
        let items = channel["items"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("YouTube returned no channels"))?;
        if items.len() != 1 {
            bail!(
                "Select a Google or Brand Account with exactly one YouTube channel and reconnect"
            );
        }
        let id = items[0]["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("YouTube channel ID missing"))?;
        let name = items[0]["snippet"]["title"].as_str().unwrap_or(id);
        sqlx::query("INSERT INTO youtube_account(id,channel_id,channel_name,refresh_token) VALUES(1,?,?,?) ON CONFLICT(id) DO UPDATE SET channel_id=excluded.channel_id,channel_name=excluded.channel_name,refresh_token=excluded.refresh_token")
            .bind(id).bind(name).bind(self.0.vault.seal(refresh.secret())?).execute(&self.0.db).await?;
        *self.0.access_token.lock().await = None;
        self.emit("youtube.status", self.status().await?).await?;
        Ok(())
    }
    pub async fn access_token(&self, channel_id: &str) -> Result<String> {
        let secret: Option<String> = sqlx::query_scalar(
            "SELECT refresh_token FROM youtube_account WHERE id=1 AND channel_id=?",
        )
        .bind(channel_id)
        .fetch_optional(&self.0.db)
        .await?;
        let encrypted = secret.ok_or_else(|| {
            anyhow::anyhow!("Reconnect the original YouTube channel before resuming this upload")
        })?;
        let mut cached = self.0.access_token.lock().await;
        if let Some(token) = cached.as_ref()
            && token.channel == channel_id
            && token.expires > std::time::Instant::now()
        {
            return Ok(token.value.clone());
        }
        let refresh = self.0.vault.open_secret(&encrypted)?;
        let token = self.oauth_client().await?.exchange_refresh_token(&RefreshToken::new(refresh))
            .request_async(&self.0.client).await.map_err(|_| anyhow::anyhow!("Google authorization expired or could not be refreshed. Reconnect YouTube, then retry this upload."))?;
        let value = token.access_token().secret().clone();
        let lifetime = token
            .expires_in()
            .unwrap_or(Duration::from_secs(60))
            .saturating_sub(Duration::from_secs(30));
        *cached = Some(CachedToken {
            channel: channel_id.into(),
            value: value.clone(),
            expires: std::time::Instant::now() + lifetime,
        });
        Ok(value)
    }
    pub async fn disconnect(&self) -> Result<()> {
        let _guard = self.0.mutation.lock().await;
        // Delete locally even if Google is unreachable. The UI explains external revocation.
        sqlx::query("DELETE FROM youtube_account")
            .execute(&self.0.db)
            .await?;
        *self.0.access_token.lock().await = None;
        self.emit("youtube.status", self.status().await?).await?;
        Ok(())
    }
}
