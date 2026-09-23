# Resumable media receiving

## Boundaries

This implements [ADR 0001](adr/0001-resumable-media.md): tusd owns transfer files, offsets and protocol locking; AgentWay owns authentication, reservations, media identity, integrity verification and publishing. The private service is pinned to tusd v2.10.1 by container digest. It has no published host port, downloads are disabled, and it cannot access the application database or credentials. AgentWay mounts its dedicated volume read-only and never copies completed multi-gigabyte files merely to hand them to the publisher.

Legacy `create_media_upload` and full-file PUT remain available. Both storage types resolve through one media-file adapter; provider behavior and publication IDs are unchanged. HTTP and MCP call the same application service. Binary chunks use authenticated HTTP, not base64 inside MCP messages. Guidance version 14 advertises the new tools and deployed limits.

## Creation and recovery

The client supplies one request UUID, exact length, MIME type and whole-file SHA-256. Reservation and its journal entry commit together. Identical retries return the same logical media ID; changed inputs conflict. Cancellation and expiration retain tombstones.

Each internal tusd creation attempt has a fresh UUID committed before the POST. The pre-create hook maps that attempt to a transport ID and atomically claims it; a repeated ID is rejected, never allowed to truncate an existing file. An uncertain creation is recovered by HEAD of its known attempt ID. After 30 seconds without a resource, another internal attempt is permitted, with at most three attempts per media reservation. No client can address an unselected attempt or supply an arbitrary internal URL. If a response is lost after creation, the next status request attaches the existing transfer. Unselected late creations are removed by periodic cleanup. Unresolved exhaustion remains an explicit error that requires cancellation/recovery, not a new logical upload silently created by the server.

No SQLite byte offset is maintained. Every resume reads tusd's authoritative offset. The adapter limits each PATCH to 8 MiB, verifies its optional checksum before forwarding, and takes one of two transfer permits. It buffers at most 8 MiB per request, with a 120-second body deadline. A per-media lock coordinates receive, completion and deletion; an owned task keeps that lock through client cancellation. Permission is checked again after receiving a chunk, before forwarding it; revocation after that admission can allow this already-admitted chunk to finish, but subsequent chunks and publishing are denied. The tusd request has its own timeout. A failed or lost response is uncertain: HEAD before retrying, since some bytes may have been stored.

The public byte endpoint supports tus 1.0 core, checksum, expiration and termination. The checksum extension is enforced at AgentWay's authenticated boundary; SHA-256 and protocol-required SHA-1 are accepted for chunks. Whole-file integrity always uses SHA-256. JSON reservation and completion are AgentWay operations, not tus creation extensions. Deferred length and concatenation are not exposed.

## Readiness, lifecycle and limits

Receiving all bytes does not mark media ready. `complete_media_upload` checks the authoritative length, streams the entire file through SHA-256, then commits readiness and its journal entry together. Repeated completion is safe. Hash mismatch blocks publishing. Ready media cannot be changed through the byte endpoint. No receiving or completion operation publishes content automatically.

Cancellation durably invalidates readiness before deleting the transport. Interrupted cancellation/expiration is retried by the worker. Media referenced by publications or unresolved asset/podcast operations is protected. Unfinished transfers expire after seven days without a successful chunk. Completed media is retained until explicitly removed when unused. Transfer tombstones and internal creation claims have no retention purge in this increment; completed-publication media retention remains a separate open audit item.

Defaults are 2 GiB/video, 2 MiB/artwork or captions, 10 GiB reserved staging, 1024 active resumable reservations, and two transfers. `MEDIA_MAX_VIDEO_BYTES` can lower the video limit; `MEDIA_RESERVATION_BYTES` adjusts the shared staging budget. Status guidance returns deployed values. Admission also requires 256 MiB of disk headroom beyond the new file and unfinished reservations. This accounting is conservative: partial bytes may be counted twice. It is not a guarantee against another process filling the disk afterward. Subsequent storage failures preserve unready state and surface errors.

Compose starts tusd with the app. `/health/media` checks the dependency and is included in the app healthcheck. Existing legacy publication reads do not require tusd. Backups must include **both** `agentway-data` (database, key, legacy files) and `agentway-media` (transport files, metadata and claims). Stop both writers for a consistent volume backup; the isolated integration test exercises backup and restore of both volumes with an unfinished transfer and an unchanged test credential. Sharing this local SQLite/filesystem installation between app replicas is unsupported.

## Agent sequence

1. Hash the source; reserve with a stable UUID and persist the returned media ID.
2. HEAD the byte URL with `Tus-Resumable: 1.0.0`; read `Upload-Offset`.
3. PATCH at most 8 MiB with `Content-Type: application/offset+octet-stream` and the exact offset. Optionally include `Upload-Checksum: sha256 <base64 digest>`.
4. After an uncertain response or 409, HEAD again. Honor Retry-After for busy/unavailable responses. Never guess the offset or replace the media ID to bypass an error.
5. When offset equals size, call `complete_media_upload`. Require `ready=true` before passing the media ID to a publishing/asset tool.
6. Cancel unused media through `cancel_media_upload`.

[The Python reference client](examples/resumable-upload.py) uses only the standard library, persists its request identity before creation, validates the source on resume and refuses redirects. It stages media without publishing. Agent products still need an HTTP/file capability to transfer binary ranges; MCP discovery alone does not prove that capability.

## Verification scope

Rust adapter tests use a fake transport to exercise application ownership, idempotency, checksums, lifecycle, concurrent requests, cancellation and creation-response recovery. `tests/architecture/tusd_contract.py` separately tests the pinned reference implementation. Public ingress verification uses synthetic media and the reference client; it is not a substitute for a real Muse/Grok resumable transfer. Exact deployment results are recorded in [STATUS](STATUS.md).

The isolated integration test also checks storage write denial, sidecar unavailability and a consistent two-volume restore. Physical power-loss durability and actual ENOSPC fault injection remain unverified; permission denial is not a disk-full test. Do not describe those gates as passed. The broader operation journal/UI, automatic published-media retention and cloud object storage remain separate work.
