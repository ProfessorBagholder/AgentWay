import { useState, type FormEvent } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
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
  cache.setQueryData<Publication>(["publication", p.id], (old) =>
    old && old.revision >= p.revision ? old : p,
  );
  cache.setQueryData<string[]>(["publications"], (old) =>
    old?.includes(p.id) ? old : [p.id, ...(old ?? [])],
  );
}
export function Publishing() {
  const status = useQuery({
    queryKey: ["youtube"],
    queryFn: () => request<YoutubeStatus>("/api/youtube"),
  });
  const connect = useMutation({
    mutationFn: () => request<{ url: string }>("/api/youtube/connect", {}),
    onSuccess: ({ url }) => {
      // Google consent is an external navigation, not an app refresh.
      window.location.assign(url);
    },
  });
  const disconnect = useMutation({
    mutationFn: () => request<YoutubeStatus>("/api/youtube/disconnect", {}),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  const policy = useMutation({
    mutationFn: (private_only: boolean) =>
      request<YoutubeStatus>("/api/youtube/policy", { private_only }),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  if (status.isPending)
    return <p role="status">Loading publishing settings…</p>;
  if (status.error)
    return (
      <p className="error" role="alert">
        {status.error.message}
      </p>
    );
  const s = status.data;
  return (
    <div className="publishing-sections">
      <section
        className="panel publishing-section"
        aria-labelledby="youtube-heading"
      >
        <h2 id="youtube-heading">YouTube</h2>
        {s.account ? (
          <>
            <p>
              Connected to <strong>{s.account.name}</strong>{" "}
              <small>({s.account.id})</small>
            </p>
            <button
              className="secondary"
              disabled={disconnect.isPending}
              onClick={() => disconnect.mutate()}
            >
              Disconnect YouTube
            </button>
            <p className="field-help">
              Disconnecting stops further upload requests. It does not delete
              videos already uploaded. You can revoke Google access in your
              Google account settings.
            </p>
          </>
        ) : s.configured ? (
          <>
            <p>Authorize the YouTube channel your agent will upload to.</p>
            <button
              className="primary"
              disabled={connect.isPending}
              onClick={() => connect.mutate()}
            >
              Connect YouTube
            </button>
            <details>
              <summary>Change Google application credentials</summary>
              <GoogleConfig />
            </details>
          </>
        ) : (
          <GoogleConfig />
        )}
        <label className="checkbox-label">
          <input
            type="checkbox"
            checked={s.private_only}
            disabled={policy.isPending}
            onChange={(e) => policy.mutate(e.target.checked)}
          />
          Only allow private uploads
        </label>
        <p className="field-help">
          Keep this enabled for the first test. Google also restricts uploads
          from unaudited API projects to private visibility.
        </p>
        {[connect.error, disconnect.error, policy.error]
          .filter(Boolean)
          .map((e, i) => (
            <p className="error" role="alert" key={i}>
              {e?.message}
            </p>
          ))}
      </section>
      <AgentAccess status={s} />
    </div>
  );
}
function GoogleConfig() {
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
function AgentAccess({ status }: { status: YoutubeStatus }) {
  const [token, setToken] = useState("");
  const [copied, setCopied] = useState(false);
  const access = useMutation({
    mutationFn: (rotate: boolean) =>
      request<{ token: string }>(
        rotate ? "/api/publishing/token/rotate" : "/api/publishing/token",
        {},
      ),
    onSuccess: (r) => setToken(r.token),
  });
  const save = useMutation({
    mutationFn: (url: string) =>
      request<YoutubeStatus>("/api/publishing/bridge", { url }),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  const base = status.bridge_url || "http://127.0.0.1:8788";
  const instructions = `Connect to my AgentWay publishing bridge at ${base}. Authenticate with a bearer token supplied through your secure credential prompt. Never ask me to paste Google credentials into chat. Read GET /v1/status and respect private_only. Choose privacy from private, unlisted, or public as requested by the user; default to private when unspecified. If private_only is true, non-private uploads are blocked until the owner changes the setting. To transfer a finished video, POST /v1/media with {"size":<exact bytes>,"mime":"video/mp4"}, then PUT raw bytes to the returned upload_path using the same bearer token. POST /v1/youtube/publish with {"request_id":<new UUID>,"media_id":<returned ID>,"title":<title>,"description":<description>,"privacy":<chosen visibility>,"made_for_kids":false,"contains_synthetic_media":<appropriate boolean>}. Reuse the same request_id and identical arguments on retries. Poll GET /v1/publications/{id} every five seconds while queued or uploading. Stop and report an interrupted upload; POST /v1/publications/{id}/retry resumes the same session. A video_url confirms upload completion only. Read GET /v1/publications/{id} after upload and compare actual_privacy with requested_privacy. Only report public publishing success when actual_privacy is public. Report any mismatch. Null actual_privacy means unverified; report visibility_error and retry the status check, never upload again for a verification failure. Use a new request_id for a new upload; never change arguments on an existing request_id. Upload completion does not mean YouTube has finished processing or classified it as a Short. MCP clients can use ${base}/mcp with the same Authorization: Bearer token.`;
  return (
    <section
      className="panel publishing-section"
      aria-labelledby="agent-access-heading"
    >
      <h2 id="agent-access-heading">Agent connection</h2>
      <p>
        Muse runs in the cloud. Run ./run --share for a temporary HTTPS address,
        or point your own reverse proxy at AgentWay’s agent port,{" "}
        <code>8788</code>. The management interface on port 8787 stays local.
      </p>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          save.mutate(String(new FormData(e.currentTarget).get("url") ?? ""));
        }}
      >
        <label>
          Public HTTPS address
          <input
            key={status.bridge_url}
            name="url"
            type="url"
            defaultValue={status.bridge_url}
            placeholder="https://agentway.example.com"
          />
        </label>
        <button className="secondary" disabled={save.isPending}>
          Save address
        </button>
      </form>
      <p className="field-help">
        Saving an address does not create a tunnel. Local agents can use
        http://127.0.0.1:8788 directly.
      </p>
      <details>
        <summary>Connection instructions and token</summary>
        <p>
          Give the instructions below to Muse. Provide the token through its
          secure credential prompt. The token permits uploads to your connected
          channel; Google credentials remain in AgentWay.
        </p>
        <textarea
          aria-label="Agent connection instructions"
          readOnly
          value={instructions}
          rows={8}
        />
        <button
          className="secondary"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(instructions);
              setCopied(true);
            } catch {
              setCopied(false);
            }
          }}
        >
          {copied ? "Copied" : "Copy instructions"}
        </button>
        <div className="button-row">
          <button
            className="secondary"
            disabled={access.isPending}
            onClick={() => access.mutate(false)}
          >
            Show token
          </button>
          <button
            className="secondary"
            disabled={access.isPending}
            onClick={() => access.mutate(true)}
          >
            Replace token
          </button>
        </div>
        <p className="field-help">
          Replacing the token immediately disconnects clients using the previous
          token.
        </p>
        {token && (
          <label>
            Agent token
            <input readOnly value={token} onFocus={(e) => e.target.select()} />
            <button className="secondary" onClick={() => setToken("")}>
              Hide token
            </button>
          </label>
        )}
      </details>
      {[save.error, access.error].filter(Boolean).map((e, i) => (
        <p role="alert" className="error" key={i}>
          {e?.message}
        </p>
      ))}
    </section>
  );
}
export function UploadRow({ id }: { id: string }) {
  const { data: p } = useQuery<Publication>({
    queryKey: ["publication", id],
    enabled: false,
  });
  const retry = useMutation({
    mutationFn: () => request<Publication>(`/api/publications/${id}/retry`, {}),
    onSuccess: upsertPublication,
  });
  if (!p) return null;
  return (
    <div className="row">
      <div className="row-main">
        <strong>{p.title}</strong>
        <small>
          {
            {
              queued: "Queued",
              uploading: "Uploading",
              uploaded: "Uploaded",
              interrupted: "Interrupted",
            }[p.status]
          }{" "}
          · {new Date(p.created_at).toLocaleString()}
        </small>
        {p.status === "uploading" && (
          <progress
            aria-label={`Upload progress for ${p.title}`}
            value={p.uploaded_bytes}
            max={p.total_bytes}
          />
        )}
        {p.video_url && (
          <a href={p.video_url} target="_blank" rel="noreferrer">
            Open on YouTube
          </a>
        )}
        {p.error && <p className="error">{p.error}</p>}
        {retry.error && (
          <p role="alert" className="error">
            {retry.error.message}
          </p>
        )}
      </div>
      {p.status === "interrupted" && (
        <button
          className="secondary"
          disabled={retry.isPending}
          onClick={() => retry.mutate()}
        >
          Retry upload
        </button>
      )}
    </div>
  );
}
