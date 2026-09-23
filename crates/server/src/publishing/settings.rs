use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum License {
    Youtube,
    CreativeCommon,
}
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Translation {
    pub title: String,
    pub description: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoSettings {
    /// YouTube category ID from list_youtube_categories. Omission on upload retains legacy 24 (Entertainment).
    pub category_id: Option<String>,
    /// At most 500 characters including separators and quotes around tags containing spaces. [] clears tags.
    pub tags: Option<Vec<String>>,
    /// BCP-47 language of title/description.
    pub default_language: Option<String>,
    /// BCP-47 language of the original audio.
    pub default_audio_language: Option<String>,
    /// Future RFC3339 timestamp with timezone. Requires private visibility; YouTube publishes automatically.
    pub publish_at: Option<String>,
    /// Explicitly cancel an existing schedule. Update only; cannot accompany publish_at.
    #[serde(default)]
    pub clear_schedule: bool,
    pub embeddable: Option<bool>,
    pub license: Option<License>,
    pub public_stats_viewable: Option<bool>,
    /// Whether this video contains paid product placement, sponsorship or endorsement.
    pub paid_product_placement: Option<bool>,
    /// RFC3339 recording date with timezone.
    pub recording_date: Option<String>,
    /// Complete language-keyed translation map; {} clears translations. Requires default_language on upload.
    pub localizations: Option<BTreeMap<String, Translation>>,
}
pub(super) fn text_valid(title: &str, description: &str) -> Result<()> {
    if title.trim().is_empty()
        || title.chars().count() > 100
        || title.contains(['<', '>'])
        || description.len() > 5000
        || description.contains(['<', '>'])
    {
        bail!(
            "Title must contain 1–100 characters; description at most 5000 UTF-8 bytes; neither may contain < or >"
        );
    }
    Ok(())
}
fn language(value: &str) -> Result<()> {
    value
        .parse::<language_tags::LanguageTag>()
        .map_err(|_| anyhow::anyhow!("Language must be a valid BCP-47 tag"))?;
    Ok(())
}
impl VideoSettings {
    pub(super) fn validate(&self, privacy: &str, upload: bool) -> Result<()> {
        if let Some(category) = &self.category_id
            && (category.is_empty()
                || category.len() > 10
                || !category.bytes().all(|b| b.is_ascii_digit()))
        {
            bail!("category_id must be a YouTube category ID");
        }
        if let Some(tags) = &self.tags {
            let count = tags
                .iter()
                .map(|v| v.chars().count() + if v.contains(' ') { 2 } else { 0 })
                .sum::<usize>()
                + tags.len().saturating_sub(1);
            if count > 500
                || tags
                    .iter()
                    .any(|v| v.trim().is_empty() || v.contains(['<', '>']))
            {
                bail!(
                    "Invalid tags: maximum 500 characters including separators and quotes; no empty tags or < >"
                );
            }
        }
        for value in [&self.default_language, &self.default_audio_language]
            .into_iter()
            .flatten()
        {
            language(value)?;
        }
        if self.clear_schedule && (upload || self.publish_at.is_some()) {
            bail!("clear_schedule is update-only and cannot accompany publish_at");
        }
        if let Some(at) = &self.publish_at {
            let at = chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| anyhow::anyhow!("publish_at must be RFC3339 with timezone"))?;
            if privacy != "private" || at <= chrono::Utc::now() {
                bail!("Scheduling requires private visibility and a future publish_at");
            }
        }
        if let Some(at) = &self.recording_date {
            chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| anyhow::anyhow!("recording_date must be RFC3339 with timezone"))?;
        }
        if let Some(values) = &self.localizations {
            if values.len() > 100 {
                bail!("At most 100 translations per request");
            }
            if upload && !values.is_empty() && self.default_language.is_none() {
                bail!("Translations require default_language");
            }
            for (lang, value) in values {
                language(lang)?;
                text_valid(&value.title, &value.description)?;
            }
        }
        Ok(())
    }
    fn parts(&self) -> Result<Value> {
        let mut body = json!({});
        for (part, key, value) in [
            ("snippet", "categoryId", json!(self.category_id)),
            ("snippet", "tags", json!(self.tags)),
            ("snippet", "defaultLanguage", json!(self.default_language)),
            (
                "snippet",
                "defaultAudioLanguage",
                json!(self.default_audio_language),
            ),
            ("status", "publishAt", json!(self.publish_at)),
            ("status", "embeddable", json!(self.embeddable)),
            ("status", "license", json!(self.license)),
            (
                "status",
                "publicStatsViewable",
                json!(self.public_stats_viewable),
            ),
            (
                "paidProductPlacementDetails",
                "hasPaidProductPlacement",
                json!(self.paid_product_placement),
            ),
            (
                "recordingDetails",
                "recordingDate",
                json!(self.recording_date),
            ),
        ] {
            if !value.is_null() {
                if body.get(part).is_none() {
                    body[part] = json!({});
                }
                body[part][key] = value;
            }
        }
        if let Some(localizations) = &self.localizations {
            body["localizations"] = serde_json::to_value(localizations)?;
        }
        if self.clear_schedule && body.get("status").is_none() {
            body["status"] = json!({});
        }
        Ok(body)
    }
}
impl PublishInput {
    pub(super) fn youtube_metadata(&self) -> Result<Value> {
        let mut body = self.settings.parts()?;
        if body.get("snippet").is_none() {
            body["snippet"] = json!({});
        }
        if body.get("status").is_none() {
            body["status"] = json!({});
        }
        body["snippet"]["title"] = json!(self.title);
        body["snippet"]["description"] = json!(self.description);
        if self.settings.category_id.is_none() {
            body["snippet"]["categoryId"] = json!("24");
        }
        body["status"]["privacyStatus"] = json!(self.privacy);
        body["status"]["selfDeclaredMadeForKids"] = json!(self.made_for_kids);
        body["status"]["containsSyntheticMedia"] = json!(self.contains_synthetic_media);
        Ok(body)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VideoUpdate {
    pub request_id: String,
    /// AgentWay publication ID, not the YouTube video ID.
    pub publication_id: String,
    /// Exact etag from get_youtube_video; prevents overwriting changes since the read.
    pub expected_etag: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub privacy: Option<String>,
    pub made_for_kids: Option<bool>,
    pub contains_synthetic_media: Option<bool>,
    #[serde(default)]
    pub settings: VideoSettings,
}
// Only documented mutable properties are echoed. Read-only provider fields must not be submitted.
const SNIPPET: &[&str] = &[
    "title",
    "description",
    "categoryId",
    "tags",
    "defaultLanguage",
    "defaultAudioLanguage",
];
const STATUS: &[&str] = &[
    "privacyStatus",
    "publishAt",
    "embeddable",
    "license",
    "publicStatsViewable",
    "selfDeclaredMadeForKids",
    "containsSyntheticMedia",
];
fn merge_update(input: &VideoUpdate, item: &Value) -> Result<Value> {
    let mut patch = input.settings.parts()?;
    for (part, key, value) in [
        ("snippet", "title", json!(input.title)),
        ("snippet", "description", json!(input.description)),
        ("status", "privacyStatus", json!(input.privacy)),
        (
            "status",
            "selfDeclaredMadeForKids",
            json!(input.made_for_kids),
        ),
        (
            "status",
            "containsSyntheticMedia",
            json!(input.contains_synthetic_media),
        ),
    ] {
        if !value.is_null() {
            if patch.get(part).is_none() {
                patch[part] = json!({});
            }
            patch[part][key] = value;
        }
    }
    if patch.as_object().unwrap().is_empty() {
        bail!("Supply at least one video setting to update");
    }
    for (part, keys) in [
        ("snippet", SNIPPET),
        ("status", STATUS),
        ("recordingDetails", &["recordingDate"][..]),
        (
            "paidProductPlacementDetails",
            &["hasPaidProductPlacement"][..],
        ),
    ] {
        if let Some(fields) = patch.get_mut(part).and_then(Value::as_object_mut) {
            for key in keys {
                if !fields.contains_key(*key)
                    && let Some(value) = item[part].get(*key)
                {
                    fields.insert((*key).into(), value.clone());
                }
            }
        }
    }
    if input.settings.clear_schedule {
        patch["status"].as_object_mut().unwrap().remove("publishAt");
    }
    if patch.get("snippet").is_some() {
        text_valid(
            patch["snippet"]["title"].as_str().unwrap_or(""),
            patch["snippet"]["description"].as_str().unwrap_or(""),
        )?;
        if !patch["snippet"]["categoryId"].is_string() {
            bail!("YouTube did not return category; no safe update possible");
        }
    }
    if input.settings.publish_at.is_some() && item["status"]["privacyStatus"] != "private" {
        bail!(
            "Only an existing private video can be scheduled; YouTube also requires it never to have been published"
        );
    }
    if patch["status"].get("publishAt").is_some() && patch["status"]["privacyStatus"] != "private" {
        bail!("Cancel the schedule explicitly with clear_schedule before changing visibility");
    }
    if input
        .settings
        .localizations
        .as_ref()
        .is_some_and(|v| !v.is_empty())
        && input.settings.default_language.is_none()
        && !item["snippet"]["defaultLanguage"].is_string()
    {
        bail!("Translations require default_language");
    }
    patch["id"] = item["id"].clone();
    Ok(patch)
}
fn matches(body: &Value, item: &Value) -> bool {
    body.as_object().unwrap().iter().all(|(part, fields)| {
        if part == "id" {
            return item[part] == *fields;
        }
        if part == "localizations" {
            return item[part] == *fields || (fields == &json!({}) && item[part].is_null());
        }
        fields.as_object().unwrap().iter().all(|(key, value)| {
            if part == "snippet" && key == "tags" {
                // YouTube can reorder tags. Preserve multiplicity but ignore order.
                let sorted = |v: &Value| -> Option<Vec<String>> {
                    let mut values = v
                        .as_array()?
                        .iter()
                        .map(|v| v.as_str().map(str::to_owned))
                        .collect::<Option<Vec<_>>>()?;
                    values.sort_unstable();
                    Some(values)
                };
                return (value == &json!([]) && item[part][key].is_null())
                    || (sorted(value).is_some() && sorted(value) == sorted(&item[part][key]));
            }
            if ["publishAt", "recordingDate"].contains(&key.as_str()) {
                let parse = |v: &Value| {
                    v.as_str()
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                };
                return parse(value).is_some() && parse(value) == parse(&item[part][key]);
            }
            item[part][key] == *value || (value == &json!([]) && item[part][key].is_null())
        })
    })
}
impl Publisher {
    pub(super) async fn check_pending_settings(&self, id: &str) -> Result<()> {
        let pending:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM youtube_settings_operations WHERE publication_id=? AND status IN ('outcome_unknown','verification_pending'))").bind(id).fetch_one(&self.0.db).await?;
        if pending {
            bail!(
                "An earlier settings write is unresolved; reconcile its request_id before another video mutation"
            );
        }
        Ok(())
    }

    pub async fn update_video_settings(&self, input: VideoUpdate) -> Result<Value> {
        if Uuid::parse_str(&input.request_id).is_err() || input.expected_etag.is_empty() {
            bail!("Provide a UUID request_id and expected_etag from get_youtube_video");
        }
        if let Some(privacy) = &input.privacy
            && !["private", "unlisted", "public"].contains(&privacy.as_str())
        {
            bail!("Invalid privacy");
        }
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        let encoded = serde_json::to_string(&input)?;
        if let Some((saved, owner)) = sqlx::query_as::<_, (String, String)>(
            "SELECT input,agent_id FROM youtube_settings_operations WHERE request_id=?",
        )
        .bind(&input.request_id)
        .fetch_optional(&self.0.db)
        .await?
        {
            if saved != encoded || owner != self.connection_id() {
                bail!("request_id belongs to another operation");
            }
            return self.reconcile_settings(&input.request_id).await;
        }
        let (token, item) = self.owned_video(&input.publication_id).await?;
        let channel = item["snippet"]["channelId"].as_str().unwrap_or("");
        if self.setting("youtube_manage_channel").await?.as_deref() != Some(channel) {
            bail!("YouTube management permission required");
        }
        if item["etag"] != input.expected_etag {
            bail!(
                "Video changed since it was read; read current settings and resolve before updating"
            );
        }
        input.settings.validate(
            input
                .privacy
                .as_deref()
                .or(item["status"]["privacyStatus"].as_str())
                .unwrap_or(""),
            false,
        )?;
        if (input.privacy.as_deref().is_some_and(|v| v != "private")
            || input.settings.publish_at.is_some())
            && self.setting("private_only").await?.as_deref() != Some("false")
        {
            bail!("Owner's private-only policy prevents publishing or scheduling");
        }
        let body = merge_update(&input, &item)?;
        self.check_pending_settings(&input.publication_id).await?;
        sqlx::query("INSERT INTO youtube_settings_operations(request_id,publication_id,agent_id,input,desired,status) VALUES(?,?,?,?,?,'outcome_unknown')").bind(&input.request_id).bind(&input.publication_id).bind(self.connection_id()).bind(encoded).bind(body.to_string()).execute(&self.0.db).await?;
        let parts = body
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| k.as_str() != "id")
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        let result = self
            .0
            .client
            .put(&self.0.endpoints.videos)
            .query(&[("part", parts)])
            .header("If-Match", &input.expected_etag)
            .bearer_auth(token)
            .timeout(Duration::from_secs(20))
            .json(&body)
            .send()
            .await;
        let (status,error)=match result {
            Ok(response) if response.status().is_success() => ("verification_pending",None),
            Ok(response) => { let code=response.status().as_u16(); if code==401{*self.0.access_token.lock().await=None;} (if (400..500).contains(&code){"rejected"}else{"outcome_unknown"},Some(provider_error("settings update", response).await)) },
            Err(_) => ("outcome_unknown",Some("YouTube settings response lost or timed out; retry the SAME request_id for read-only reconciliation".into())),
        };
        sqlx::query("UPDATE youtube_settings_operations SET status=?,error=? WHERE request_id=?")
            .bind(status)
            .bind(error)
            .bind(&input.request_id)
            .execute(&self.0.db)
            .await?;
        self.reconcile_settings(&input.request_id).await
    }
    async fn reconcile_settings(&self, id: &str) -> Result<Value> {
        let (publication,desired,status): (String,String,String)=sqlx::query_as("SELECT publication_id,desired,status FROM youtube_settings_operations WHERE request_id=?").bind(id).fetch_one(&self.0.db).await?;
        if ["outcome_unknown", "verification_pending"].contains(&status.as_str()) {
            let desired: Value = serde_json::from_str(&desired)?;
            if let Ok((_, item)) = self.owned_video(&publication).await {
                let clears_schedule =
                    desired.get("status").is_some() && desired["status"].get("publishAt").is_none();
                if matches(&desired, &item)
                    && (!clears_schedule || item["status"].get("publishAt").is_none())
                {
                    sqlx::query("UPDATE youtube_settings_operations SET status='completed',error=NULL WHERE request_id=?").bind(id).execute(&self.0.db).await?;
                }
            }
        }
        self.saved_settings_operation(id).await
    }
    pub async fn settings_operation(&self, id: &str) -> Result<Value> {
        let _guard = self.0.mutation.lock().await;
        self.check_publish_access().await?;
        // Authorize the record before making any provider request or changing it.
        self.saved_settings_operation(id).await?;
        self.reconcile_settings(id).await
    }
    async fn saved_settings_operation(&self, id: &str) -> Result<Value> {
        let (publication,owner,status,error):(String,String,String,Option<String>)=sqlx::query_as("SELECT publication_id,agent_id,status,error FROM youtube_settings_operations WHERE request_id=?").bind(id).fetch_one(&self.0.db).await?;
        if self.1.is_some() && owner != self.connection_id() {
            bail!("Operation belongs to another connection");
        }
        Ok(
            json!({"request_id":id,"publication_id":publication,"status":status,"verified":status=="completed","error":error,"retry_after_seconds":5}),
        )
    }
}

// Provider bodies may contain sensitive data; retain only bounded machine reason codes.
pub(super) async fn provider_error(action: &str, response: reqwest::Response) -> String {
    let code = response.status().as_u16();
    let value = response.json::<Value>().await.unwrap_or(Value::Null);
    let reason = value["error"]["errors"][0]["reason"]
        .as_str()
        .filter(|s| s.len() <= 80 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .unwrap_or("unspecified");
    format!("YouTube {action} returned HTTP {code} ({reason})")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tag_readback_ignores_order_but_detects_changed_values() {
        let desired = json!({"snippet":{"tags":["agentway","v12-test","updated"]}});
        assert!(matches(
            &desired,
            &json!({"snippet":{"tags":["agentway","updated","v12-test"]}})
        ));
        assert!(!matches(
            &desired,
            &json!({"snippet":{"tags":["agentway","updated"]}})
        ));
        assert!(!matches(
            &desired,
            &json!({"snippet":{"tags":["agentway","updated","other"]}})
        ));
        assert!(!matches(&desired, &json!({"snippet":{}})));
        assert!(!matches(
            &json!({"status":{"containsSyntheticMedia":true}}),
            &json!({"status":{}})
        ));
    }
    #[test]
    fn schedule_cancellation_preserves_disclosures_and_snippet_edits_preserve_translations() {
        let item = json!({"id":"v","snippet":{"title":"Title","description":"Description","categoryId":"27","tags":["existing"]},"status":{"privacyStatus":"private","publishAt":"2099-01-01T00:00:00Z","license":"creativeCommon","containsSyntheticMedia":true,"selfDeclaredMadeForKids":false,"embeddable":false},"localizations":{"fr":{"title":"Titre","description":"Texte"}}});
        let mut input: VideoUpdate = serde_json::from_value(
            json!({"request_id":"r","publication_id":"p","expected_etag":"e","privacy":"public"}),
        )
        .unwrap();
        assert!(merge_update(&input, &item).is_err());
        input.settings.clear_schedule = true;
        let body = merge_update(&input, &item).unwrap();
        assert!(body["status"].get("publishAt").is_none());
        assert_eq!(body["status"]["containsSyntheticMedia"], true);
        assert_eq!(body["status"]["license"], "creativeCommon");
        assert!(body.get("snippet").is_none());
        assert!(body.get("localizations").is_none());
        input.privacy = None;
        input.settings.clear_schedule = false;
        input.settings.tags = Some(vec![]);
        let body = merge_update(&input, &item).unwrap();
        assert_eq!(body["snippet"]["tags"], json!([]));
        assert_eq!(body["snippet"]["description"], "Description");
        assert!(body.get("status").is_none());
    }
}
