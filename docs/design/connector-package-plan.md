# Personal Podcast Connector Bridge — Package Plan

Planning draft • 21 September 2026 • No implementation or live publishing tests performed

**Superseded in part:** `bridge-design.md` is the current architecture and delegation plan. It replaces section 4's agent-product assumptions and section 8's delivery estimates. This document remains the publishing destination catalog; its unverified routes are still unverified.

## 1. Fixed product boundary

The package exposes publishing-platform capabilities to the user's existing AI agent. The agent creates the content, chooses destinations and formats, supplies metadata, decides the sequence of actions, and interprets results. The bridge authenticates, translates explicit tool calls, transports files, executes requested operations, and returns structured responses.

The package does not contain an AI model, independent agent, content generator, editor, campaign planner, audience strategist, or autonomous cross-posting workflow. It never chooses a caption, target account, episode association, publication time, disclosure, or retry after an ambiguous publication outcome. File inspection and validation are deterministic connector functions; unsuitable files are returned to the agent for correction.

Local installation and self-hosting are first-class. No bridge subscription or model API key is required. External platform charges, account eligibility, API access, and media-hosting costs remain separate.

## 2. Complete destination catalog

Priority describes development order and likely relevance to this podcast, not a claim of proven audience performance. Inclusion is a planned connector target, not a claim that its implementation already exists. API = documented official interface; UI = candidate authenticated browser interface requiring validation; RSS = distribution from a configured feed/host. UI documentation alone does not establish permission or reliable automation.

### Primary publication and short-form destinations

| Destination | Formats to expose | Required operations and route |
|---|---|---|
| YouTube | Full video episodes, short clips eligible for Shorts, thumbnails, captions, descriptions, playlist membership | Official video API for upload, metadata, status, edits/deletion where supported. Separate capability for attaching a Short's related video: Studio UI route is documented; no public API setter verified. Podcast-playlist designation must also be verified separately from ordinary playlist membership. [Video resource](https://developers.google.com/youtube/v3/docs/videos), [related-video UI](https://support.google.com/youtube/answer/14075157?hl=en) |
| Spotify | Audio episodes, full video episodes, episode-linked Clips, episode descriptions/artwork | Audio via configured podcast host/RSS or creator UI. Video and Clips via candidate creator-UI adapter; no public creator publishing API verified. One Clip per episode; current Clip requirements include 15–90 seconds, audio, MP4/MOV, and up to 1 GB. Existing external-feed episodes can receive video through Spotify for Creators. [Clips](https://support.spotify.com/us/creators/article/clips/), [external-host video](https://support.spotify.com/fm/creators/article/video-episodes-for-shows-not-hosted-with-spotify/) |
| Instagram | Reels, Stories, image/carousel posts, captions, covers where available | Official publishing integration for eligible professional accounts. Model ordinary Reels, Stories, and link stickers as separate capabilities; do not imply Story publishing includes a clickable-link sticker. [Meta API collection](https://www.postman.com/meta/workspace/instagram/documentation/23987686-9386f468-7714-490f-9bfc-9442db5c8f00) |
| TikTok | Short videos, photo posts where enabled, descriptions, visibility and disclosure fields | Official Content Posting API. Expose Direct Post separately from upload-to-inbox; the latter is not completed publication. Account connection alone does not establish approved Direct Post access or satisfy per-post requirements. [Guidelines](https://developers.tiktok.com/docs/en/content-sharing-guidelines) |
| Facebook Page | Reels, episode/segment video where supported, image/link/text posts | Official Page publishing interface. Target Pages, not an assumed personal-profile publishing capability. [Meta Reels collection](https://www.postman.com/meta/facebook/documentation/r56bjfd/facebook-api?entity=request-23987686-4437f4e3-569e-4982-95ba-69a9c2452500) |
| Apple Podcasts | Audio episodes, trailers and bonus episodes via RSS; video as a separately configured capability | Publish to the configured feed/host, then read directory availability when possible. Standard video RSS remains available; Apple's HLS video route depends on eligible hosting-provider support and account access. Do not equate this with a universal direct upload API. [RSS](https://podcasters.apple.com/support/823-podcast-requirements), [video](https://podcasters.apple.com/support/5593-how-to-publish-video) |

### Audio-directory distribution

Include Apple Podcasts, Spotify audio, Amazon Music/Audible, Pocket Casts, Overcast, Castro, Podcast Addict, Castbox, iHeartRadio, TuneIn, Deezer, Player FM and Podcast Index as a directory registry and compatibility checklist. Verify each current submission/claiming mechanism before advertising integration. Do not invent a separate episode-upload API for each directory.

The agent publishes an episode once through the selected host or a configured public RSS destination. Directories ingest the feed independently; availability can lag. Preserve stable episode GUIDs and avoid duplicate publication through both feed ingestion and creator upload.

For Apple, initial feed submission and review are distinct from ongoing episode distribution. Pocket Casts accepts a feed URL or Apple Podcasts link. Amazon provides a show-claiming/submission surface. [Apple submission](https://podcasters.apple.com/support/897-submit-a-show), [Pocket Casts](https://pocketcasts.com/submit), [Amazon](https://podcasters.amazon.com/)

Implement a host-adapter interface. Initial reference implementation: explicitly publish agent-supplied audio, artwork and metadata to a user-configured static/object-storage destination and update its RSS feed, or call a selected existing host's documented API. Hosting-provider selection is an implementation decision, not a new hosted service operated by this package. Public feed/media URLs must remain available when the local bridge is off; localhost URLs cannot serve podcast directories.

### Relevant promotional and community connectors

| Destination | Formats and purpose | Route / qualification |
|---|---|---|
| X | Native short video, images, text and episode-link posts; topical market commentary | Official API. Its current API is usage-priced; free bridge software does not make X API requests free. [Pricing](https://docs.x.com/x-api/getting-started/pricing) |
| Threads | Video clips, images, text and links | Official Threads publishing interface; check account scopes and media constraints. [Meta collection](https://www.postman.com/meta/threads/request/34203612-c940a17f-e719-4b5b-9d28-7390bc658fb7) |
| Bluesky | Video, images and linked text posts | AT Protocol publishing/video operations. [Post creation](https://docs.bsky.app/docs/tutorials/creating-a-post) |
| Reddit | Community-specific text/link/image posts and video where supported | Approved API access and the selected community's posting rules; expose required flair and supported post types. Native-video implementation is a separate test, not assumed from text posting. [Access guidance](https://support.reddithelp.com/hc/en-us/articles/14945211791892-Developer-Platform-Accessing-Reddit-Data) |
| LinkedIn | Native video, images and episode-link commentary | Official Videos/Posts interfaces with the relevant member or organization permissions. Useful if the agent targets workplace/industry satire. [Video API](https://learn.microsoft.com/en-us/linkedin/marketing/community-management/shares/videos-api?view=li-lms-2026-06) |
| Substack | Full video/audio podcast posts, written episode notes/newsletters, Notes/clip promotion | Candidate creator-UI connector; public write API not verified. Treat publishing a web post and emailing subscribers as distinct requested actions. [Video guide](https://support.substack.com/hc/en-us/articles/21093671091220-Guide-to-video-posts-on-Substack) |
| Discord | Episode announcements, attachments, links and requested community replies | Bot or webhook in authorized channels; file-size limits exposed to agent. No personal-account self-bot. [Webhooks](https://docs.discord.com/developers/resources/webhook) |
| Telegram | Channel announcements, video/audio messages and links | Bot API with channel permissions. [Bot API](https://core.telegram.org/bots/api) |
| Mastodon | Captioned video, images, text and links | Official instance API; discover instance-dependent limits. [Statuses](https://docs.joinmastodon.org/methods/statuses/) |

### Included in the extended connector backlog

- Snapchat Spotlight/public Stories: plausible additional short-video discovery. Official Public Profile APIs exist but are allowlist-gated; content-management access is a delivery gate. [Snap access](https://www.developers.snap.com/marketing-api/Public-Profile-API/GetStarted)
- Pinterest: video/image Pins with destination URLs, most relevant for evergreen money humour or visual explainers. Official API supports video Pins. [Pinterest guide](https://developer.pinterest.com/docs/work-with-organic-content-and-users/create-boards-and-pins/)
- Rumble: optional full-video and excerpt mirror. Candidate creator-UI route; public upload API not verified. Validate licensing choices rather than accepting defaults. [Creator help](https://rumble.support/en/help/how-to-use-rumble)
- Patreon: optional member-only episodes, bonus posts and community announcements. Publishing route and access not verified; add when membership distribution is part of the show.
- Own website: optional connector to an existing CMS or static storage for agent-written episode pages, transcripts and links. It transports supplied artifacts; it does not build the site or write copy.
- Twitch/Kick/live-streaming operations: conditional extension if the show adopts live production. Not part of the recorded-podcast completion claim; requires its own stream-control and credential plan.

These destinations are cataloged rather than indiscriminately targeted. The user's agent decides which are appropriate for a particular release.

## 3. Tool contract

Use a small shared vocabulary with discoverable platform-specific schemas. Do not hide meaningful platform differences behind a lowest-common-denominator publish call.

| Tool family | Responsibility |
|---|---|
| `connections.list`, `connections.status`, `connections.authorize`, `connections.revoke` | Return accounts and scopes; initiate or revoke user-authorized connections. |
| `capabilities.get`, `requirements.get` | Return actual supported operations, file requirements, account gates, charges, native scheduling and usable linking surfaces. |
| `media.register`, `media.upload`, `media.status` | Accept a scoped local file or explicit upload, create an asset handle, transfer bytes and report provider processing status. |
| `content.validate` | Return field-level problems without editing the content. |
| `content.create`, `content.update`, `content.delete`, `content.get`, `content.list` | Execute a specified action for an explicit account and format; preserve all supplied metadata. |
| `relationships.set`, `relationships.get` | Set/read supported relationships such as YouTube related video, Spotify episode/Clip association and playlist membership. Unsupported relationships return a capability error. |
| `feed.publish_episode`, `feed.update_episode`, `feed.validate`, `directory.get_status` | Expose configured RSS/host operations and directory observation without pretending ingestion is synchronous. |
| `metrics.get`, `comments.list`, `comments.reply` | Optional readback and explicitly requested replies where the provider exposes them. No autonomous engagement or optimization. |

Specific discoverable operations can use names such as `youtube.set_related_video` and `spotify.publish_clip` when platform semantics demand them. Publish and draft are distinct states. A `publish_at` field is accepted only when a provider offers native scheduling. The agent's own scheduler handles future calls otherwise; an independent content scheduler is not part of this package.

Common inputs: connection ID, exact destination, native format, asset handles, agent-authored metadata, explicit visibility, disclosures, optional native schedule, expected revision and idempotency key. The bridge never silently defaults to public, changes text, crops a video, invents a disclosure, or substitutes a different target.

Responses include provider IDs, canonical URLs, observed visibility, processing state, timestamp, request ID, warnings, and partial completion. Distinguish `accepted`, `processing`, `published`, `failed`, `auth_required`, `action_required`, `unsupported`, and `outcome_unknown`.

Capability entries include `route` (API/RSS/UI), `verified_at`, account prerequisites, tested client versions, and evidence of a successful round trip. A platform logo alone is never a support claim.

## 4. Existing-agent connection package

Ship local MCP over stdio, authenticated remote MCP over Streamable HTTP, a CLI with JSON output, and an OpenAPI-described REST interface. The CLI and MCP share the same connector core.

- ChatGPT/Codex: package tools using documented MCP/plugin interfaces, then test the exact client and account configuration. [OpenAI MCP documentation](https://developers.openai.com/plugins/build/mcp-server)
- Claude: local connection for capable local clients; remote connector for cloud-brokered clients. [Claude remote connectors](https://support.claude.com/en/articles/11175166-get-started-with-custom-connectors-using-remote-mcp)
- Grok: test the user's actual client, not merely model API capability. CLI-capable agents can call the CLI; MCP-enabled clients can connect directly. Consumer-chat support remains a separate certification item.
- Meta: Muse Code has documented MCP support. Consumer Muse must be tested separately; the existence of Muse Spark's API does not establish consumer connector support. [Muse Code](https://dev.meta.ai/docs/muse-code/extending)
- Other agents: the same CLI/MCP/API contract; no provider-specific content logic.

Include tool-use documentation, schemas and examples, not a creative-agent prompt or mandatory content workflow. An agent that cannot export its generated media cannot be made file-capable merely by connecting an MCP server; return a supported upload route and document that prerequisite.

## 5. Runtime and packaging

Required implementation: async Rust connector core, established provider/protocol libraries where suitable, SQLite for connection metadata and operation receipts, local temporary asset directory, OS keychain on native desktops and an established encrypted secret-storage mechanism on servers. React with strict TypeScript provides the frontend. Keep schemas independent of transport. See bridge-design.md and engineering-contract.md for the current architecture and launch contract.

Deliver a command-line package first, Docker image/Compose configuration for home servers, and a thin desktop setup wrapper for macOS/Windows/Linux. The wrapper shows connections, tool access and technical diagnostics; it is not a content-management application.

API operations may continue a single requested resumable upload or processing check. The journal exists to recover network interruptions and prevent duplicate writes, not to plan new work. After uncertain remote publication, return the ambiguity to the agent instead of blindly creating another post.

For local agents, files remain local until explicitly uploaded. Remote agents use streaming/chunked upload endpoints, not giant base64 tool arguments. URL imports need size limits, redirect checks and protection against internal-network fetches. Local paths require granted directory access.

Some providers fetch media from public URLs; expose a configured temporary media-serving/storage adapter for those requests. Other uploads can stream directly. A LAN-only service is not automatically reachable by a cloud assistant or a provider's media fetcher. Optional authenticated ingress or relay must preserve end-to-end account scoping; do not expose the entire home machine.

## 6. Filling interface gaps without adding another agent

For missing official operations, investigate platform-specific deterministic browser tools in a dedicated authenticated browser profile, or let the existing agent use its browser through a scoped session bridge. Neither route embeds another AI agent.

Browser adapters are candidates, not guaranteed workarounds. Confirm technical feasibility and applicable access terms before release. Do not evade audits, CAPTCHAs, 2FA, account restrictions or unavailable API approvals. Login challenges return a resumable `action_required` response. UI writes must verify the selected account, media, metadata and final state; changed interfaces fail visibly rather than clicking guessed coordinates.

The initial high-risk proofs are YouTube related-video association, Spotify video upload, Spotify Clip association, and Substack publication. TikTok approval/consent is an independent gate, even if a tool can technically send a publish request.

There is no plan to embed shared confidential platform app secrets in the distributed client. Each direct integration must use a supported native-client flow or user-managed developer credentials. If a platform requires a confidential backend or approved service, disclose that architecture/cost instead of claiming a fully local solution.

## 7. Agent-driven example, not a built-in workflow

The user's agent creates an episode and clips using its existing tools. It queries capabilities, uploads the episode to each chosen destination, inspects returned IDs, and publishes supplied clips. It explicitly attaches the returned full-episode ID where a native relationship exists and includes its chosen URL in other supported surfaces. It queries statuses and reports results to the user.

Every step is an external agent call. The bridge does not run this sequence on its own. If the agent disconnects, the bridge may finish an already-requested transfer; it does not invent subsequent distribution actions.

## 8. Delivery plan and acceptance criteria

Planning estimate for one experienced developer, with coding assistance: 1–2 weeks for capability/access proofs; 2–3 for the core and one complete API connector; 3–5 for primary destination adapters and RSS; 3–6 for promotional connectors, packaging and hardening. Roughly 9–16 weeks for broad tested coverage, not a commitment that access-gated/UI routes can all be solved in that period. External reviews are outside this estimate.

1. **Prove the gaps first.** Test the four high-risk operations with designated test accounts and existing sample media. Record supported, conditional or blocked for each. Do not spend weeks on an installer before these are known.
2. **Build the contract and core.** Implement local MCP, CLI, scoped accounts, uploads, structured errors, capability discovery and idempotent operation receipts.
3. **Primary adapters.** YouTube, Spotify, Instagram, TikTok, Facebook and RSS/Apple. Require all advertised operations, not just a successful upload.
4. **Promotion adapters.** X, Threads, Bluesky, Reddit, LinkedIn, Substack, Discord, Telegram and Mastodon. Add extended adapters after access and relevance are established.
5. **Package and certify.** Fresh-machine installs, upgrades, migration/backups, agent connection guides and a per-client capability matrix. UI adapters remain marked experimental until repeatable.

Acceptance tests: the external agent alone submits prepared assets and metadata; each claimed format reaches its intended destination; native episode relationships survive readback; no dashboard work is needed in a healthy configured session; 2FA/expired authorization is surfaced honestly; a restarted upload does not duplicate the post; partial success never becomes global success; correcting invalid content remains an agent action; account isolation and credential redaction hold.

RSS success means the feed/media update is valid and reachable, with directory ingestion separately reported. Metrics do not imply universal cross-platform attribution. Missing measurements are unavailable, not zero.

## 9. Cost and release position

Target: free personal connector package on existing hardware. No paid aggregator dependency and no inference bill from the bridge. Costs can still arise from X API requests, podcast storage/bandwidth, domains/ingress, provider plans and maintenance. Do not advertise all-network use as unconditionally free.

Proposed clean-room license: Apache-2.0, subject to dependency review before release. Inspect existing projects for interoperability and tests, but do not copy incompatible code into a differently licensed package. Optional hosted execution can be added later without changing the tool contract.

Completion claim: operation-by-operation tested coverage of the declared release catalog. The full catalog is the target; blocked connectors remain visibly blocked. A partial implementation must not be sold as a complete replacement for all first-party connectors.
