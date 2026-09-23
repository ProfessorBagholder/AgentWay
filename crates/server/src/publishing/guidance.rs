use super::PublishInput;
use serde_json::{Value, json};

pub(super) const VERSION: &str = "15";
pub(super) const INSTRUCTIONS: &str = include_str!("agent-instructions.md");

pub(super) fn payload(reservation_bytes: i64, max_video_bytes: i64) -> Value {
    json!({
        "version": VERSION,
        "agent_handoff_schema": schemars::schema_for!(super::handoffs::CreateHandoff),
        "agent_task_list_schema": schemars::schema_for!(super::handoffs::ListHandoffs),
        "resumable_media_schema": schemars::schema_for!(super::transfers::CreateUpload),
        "media_transfer": {"protocol":"tus", "version":"1.0.0", "max_chunk_bytes":super::transfers::MAX_CHUNK,"max_video_bytes":max_video_bytes,"max_artwork_caption_bytes":2097152,"reservation_bytes":reservation_bytes,"inactivity_expiry_seconds":604800,"whole_file_sha256_required":true},
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
