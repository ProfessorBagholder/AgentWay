import { useState, type FormEvent } from "react";
import { useMutation } from "@tanstack/react-query";
import { cache, request } from "./api";

export interface YoutubeStatus {
  configured: boolean;
  account: { id: string; name: string } | null;
  private_only: boolean;
  bridge_url: string;
}
export interface Publication {
  id: string;
  title: string;
  status: "queued" | "uploading" | "uploaded" | "interrupted";
  uploaded_bytes: number;
  total_bytes: number;
  video_url: string | null;
  error: string | null;
  created_at: string;
  revision: number;
}
export function upsertPublication(p: Publication) {
  const previous = cache.getQueryData<Publication>(["publication", p.id]);
  if (previous && previous.revision >= p.revision) return;
  for (const [filter, status] of [
    ["attention", "interrupted"],
    ["complete", "uploaded"],
  ]) {
    cache.setQueryData<string[]>(["publications", filter], (old) => {
      const ids = old ?? [];
      return p.status === status
        ? ids.includes(p.id)
          ? ids
          : [p.id, ...ids]
        : ids.includes(p.id)
          ? ids.filter((id) => id !== p.id)
          : ids;
    });
  }
  cache.setQueryData<Publication>(["publication", p.id], (old) =>
    old && old.revision >= p.revision ? old : p,
  );
  cache.setQueryData<string[]>(["publications"], (old) =>
    old?.includes(p.id) ? old : [p.id, ...(old ?? [])],
  );
}
export function GoogleConfig() {
  const save = useMutation({
    mutationFn: (body: unknown) =>
      request<YoutubeStatus>("/api/youtube/config", body),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    save.mutate(Object.fromEntries(new FormData(e.currentTarget)));
  }
  return (
    <form onSubmit={submit} className="publishing-form">
      <p>
        Use a Google Cloud project with YouTube Data API v3 enabled. Create an
        OAuth client of type <strong>Web application</strong> and add this exact
        redirect URI:
      </p>
      <code className="copy-value">
        {window.location.origin}/api/youtube/callback
      </code>
      <p className="field-help">
        If the consent screen is in testing mode, add your Google account as a
        test user.{" "}
        <a
          href="https://developers.google.com/identity/protocols/oauth2/web-server#creatingcred"
          target="_blank"
          rel="noreferrer"
        >
          Google setup instructions
        </a>
      </p>
      <label>
        Client ID
        <input name="client_id" required autoComplete="off" />
      </label>
      <label>
        Client secret
        <input
          name="client_secret"
          type="password"
          required
          autoComplete="off"
        />
      </label>
      {save.error && (
        <p className="error" role="alert">
          {save.error.message}
        </p>
      )}
      <button className="primary" disabled={save.isPending}>
        {save.isPending ? "Saving…" : "Save credentials"}
      </button>
    </form>
  );
}

export function ConnectionInstructions({ status }: { status: YoutubeStatus }) {
  const [token, setToken] = useState("");
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  const access = useMutation({
    mutationFn: () => request<{ token: string }>("/api/publishing/token", {}),
    onSuccess: (r) => setToken(r.token),
  });
  const base = status.bridge_url || "http://127.0.0.1:8788";
  const instructions = `Connect to my AgentWay publishing bridge at ${base}.

AUTHENTICATION
Use Authorization: Bearer <token> on every HTTP request, including the media PUT. Obtain the token through your secure credential prompt; never ask for Google credentials in chat. MCP clients use ${base}/mcp with the same bearer token and discover the tool schemas.

BEFORE UPLOADING
Read GET /v1/status (MCP: youtube_status). Follow the returned agent_guidance instructions and publish_schema; remember its version and revisit changed guidance. Briefly introduce the supported publishing choices to the user on first use, reuse established preferences, and ask only for unresolved decisions. Confirm the intended channel and respect private_only. Choose privacy from private, unlisted, or public as requested by the user; default to private when unspecified. If private_only is true, non-private uploads are blocked. Do not silently change the requested visibility.

VIDEO TRANSFER
POST /v1/media with {"size":<exact byte count>,"mime":"video/mp4"} (MCP: create_media_upload). PUT the complete raw file bytes to the returned upload_path on this bridge, using the same bearer token. Do not send a filesystem path, download URL, base64 or multipart body. Media must finish uploading before publication. AgentWay accepts video/mp4, video/quicktime and video/webm, up to 2 GiB; the tunnel may impose a lower limit.

PUBLISH REQUEST
POST /v1/youtube/publish (MCP: publish_youtube) with:
{"request_id":<new UUID>,"media_id":<returned ID>,"title":<title>,"description":<description>,"privacy":<chosen visibility>,"made_for_kids":<explicit boolean>,"contains_synthetic_media":<explicit boolean>}

SETTINGS
- title: required, 1–100 characters; description: optional, at most 5000 UTF-8 bytes. Neither may contain < or >.
- made_for_kids: required. Set according to the video's intended audience; never hard-code false or infer it from the channel's name.
- contains_synthetic_media: required. Declare realistic altered or synthetic content according to YouTube's guidance: https://support.google.com/youtube/answer/14328491. AI assistance with a script alone does not automatically require disclosure. The agent determines the value from the content and user instructions; AgentWay does not inspect the video to decide it.
- Both declarations must be JSON true or false, not strings or omitted fields. Resolve unknown declarations before submission.
- Only the seven fields above are supported. Category is currently fixed to Entertainment (24), and subscriber notifications are disabled. Tags, scheduling, paid-promotion settings, language, thumbnails, captions, playlists and post-upload edits are not supported by this bridge yet. If the user requires an unsupported setting, report that limitation before uploading; do not omit it silently or claim it was applied.

RETRIES AND RESULTS
Reuse the same request_id and identical arguments on retries. Use a new request_id for a genuinely new upload; never change arguments on an existing request_id. Save the returned publication id. Poll GET /v1/publications/{id} (MCP: get_publication) every five seconds while queued or uploading. Stop and report interrupted jobs; POST /v1/publications/{id}/retry (MCP: retry_publication) resumes the same job. Do not create a new request to recover an uncertain or expired upload session.

A video_url confirms upload completion only. Read GET /v1/publications/{id} after upload and compare actual_privacy with requested_privacy. Only report public publishing success when actual_privacy is public. Report any mismatch. Null actual_privacy means unverified; report visibility_error and retry the status check, never upload again for a verification failure. Current readback verifies visibility only, not the disclosure fields. Upload completion does not mean YouTube has finished processing or classified it as a Short.`;
  return (
    <details className="panel instructions">
      <summary>Connection instructions</summary>
      <div className="pad">
        <label>
          HTTP endpoint<code className="copy-value">{base}</code>
        </label>
        <label>
          MCP endpoint<code className="copy-value">{base}/mcp</code>
        </label>
        <textarea
          aria-label="Agent connection instructions"
          readOnly
          value={instructions}
          rows={8}
        />
        <div className="actions">
          <button
            onClick={async () => {
              try {
                await navigator.clipboard.writeText(instructions);
                setCopied(true);
                setCopyError("");
              } catch {
                setCopyError(
                  "Copy failed. Select and copy the instructions above.",
                );
              }
            }}
          >
            {copied ? "Copied" : "Copy instructions"}
          </button>
          <button
            disabled={access.isPending}
            onClick={() => (token ? setToken("") : access.mutate())}
          >
            {token ? "Hide token" : "Show token"}
          </button>
        </div>
        {token && (
          <label>
            Agent token
            <input readOnly value={token} onFocus={(e) => e.target.select()} />
            <span className="muted">
              Enter this in your agent’s secure credential field.
            </span>
          </label>
        )}
        {(access.error || copyError) && (
          <p role="alert" className="error">
            {access.error?.message || copyError}
          </p>
        )}
      </div>
    </details>
  );
}
