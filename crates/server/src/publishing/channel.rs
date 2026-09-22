use super::*;

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChannelDescriptionInput {
    /// Connected channel ID returned by get_youtube_channel. Prevents editing a different account.
    pub channel_id: String,
    /// Exact description returned by get_youtube_channel. Rejects stale edits; keep unchanged on retries.
    pub expected_description: String,
    /// New channel description, at most 1000 characters. Empty explicitly clears it.
    pub description: String,
}

impl Publisher {
    async fn connected_channel_id(&self) -> Result<String> {
        sqlx::query_scalar("SELECT channel_id FROM youtube_account WHERE id=1")
            .fetch_optional(&self.0.db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No YouTube channel connected"))
    }

    async fn channel_resource(&self, channel: &str) -> Result<(String, Value)> {
        let token = self.access_token(channel).await?;
        let response = self
            .0
            .client
            .get(&self.0.endpoints.channels)
            .timeout(Duration::from_secs(20))
            .query(&[("part", "snippet,brandingSettings"), ("mine", "true")])
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|_| anyhow::anyhow!("Could not read YouTube channel; retry the read"))?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if !response.status().is_success() {
            bail!(
                "YouTube channel lookup failed (HTTP {}); check authorization and retry",
                response.status().as_u16()
            );
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| anyhow::anyhow!("Invalid YouTube channel response"))?;
        let item = body["items"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["id"] == channel))
            .ok_or_else(|| {
                anyhow::anyhow!("Connected channel is not owned by the authorized account")
            })?;
        if !item["brandingSettings"]["channel"].is_object() {
            bail!("YouTube did not return channel branding settings; no update is safe");
        }
        description(item)?;
        Ok((token, item.clone()))
    }

    pub async fn youtube_channel(&self) -> Result<Value> {
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        let channel = self.connected_channel_id().await?;
        let (_, item) = self.channel_resource(&channel).await?;
        Ok(channel_projection(&item))
    }

    pub async fn set_channel_description(&self, input: ChannelDescriptionInput) -> Result<Value> {
        if input.description.chars().count() > 1000 {
            bail!("Channel description must be at most 1000 characters");
        }
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        let channel = self.connected_channel_id().await?;
        if channel != input.channel_id {
            bail!("Connected channel changed; read the intended channel before updating");
        }
        if self.setting("youtube_manage_channel").await?.as_deref() != Some(channel.as_str()) {
            bail!("YouTube management permission required; authorize video management in AgentWay");
        }
        let (token, item) = self.channel_resource(&channel).await?;
        let current = description(&item)?;
        // Also reconciles a successful write whose response was lost, without writing twice.
        if current == input.description {
            return Ok(
                json!({"status":"completed","verified":true,"channel":channel_projection(&item)}),
            );
        }
        if current != input.expected_description {
            bail!(
                "Channel description changed since it was read; read it again and resolve the change before updating"
            );
        }
        // channels.update replaces the whole part. Preserve all current supported channel
        // fields; deprecated watch/image fields must not be echoed (YouTube rejects them).
        let mut settings = item["brandingSettings"]["channel"].clone();
        settings["description"] = json!(input.description);
        let response = self
            .0
            .client
            .put(&self.0.endpoints.channels)
            .timeout(Duration::from_secs(20))
            .query(&[("part", "brandingSettings")])
            .bearer_auth(token)
            .json(&json!({"id":channel,"brandingSettings":{"channel":settings}}))
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "Channel update outcome uncertain; retry identical inputs to reconcile"
                )
            })?;
        if response.status().as_u16() == 401 {
            *self.0.access_token.lock().await = None;
        }
        if !response.status().is_success() {
            bail!(
                "YouTube channel update returned HTTP {}; retry identical inputs to reconcile before changing the request",
                response.status().as_u16()
            );
        }
        let (_, saved) = self.channel_resource(&channel).await.map_err(|_| {
            anyhow::anyhow!(
                "Channel update sent but readback failed; retry identical inputs to verify"
            )
        })?;
        if description(&saved)? != input.description {
            bail!(
                "Channel description readback does not match; read current state before retrying"
            );
        }
        Ok(json!({"status":"completed","verified":true,"channel":channel_projection(&saved)}))
    }
}

fn description(item: &Value) -> Result<&str> {
    match &item["brandingSettings"]["channel"]["description"] {
        Value::Null => Ok(""),
        Value::String(value) => Ok(value),
        _ => bail!("Invalid YouTube channel description"),
    }
}
fn channel_projection(item: &Value) -> Value {
    json!({"channel_id":item["id"],"title":item["snippet"]["title"],"description":item["brandingSettings"]["channel"]["description"].as_str().unwrap_or("")})
}
