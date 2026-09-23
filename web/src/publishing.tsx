import type { BridgeConnection } from "./activity";
import { useState, type FormEvent } from "react";
import { useMutation } from "@tanstack/react-query";
import { cache, request } from "./api";

export interface YoutubeStatus {
  video_management_authorized?: boolean;
  configured: boolean;
  account: { id: string; name: string } | null;
  private_only: boolean;
  bridge_url: string;
}
export interface Publication {
  agent_name?: string | null;
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

export function ConnectionInstructions({
  status,
  connection,
}: {
  status: YoutubeStatus;
  connection: BridgeConnection;
}) {
  const [token, setToken] = useState("");
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState("");
  const access = useMutation({
    mutationFn: () =>
      request<{ token: string }>(
        `/api/agent-connections/${connection.id}/token`,
        {},
      ),
    onSuccess: (r) => setToken(r.token),
  });
  const base = status.bridge_url || "http://127.0.0.1:8788";
  const instructions = `Connect ${connection.product} to AgentWay using its MCP server: ${base}/mcp
Transport: Streamable HTTP. Authenticate every request with Authorization: Bearer <token>. Store the token in your connector's secret field, not in chat, source code or instructions. Use the credential for this ${connection.name} connection.

First discover the tools and call youtube_status. Confirm the returned connection.id is ${connection.id}, and report the connected channel and your permissions. Do not publish anything during this connection check.

Read agent_guidance from youtube_status (HTTP equivalent: GET ${base}/v1/status). It contains current tool schemas, supported settings, limits and recovery instructions. Refresh it when its version changes. Ask the user only for unresolved publishing preferences; follow their explicit instructions before any public changes.

Call list_agents to discover peers this connection may assign tasks to. Use create_agent_task with a stable request_id to place work in a peer's inbox, and get_agent_task to check the result. Queued work does not wake the peer automatically; recipients must poll list_agent_tasks and claim work. Read the handoff section of agent_guidance before using these tools.

Media transfer uses create_media_upload, followed by raw HTTP PUT to its returned upload_path at ${base}, with the same bearer token. POST /v1/youtube/publish is the HTTP equivalent of publish_youtube; made_for_kids and contains_synthetic_media must be explicit booleans. Reuse request_id and identical arguments for retries. Use get_publication to check upload and visibility; get_youtube_video checks processing. Do not claim draft, scheduling or other-platform support unless discovery explicitly advertises it.`;
  return (
    <details
      className="panel instructions"
      open={connection.state === "Setup incomplete" ? true : undefined}
    >
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
