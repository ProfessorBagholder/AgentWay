# ADR 0001: Resumable agent-to-bridge media transport

Date: 21 September 2026. Status: dependency selected for integration; production integration not implemented. Scope: local/self-hosted AgentWay, preserving the Rust application and current YouTube flow.

## Problem

AgentWay currently receives an entire video in one PUT. A reverse proxy's request-size limit therefore becomes an accidental video-size limit. Interrupted uploads restart from zero, although the later AgentWay-to-YouTube leg is already resumable. A stable hostname does not solve either issue.

## Decision

Use **tus 1.0** for resumable transport and **tusd**, the official reference implementation, as a private storage/transport component. Keep authentication, authorization, reservations, quota, artifacts, publication operations and provider adapters in Rust. Compose should manage the dependency through the existing one-command launcher. Do not expose tusd directly to the Internet or publish its unauthenticated port on the host.

This is one established protocol component, not a replacement backend or a new content-processing service. It is an explicit exception to the initial single-process preference, justified by avoiding a new upload protocol implementation. The dependency proof pins v2.10.1 and its container digest; upstream is MIT licensed. Release/advisory and deployment resource reviews remain integration gates, not conclusions of a seven-case protocol test.

The selected protocol offers HEAD to obtain the actual committed offset, PATCH to transmit from that offset, and conflict detection for stale offsets. AgentWay should advertise an **8 MiB maximum per request**, independently of the total artifact-size policy. A client must query offset after an uncertain result rather than assume nothing was stored. Exact supported extensions must be discovered and tested; do not claim checksum or concatenation extensions merely because tus defines them.

## Ownership and durable boundaries

- The Rust API reserves capacity and records an operation before authorizing transfer creation. Every public request authenticates through Rust, including HEAD/PATCH/DELETE. A caller must never select an arbitrary internal URL or filesystem path.
- A durable transfer record maps a stable AgentWay media ID to the storage upload ID and records creation/termination intent. Creation must be idempotent at the AgentWay boundary. Reconcile crashes between reservation, tusd creation and mapping commit. A lost creation response must neither leak unaccounted storage indefinitely nor create another logical artifact. Evaluate tusd hooks for this mapping and enforce the invariant with failure injection before shipping.
- tusd owns transport offset/file-write locking; Rust owns artifact state (`reserved`, `receiving`, `verifying`, `ready`, `deleting`, `deleted`/`expired`) and publication authorization. Do not maintain a second independently writable byte-offset ledger.
- Observe completion from authoritative transport state, verify declared length and a streamed content digest, then atomically commit ready state plus its journal entry. Optional caller-provided digests must be checked when supplied. Completion callbacks can be duplicated or lost; reconciliation must finish without requiring another successful callback.
- Publishing can consume only a ready artifact. Existing accepted jobs retain immutable artifact references. Cleanup uses durable deletion intent, excludes live/retryable references and reconciles partial failures. Completed publication metadata survives removal of expired media bytes.
- Reserve total bytes, limit active streams, bound request size and account for temporary bytes. Reject insufficient quota/free-space headroom before admitting more work. Resume/delete operations remain available when creation is blocked.
- The storage adapter exposes artifact access to the publisher. Avoid copying a multi-gigabyte file merely to cross the adapter boundary. Test filesystem/layout assumptions explicitly; no client-controlled path handling.

## Compatibility and client contract

Preserve the existing `POST /v1/media` and complete-file PUT path for the current client while adding an explicit resumable transfer option. Do not change the stored publication input serialization or existing idempotency IDs as a side effect. Do not rotate the existing token, disconnect YouTube or rename existing records.

A resumable reservation should return the logical media ID, authenticated upload URL/path, transfer protocol/version, total length, maximum request size and recovery instructions. HTTP and MCP reservation tools must return the same application-service result. Increment versioned guidance and update copied connection instructions together; use schemas generated from the actual types and tests to prevent drift.

The agent sends raw binary chunks through its HTTP/file capability, never base64 video inside MCP messages. If an agent product cannot transfer binary ranges, report that capability gap honestly; supporting MCP alone does not prove resumable media transfer from that product. The bridge does not edit or generate the media.

The current 2 GiB artifact cap and 10 GiB reservation budget are application policy, not tus or YouTube limits. Make the deployed limits discoverable and configurable as part of integration. Do not silently promise unlimited disk or silently lower a requested video's size.

## Alternatives

| Option | Assessment |
|---|---|
| Increase the request limit or change tunnel provider | May permit a bigger single request, but leaves restart, retry and concurrency problems. Insufficient. |
| Implement tus or a private chunk protocol in Rust | Reduces process count but makes AgentWay responsible for protocol correctness and recovery edge cases. No adequately vetted mature Rust implementation was established in this review; do not interpret that as proof none exists. Prefer the tested reference implementation. |
| tusd behind the Rust boundary | Selected. Adds a pinned component and explicit storage integration; reuses maintained transport behavior. Must test the adapter and crash boundaries before production adoption. |
| Direct multipart uploads to object storage | Appropriate for a future cloud deployment, but adds storage provisioning, signed part URLs and different completion/cleanup semantics. Keep storage replaceable; do not require a cloud account for local use. |

## Acceptance before enabling for agents

- Legacy small-file PUT and existing publication retry/privacy/disclosure tests still pass.
- Authenticated AgentWay path transfers >100 MB without a request over 8 MiB and without whole-file buffering; maximum-size/quota bounds are exercised.
- Disconnect midway through a chunk; lose a successful response; restart the app and storage service independently; recover without duplicated bytes or new media IDs.
- Concurrent writes, completion, publication and deletion cannot expose incomplete content or delete referenced content.
- Interrupt every creation/finalization/deletion boundary; reconcile orphan files/reservations; expiration and physical storage accounting converge.
- Disk full, read-only storage, unavailable sidecar, expired credentials and canceled transfers produce safe typed errors and correlated activity records. Requests after revocation fail; explicitly define handling of an already admitted in-flight chunk.
- One-command cold start and upgrade preserve data; readiness diagnoses the dependency; restore includes its data and metadata. Keep the existing tunnel running during application-only deployments where possible.
- An existing real agent demonstrates the new path with a synthetic >100 MB artifact. A further public YouTube upload is not required just to prove transport; use a separately authorized provider test only if needed.

## Evidence and sources

`tests/architecture/tusd_contract.py` exercises the pinned component independently of AgentWay. Seven checks passed on the development Docker host, including 104 MiB integrity after interruption and process restart. See the [audit](../design/reliability-audit.md) for precise exclusions.

- [tus 1.0 protocol](https://tus.io/protocols/resumable-upload)
- [Official tusd documentation](https://tus.github.io/tusd/)
- [Docker installation](https://tus.github.io/tusd/getting-started/installation/)
- [Configuration, limits and disabled downloads](https://tus.github.io/tusd/getting-started/configuration/)
- [Hooks and their delivery behavior](https://tus.github.io/tusd/advanced-topics/hooks/)
- [Pinned release](https://github.com/tus/tusd/releases/tag/v2.10.1)
- [License](https://github.com/tus/tusd/blob/v2.10.1/LICENSE.txt)
