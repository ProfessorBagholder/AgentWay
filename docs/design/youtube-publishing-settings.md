# YouTube publishing settings

Status: design requirement; implementation gaps below are not claims of existing support. Reviewed against official documentation on 2026-09-21. Applies to HTTP and MCP equally, and to full episodes and Shorts.

## Existing implementation

PublishInput requires `made_for_kids` and `contains_synthetic_media` booleans. The worker sends them as `status.selfDeclaredMadeForKids` and `status.containsSyntheticMedia`. It also accepts title, description and privacy. Category is hard-coded to 24. The current result checks actual privacy, not all metadata. Successful private/public Muse tests do not establish disclosure readback, complete settings support or Shorts classification.

## Capability inventory

Expose a versioned machine-readable schema per destination account. Each setting records type, allowed values, operations (create/update/read), required scopes, provider/account restrictions and evidence. Distinguish implemented, documented but not implemented, verification needed, and unavailable. Never infer writability simply because a field appears in a GET response.

| Setting group                                                                                                                                     | Provider surface / design treatment                                                                                                                                                                                                 |
| ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Title, description, tags, category, metadata language, localized titles/descriptions                                                              | Documented video metadata writes. Replace fixed category with validated agent choice or explicit owner default.                                                                                                                     |
| Altered/synthetic content and child-directed audience                                                                                             | Writable status declarations. Already sent on create; add documented agent guidance and observed values.                                                                                                                            |
| Visibility, scheduled publication, license, embedding, public statistics                                                                          | Documented status writes. Validate scheduling against provider restrictions.                                                                                                                                                        |
| Recording date                                                                                                                                    | Documented recording metadata write; optional.                                                                                                                                                                                      |
| Subscriber notification                                                                                                                           | Upload request option; not a persisted video setting that can be read back.                                                                                                                                                         |
| Paid promotion and audio language                                                                                                                 | Resource fields exist, but method writable-field lists do not clearly establish the entire write contract. Verify exact endpoint behavior and scope before enabling; do not label unsupported permanently or claim working support. |
| Thumbnail, caption tracks, playlist membership                                                                                                    | Separate API operations; plan durable child operations with their own permissions, verification and recovery.                                                                                                                       |
| Age restriction, comment defaults, monetization/ad suitability, remix controls, end screens/cards, Shorts related-video link, podcast designation | Explicit coverage backlog. Do not expose as writable until a supported mechanism is established for the actual account. Report gaps before publication.                                                                             |

Sources: [video insert](https://developers.google.com/youtube/v3/docs/videos/insert), [video update](https://developers.google.com/youtube/v3/docs/videos/update), [video resource](https://developers.google.com/youtube/v3/docs/videos). Account capability tests are required in addition to reading documentation.

YouTube's disclosure concerns realistic altered/synthetic content; AI involvement in scripting alone does not automatically require it. The agent makes the content assessment using the owner's instructions and [YouTube's disclosure guidance](https://support.google.com/youtube/answer/14328491). AgentWay transports the declaration and enforces owner policy. It must not guess from filenames, the agent's provider, or the fact that an agent submitted the video.

## Agent workflow

1. Discover the destination's settings schema and the authenticated agent's permissions. Return support and scope gaps before media transfer where possible.
2. Submit the media reference, metadata, content declarations, desired settings, related assets and idempotency key. A validation operation uses the same validator as publication and makes no external writes.
3. Resolve owner defaults and explicit overrides into an immutable effective request. Return per-field provenance (agent / owner default / enforced policy), conflicts and unmet requirements. Required content declarations must be explicit per video. An owner may enforce disclosure=true, but missing declarations must never silently become false.
4. Persist the effective request and defaults/schema versions with the task. Changes to channel defaults must not alter retries of accepted work. Recheck revocations and enforced policy at execution; conflicting new policy pauses work with a structured reason.
5. Execute authorized operations with durable receipts. When completion requires a post-upload operation, stage privately and release only after required settings/assets succeed, where supported. Do not send a public video first then discover a required disclosure cannot be applied. Scheduled release follows the same dependency gate.
6. Read back provider state for readable settings. Return requested / effective / observed values with timestamps and `verified`, `mismatch`, `not_readable`, or `verification_failed`. Missing/omitted provider values are unknown, not false. A successful upload is not full settings verification.
7. Optional follow-up failures can yield partial completion; required failures block the intended final release. Preserve the existing video ID and retry only incomplete operations. A failed caption upload must not cause a second video upload.

Expose settings in agent tools, not solely in forms. Use typed Rust inputs and generated JSON schemas with identical HTTP/MCP validation; reject unknown fields. Broader editing operations need scope review: current upload + read-only consent does not authorize all video updates. Request incremental reconnection when needed without disrupting existing upload permission.

If a required setting cannot be automated, return a structured capability gap with the setting name, reason and available manual action. The agent can report it or follow a pre-authorized fallback; AgentWay must not falsely report autonomous completion. Browser automation is a separate adapter decision, not a hidden fallback.

## Updating existing videos

Offer authorized metadata updates using the existing video ID, without re-uploading media. Preserve unrelated values: YouTube update semantics can reset omitted mutable properties in selected parts. Read current state, merge intended changes, serialize per-video operations and use provider conditional controls where supported. Detect external edits as far as the provider permits and surface conflicts rather than overwrite silently; do not claim atomic compare-and-swap unless verified.

Track declaration (owner/agent input) separately from provider-enforced classification. Do not overwrite a provider's audience determination merely to make verification green. Distinguish scheduled private from unexpected private, and distinguish API acceptance from public availability or Shorts placement.

## UI placement

- **Destinations → YouTube account → Publishing defaults:** reusable category, language, license and notification choices, plus explicit owner restrictions. Group advanced settings behind disclosure; no giant mandatory form.
- **Tasks → publication → Video settings:** compact readable summary of content disclosures and visibility, then expandable metadata/distribution/assets. Show exceptions first, with source and observed value when useful.
- Editing a setting is a real authorized operation on that video, with immediate targeted updates. Agents can perform the same operation through their tool contract.
- Settings absent from the current connector are labelled accurately in capability details. Do not render inert controls that imply support.

## Acceptance gates

- Both transports require explicit content declarations; false is accepted as an intentional value, absent is rejected.
- Multiple agents receive only account/operation capabilities they are authorized to use.
- Each implemented setting has a provider mapping test; disclosure readback is independently tested.
- Metadata updates preserve tags, language, privacy and declarations not being changed.
- Default changes after admission cannot change a retry's effective settings.
- Scheduling validation, scope expiry, provider overrides and unreadable fields give actionable structured outcomes.
- Required unsupported settings are rejected before publication; required post-upload failure retains a recoverable private video, not a false success.
- Thumbnail/caption/playlist failures and ambiguous responses retry without duplicate videos or memberships.
- Live account tests are explicitly distinguished from fake-provider tests. The owner authorizes any new external test publication.
