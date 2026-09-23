# YouTube video settings

## Scope and contract

This increment extends the existing YouTube connector. HTTP and MCP invoke the same Publisher methods and generate schemas from the same Rust types. Media ownership and management consent remain unchanged. No token is rotated and no new OAuth scope is introduced: captions use the existing youtube.force-ssl management grant.

`publish_youtube.settings` is optional for backward compatibility. Its fields are optional; new callers should choose category explicitly after `list_youtube_categories`. Legacy omitted category remains 24 and existing saved notification behavior is preserved. Explicit notification defaults remain on for new uploads. Required audience and synthetic-media booleans remain top-level. Unknown fields fail rather than being silently ignored.

| Capability | Upload | Existing video | Readback |
| --- | --- | --- | --- |
| Title, description, audience, synthetic disclosure, visibility | Existing fields | update_youtube_video | get_youtube_video |
| Category, tags, metadata/audio language | settings | settings patch | snippet |
| Scheduling | Future publish_at + private | Set/reschedule or clear_schedule | status.publishAt |
| Embedding, license, public statistics | settings | settings patch | status |
| Paid product placement | settings | settings patch | paidProductPlacementDetails |
| Recording date | settings | settings patch | recordingDetails |
| Translations | settings.localizations | Explicit complete-map replacement | localizations |
| Subscriber notifications | Defaults true | Upload-only | Not exposed by YouTube |
| Thumbnail | Separate post-upload asset operation | set_thumbnail | snippet.thumbnails; visual review |
| Timed captions | Separate post-upload asset operation | Create, replace, delete | list_youtube_captions processing status |

Provider feature eligibility still applies. Grok’s live private-upload test confirmed the supplied audio language and paid-placement false values; this does not establish every value or update path. Do not claim a missing readback field is false or verified. API resource definitions include audio language and paid placement, while the mutable-field lists on insert/update omit them; values and update paths beyond those exercised still require live verification before asserting end-to-end support. The connector does not expose Studio-only controls such as end screens, cards, monetization, comment moderation configuration or in-place byte replacement. It does not generate captions or media.

## Mutation semantics

Metadata updates require an etag read from the intended publication. The provider ownership check binds the actual video to the recorded channel. A local mutation lock serializes checks and intent persistence; the provider request includes If-Match to guard the read/write interval. Only touched parts are sent and all documented writable properties in those parts are preserved. A translations map explicitly replaces that part. Provider read-only fields are not echoed.

The durable operation is stored before the provider write. Accepted writes are verification_pending until desired readback matches. Timeouts/server failures remain outcome_unknown. Same request ID + identical arguments + same authenticated connection only reconcile by reading; they never resend a potentially accepted write. A different pending operation on that publication blocks another metadata write. Status polling now also performs authorized read-only reconciliation, so agents do not have to resubmit the mutation just to refresh its status. Tag ordering is ignored during comparison because YouTube reorders tags. Completed results describe the verified operation, not perpetual current state. A process crash before sending can conservatively leave an uncertain operation; no automatic guess is made that it is safe to replay. Current recovery is read-only reconciliation; unresolved outcomes require investigation. Future operation recovery must not erase this distinction.

Thumbnails/captions use separate durable asset records because a successful resource acknowledgement and asynchronous caption processing are different from field readback. Upload sessions are encrypted and persisted before any bytes are sent. Same-ID upload_pending retries query the same session and resume from its offset; a lost final response is reconciled without retransmission, including after restart. Accepted/rejected retries return saved results. Expired sessions cannot prove which image/content was applied from a URL or caption name alone, so those remain unknown and require provider inspection instead of creating another caption. This is a deliberate conservative limit, not automatic exactly-once delivery. Caption replacement verifies its ID belongs to the selected video. New caption tracks check language/name membership before insertion. Cross-connection media and operation IDs are rejected.

Scheduling is YouTube-side and requires a future zoned RFC3339 timestamp, private visibility, and an eligible never-published video. Private-only restrictions reject scheduling at enqueue and worker execution. A schedule that elapsed before upload initiation is rejected rather than converted into immediate public release. Scheduling a video with mandatory artwork/captions should follow private upload → assets → processing verification → schedule; the workflow is not atomic.

## Boundaries

This work does not integrate resumable agent-to-AgentWay transfers or finish all operational Activity log coverage. Existing video transfer limits remain. New operation results are persisted and exposed through HTTP/MCP. Tasks displays upload-submitted settings, not an assertion about current provider state. No new per-platform default controls are added.

## Sources checked 2026-09-22

- https://developers.google.com/youtube/v3/docs/videos/insert
- https://developers.google.com/youtube/v3/docs/videos/update
- https://developers.google.com/youtube/v3/docs/videos
- https://www.googleapis.com/discovery/v1/apis/youtube/v3/rest
- https://developers.google.com/youtube/v3/docs/videoCategories/list
- https://developers.google.com/youtube/v3/docs/thumbnails/set
- https://developers.google.com/youtube/v3/docs/captions/insert
- https://developers.google.com/youtube/v3/docs/captions/update

## Verification

Automated provider fixtures cover settings preservation, stale edits, delayed readback/restart, connection isolation, schedule-policy enforcement/cancellation, caption resumable transport, encrypted session persistence and lost-completion recovery across restart without duplicate bytes. These are not live platform tests. Real write tests belong on the feature deployment using a disposable private video with the user's agent; do not use public production content as a fixture.

## Live-test corrections (2026-09-23)

Grok uploaded a disposable private video and verified metadata updates and a serving caption track. Its metadata operation stayed pending because YouTube reordered tags. The operation now compares tag values independent of order, and operation polling reconciles current readback without another write. A regression test exercises this after restart and checks cross-agent access rejection.

Thumbnail initialization returned HTTP 411 because the empty metadata request did not include Content-Length. It now explicitly sends Content-Length: 0; the regression provider requires that header before returning an upload session. Existing rejected operation IDs retain their result; a corrected thumbnail attempt uses a new request ID with the same staged artwork, without recreating the video.

YouTube omitted status.containsSyntheticMedia in this live test even though the upload supplied true. AgentWay preserves the raw status and additionally exposes contains_synthetic_media as true/false/null; null means unavailable from the provider, never false or verified. The connector does not rewrite the flag merely to get a readback. Guidance 13 documents these behaviors.

The correction was then verified through the public endpoint on Grok's same disposable video: the original metadata operation became completed through polling, and YouTube accepted the thumbnail using the existing staged image and a new request ID. Video privacy stayed private and the existing caption remained serving. All 35 Rust tests and strict Clippy passed. This verifies provider acceptance of the thumbnail, not its visual quality; synthetic-media readback is still unavailable.
