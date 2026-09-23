use super::PublishInput;
use serde_json::{Value, json};

pub(super) const VERSION: &str = "12";
pub(super) const INSTRUCTIONS: &str = include_str!("agent-instructions.md");

pub(super) fn payload() -> Value {
    json!({
        "version": VERSION,
        "podcast_schema": schemars::schema_for!(super::podcast::PodcastInput),
        "playlist_query_schema": schemars::schema_for!(super::podcast::PlaylistQuery),
        "instructions": INSTRUCTIONS,
        "channel_description_schema": schemars::schema_for!(super::ChannelDescriptionInput),
        "video_asset_schema": schemars::schema_for!(super::assets::VideoAssetInput),
        "category_query_schema": schemars::schema_for!(super::assets::CategoryQuery),
        "video_update_schema": schemars::schema_for!(super::settings::VideoUpdate),
        "publish_schema": schemars::schema_for!(PublishInput),
        "delete_schema": schemars::schema_for!(super::DeleteInput),
        "visibility_schema": schemars::schema_for!(super::VisibilityInput)
    })
}
