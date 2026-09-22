use super::PublishInput;
use serde_json::{Value, json};

pub(super) const VERSION: &str = "4";
pub(super) const INSTRUCTIONS: &str = include_str!("agent-instructions.md");

pub(super) fn payload() -> Value {
    json!({
        "version": VERSION,
        "instructions": INSTRUCTIONS,
        "publish_schema": schemars::schema_for!(PublishInput),
        "delete_schema": schemars::schema_for!(super::DeleteInput),
        "visibility_schema": schemars::schema_for!(super::VisibilityInput)
    })
}
