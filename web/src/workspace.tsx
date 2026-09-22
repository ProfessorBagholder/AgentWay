import { useState, type ReactNode } from "react";
import { useInfiniteQuery, useMutation, useQuery } from "@tanstack/react-query";
import { Moon, Sun, ArrowLeft, ExternalLink } from "lucide-react";
import { cache, request } from "./api";
import {
  useConnection,
  useYoutube,
  usePublications,
  usePublication,
  type BridgeConnection,
} from "./activity";
import {
  ConnectionInstructions,
  GoogleConfig,
  upsertPublication,
  type YoutubeStatus,
  type Publication,
} from "./publishing";
const states: Record<string, string> = {
  queued: "Queued",
  uploading: "Uploading",
  uploaded: "Uploaded",
  interrupted: "Needs attention",
};
function Badge({ value }: { value: string }) {
  return (
    <span
      className={`badge ${value === "Connected" || value === "Uploaded" ? "good" : value === "Needs attention" ? "warning" : ""}`}
    >
      {value}
    </span>
  );
}
function Heading({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <header className="page-heading">
      <h1>{title}</h1>
      {children}
    </header>
  );
}
function Back({ to, children }: { to: string; children: ReactNode }) {
  return (
    <a className="back" href={`#/${to}`}>
      <ArrowLeft size={16} />
      {children}
    </a>
  );
}
function ErrorMessage({ error }: { error: Error | null }) {
  return error ? (
    <p className="error" role="alert">
      {error.message}
    </p>
  ) : null;
}
function Loading() {
  return <p role="status">Loading…</p>;
}
function ConnectionBadge({ c }: { c: BridgeConnection }) {
  return <Badge value={c.state} />;
}
function Agents() {
  const c = useConnection();
  return (
    <>
      <Heading title="Agents" />
      {c.isPending ? (
        <Loading />
      ) : c.error ? (
        <ErrorMessage error={c.error} />
      ) : (
        <section className="panel">
          <table className="agent-table">
            <thead>
              <tr>
                <th>Agent</th>
                <th>Connection</th>
              </tr>
            </thead>
            <tbody>
              <tr>
                <td data-label="Agent">
                  <a href="#/agents/publishing">
                    <strong>{c.data.name}</strong>
                  </a>
                </td>
                <td data-label="Connection">
                  <ConnectionBadge c={c.data} />
                </td>
              </tr>
            </tbody>
          </table>
        </section>
      )}
    </>
  );
}
function AgentDetail() {
  const c = useConnection(),
    y = useYoutube();
  if (c.isPending || y.isPending) return <Loading />;
  if (c.error || y.error) return <ErrorMessage error={c.error || y.error} />;
  return (
    <>
      <Back to="agents">Agents</Back>
      <Heading title={c.data.name}>
        <ConnectionBadge c={c.data} />
      </Heading>
      <AgentControls connection={c.data} youtube={y.data} />
    </>
  );
}
function AgentControls({
  connection: c,
  youtube: y,
}: {
  connection: BridgeConnection;
  youtube: YoutubeStatus;
}) {
  const [permission, setPermission] = useState<boolean | null>(null);
  const [confirm, setConfirm] = useState(false);
  const change = useMutation({
    mutationFn: ({ action, body }: { action: string; body: unknown }) =>
      request<BridgeConnection>(`/api/publishing/connection/${action}`, body),
    onSuccess: (r) => {
      cache.setQueryData(["bridge-connection"], r);
      setPermission(null);
      setConfirm(false);
    },
  });
  return (
    <div className="narrow stack">
      {c.state !== "Disconnected" && y.account && (
        <section className="panel">
          <div className="panel-title">
            <h2>Platform permissions</h2>
          </div>
          <div className="pad">
            <h3>YouTube</h3>
            <p>{y.account.name}</p>
            <label className="check">
              <input
                type="checkbox"
                checked={permission ?? c.publish_enabled}
                onChange={(e) => setPermission(e.target.checked)}
              />
              Publish videos
            </label>
            <div className="actions">
              <button
                disabled={
                  permission === null ||
                  permission === c.publish_enabled ||
                  change.isPending
                }
                onClick={() =>
                  change.mutate({
                    action: "access",
                    body: { publish_enabled: permission },
                  })
                }
              >
                Save permissions
              </button>
            </div>
          </div>
        </section>
      )}
      {c.state !== "Disconnected" && <ConnectionInstructions status={y} />}
      {c.state === "Disconnected" ? (
        <button
          className="primary"
          disabled={change.isPending}
          onClick={() => change.mutate({ action: "enable", body: {} })}
        >
          Set up connection
        </button>
      ) : confirm ? (
        <section className="panel pad">
          <h2>Disconnect {c.name}?</h2>
          <p>
            Clients using this connection’s token will lose access. An upload
            already sent to YouTube may finish.
          </p>
          <div className="actions">
            <button
              className="danger"
              disabled={change.isPending}
              onClick={() => change.mutate({ action: "disconnect", body: {} })}
            >
              Disconnect agent
            </button>
            <button onClick={() => setConfirm(false)}>Cancel</button>
          </div>
        </section>
      ) : (
        <button className="danger" onClick={() => setConfirm(true)}>
          Disconnect
        </button>
      )}
      <ErrorMessage error={change.error} />
    </div>
  );
}
function Platforms() {
  const y = useYoutube(),
    c = useConnection();
  return (
    <>
      <Heading title="Platforms">
        {!y.data?.account && (
          <a className="button primary" href="#/platforms/youtube">
            Connect platform
          </a>
        )}
      </Heading>
      {y.isPending || c.isPending ? (
        <Loading />
      ) : y.error || c.error ? (
        <ErrorMessage error={y.error || c.error} />
      ) : y.data.account ? (
        <section className="panel">
          <table className="platform-table">
            <thead>
              <tr>
                {["Platform", "Account", "Status", "Agents"].map((x) => (
                  <th key={x}>{x}</th>
                ))}
              </tr>
            </thead>
            <tbody>
              <tr>
                <td data-label="Platform">
                  <a className="platform-name" href="#/platforms/youtube">
                    YouTube
                  </a>
                </td>
                <td data-label="Account">{y.data.account.name}</td>
                <td data-label="Status">
                  <Badge value="Connected" />
                </td>
                <td data-label="Agents">
                  {c.data.publish_enabled && c.data.state !== "Disconnected" ? (
                    <a href="#/agents/publishing">{c.data.name}</a>
                  ) : (
                    "None"
                  )}
                </td>
              </tr>
            </tbody>
          </table>
        </section>
      ) : (
        <div className="empty">
          <h2>No connected platforms</h2>
        </div>
      )}
    </>
  );
}
function Youtube() {
  const y = useYoutube(),
    c = useConnection();
  const [confirm, setConfirm] = useState(false);
  const connect = useMutation({
    mutationFn: () => request<{ url: string }>("/api/youtube/connect", {}),
    onSuccess: ({ url }) => window.location.assign(url),
  });
  const disconnect = useMutation({
    mutationFn: () => request<YoutubeStatus>("/api/youtube/disconnect", {}),
    onSuccess: (s) => {
      cache.setQueryData(["youtube"], s);
      setConfirm(false);
    },
  });
  if (y.isPending) return <Loading />;
  if (y.error) return <ErrorMessage error={y.error} />;
  const s = y.data;
  return (
    <>
      <Back to="platforms">Platforms</Back>
      <Heading title="YouTube">
        {s.account && <Badge value="Connected" />}
      </Heading>
      <div className="narrow stack">
        {s.account ? (
          <>
            <section className="panel pad">
              <dl>
                <dt>Account</dt>
                <dd>{s.account.name}</dd>
                <dt>Video management</dt>
                <dd>
                  {s.video_management_authorized
                    ? "Authorized"
                    : "Not authorized"}
                </dd>
              </dl>
              {s.video_management_authorized === false && (
                <div className="stack">
                  <p>
                    Authorize video management to let agents change published
                    videos’ visibility.
                  </p>
                  <button
                    className="primary"
                    disabled={connect.isPending}
                    onClick={() => connect.mutate()}
                  >
                    Authorize video management
                  </button>
                </div>
              )}
            </section>
            <section className="panel">
              <div className="panel-title">
                <h2>Agents</h2>
              </div>
              <div className="pad">
                {c.isPending ? (
                  <Loading />
                ) : c.error ? (
                  <ErrorMessage error={c.error} />
                ) : c.data.publish_enabled &&
                  c.data.state !== "Disconnected" ? (
                  <a href="#/agents/publishing">{c.data.name}</a>
                ) : (
                  <p>None</p>
                )}
              </div>
            </section>
            {confirm ? (
              <section className="panel pad">
                <h2>Disconnect YouTube?</h2>
                <p>
                  New uploads will stop. Published videos remain on YouTube.
                </p>
                <div className="actions">
                  <button
                    className="danger"
                    disabled={disconnect.isPending}
                    onClick={() => disconnect.mutate()}
                  >
                    Disconnect YouTube
                  </button>
                  <button onClick={() => setConfirm(false)}>Cancel</button>
                </div>
              </section>
            ) : (
              <button className="danger" onClick={() => setConfirm(true)}>
                Disconnect YouTube
              </button>
            )}
          </>
        ) : (
          <section className="panel pad">
            {s.configured ? (
              <>
                <button
                  className="primary"
                  disabled={connect.isPending}
                  onClick={() => connect.mutate()}
                >
                  Connect YouTube
                </button>
                <details>
                  <summary>Google application credentials</summary>
                  <GoogleConfig />
                </details>
              </>
            ) : (
              <GoogleConfig />
            )}
          </section>
        )}
        <ErrorMessage error={connect.error || disconnect.error} />
      </div>
    </>
  );
}
function Tasks({ filter }: { filter: string }) {
  const loaded = usePublications(filter);
  return (
    <>
      <Heading title="Tasks" />
      <div className="toolbar" aria-label="Task filters">
        {[
          ["all", "All tasks"],
          ["attention", "Needs attention"],
          ["complete", "Uploaded"],
        ].map(([value, label]) => (
          <a
            key={value}
            className={filter === value ? "selected" : ""}
            href={`#/tasks?state=${value}`}
          >
            {label}
          </a>
        ))}
      </div>
      {loaded.isPending ? (
        <Loading />
      ) : loaded.error ? (
        <ErrorMessage error={loaded.error} />
      ) : loaded.ids.length ? (
        <section className="panel task-list">
          {loaded.ids.map((id) => (
            <TaskRow key={id} id={id} filter={filter} />
          ))}
        </section>
      ) : (
        <div className="empty">
          <h2>No tasks yet</h2>
        </div>
      )}
    </>
  );
}
function Retry({ id }: { id: string }) {
  const r = useMutation({
    mutationFn: () => request<Publication>(`/api/publications/${id}/retry`, {}),
    onSuccess: upsertPublication,
  });
  return (
    <>
      <button disabled={r.isPending} onClick={() => r.mutate()}>
        {r.isPending ? "Retrying…" : "Retry upload"}
      </button>
      <ErrorMessage error={r.error} />
    </>
  );
}
function TaskRow({ id, filter }: { id: string; filter: string }) {
  const { data: p } = usePublication(id);
  if (
    !p ||
    (filter === "attention" && p.status !== "interrupted") ||
    (filter === "complete" && p.status !== "uploaded")
  )
    return null;
  return (
    <article className="task-row">
      <div>
        <a href={`#/tasks/${id}`}>
          <strong>{p.title}</strong>
        </a>
        <time dateTime={p.created_at}>
          {new Date(p.created_at).toLocaleString()}
        </time>
        {p.status === "uploading" && (
          <progress
            aria-label={`Upload progress for ${p.title}`}
            value={p.uploaded_bytes}
            max={p.total_bytes}
          />
        )}{" "}
        {p.error && <p className="error">{p.error}</p>}
      </div>
      <Badge value={states[p.status]} />
      {p.status === "interrupted" && <Retry id={id} />}
    </article>
  );
}
interface Detail {
  publication: Publication;
  settings: {
    privacy: string;
    made_for_kids: boolean;
    contains_synthetic_media: boolean;
    description: string;
  };
  channel_id: string;
}
function TaskDetail({ id }: { id: string }) {
  const d = useQuery({
    queryKey: ["publication-detail", id],
    queryFn: async () => {
      const r = await request<Detail>(`/api/publications/${id}`);
      upsertPublication(r.publication);
      return r;
    },
  });
  const { data: live } = usePublication(id);
  const verified = useMutation({
    mutationFn: () =>
      request<{
        actual_privacy: string | null;
        visibility_error: string | null;
      }>(`/api/publications/${id}/visibility`),
  });
  if (d.isPending) return <Loading />;
  if (d.error) return <ErrorMessage error={d.error} />;
  const p = live ?? d.data.publication,
    s = d.data.settings;
  return (
    <>
      <Back to="tasks">Tasks</Back>
      <Heading title={p.title}>
        <Badge value={states[p.status]} />
      </Heading>
      <div className="narrow stack">
        <section className="panel pad">
          <dl>
            <dt>Platform</dt>
            <dd>YouTube</dd>
            <dt>Created</dt>
            <dd>{new Date(p.created_at).toLocaleString()}</dd>
            <dt>Transferred</dt>
            <dd>
              {p.uploaded_bytes.toLocaleString()} /{" "}
              {p.total_bytes.toLocaleString()} bytes
            </dd>
          </dl>
          {p.error && (
            <p className="error" role="alert">
              {p.error}
            </p>
          )}
          <div className="actions">
            {p.video_url && (
              <a
                className="button"
                href={p.video_url}
                target="_blank"
                rel="noreferrer"
              >
                Open on YouTube <ExternalLink size={14} />
              </a>
            )}
            {p.status === "interrupted" && <Retry id={id} />}
            <a className="button" href={`#/activity?task=${id}`}>
              View activity
            </a>
          </div>
        </section>
        <section className="panel">
          <div className="panel-title">
            <h2>Video settings</h2>
          </div>
          <div className="pad">
            <dl>
              <dt>Requested visibility</dt>
              <dd>{s.privacy}</dd>
              <dt>Made for kids</dt>
              <dd>{s.made_for_kids ? "Yes" : "No"}</dd>
              <dt>Altered or synthetic content</dt>
              <dd>{s.contains_synthetic_media ? "Yes" : "No"}</dd>
              {s.description && (
                <>
                  <dt>Description</dt>
                  <dd className="preserve-lines">{s.description}</dd>
                </>
              )}
            </dl>
            {p.video_url && (
              <div className="actions">
                <button
                  disabled={verified.isPending}
                  onClick={() => verified.mutate()}
                >
                  {verified.isPending
                    ? "Checking…"
                    : "Check YouTube visibility"}
                </button>
                {verified.data && (
                  <span role="status">
                    {verified.data.actual_privacy
                      ? `YouTube visibility: ${verified.data.actual_privacy}`
                      : verified.data.visibility_error ||
                        "Visibility unavailable"}
                  </span>
                )}
              </div>
            )}
            <ErrorMessage error={verified.error} />
          </div>
        </section>
      </div>
    </>
  );
}
export interface Journal {
  items: {
    sequence: number;
    event_at?: string | null;
    publication: Publication;
  }[];
  next: number | null;
}
function JournalSteps({ id }: { id: string }) {
  const h = useInfiniteQuery({
    queryKey: ["history", id],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      request<Journal>(`/api/publications/${id}/history?after=${pageParam}`),
    getNextPageParam: (last) => last.next ?? undefined,
  });
  if (h.isPending) return <Loading />;
  if (h.error) return <ErrorMessage error={h.error} />;
  return (
    <div className="journal">
      <ol>
        {h.data.pages
          .flatMap((p) => p.items)
          .map(({ sequence, event_at, publication: p }) => (
            <li key={sequence}>
              {event_at && (
                <time dateTime={event_at}>
                  {new Date(event_at).toLocaleString()}
                </time>
              )}
              <strong>{states[p.status]}</strong>
              <span>
                {p.uploaded_bytes.toLocaleString()} /{" "}
                {p.total_bytes.toLocaleString()} bytes
              </span>
              {p.error && <p className="error">{p.error}</p>}
            </li>
          ))}
      </ol>
      {!h.data.pages[0].items.length && <p>No recorded steps</p>}
      {h.hasNextPage && (
        <button
          disabled={h.isFetchingNextPage}
          onClick={() => void h.fetchNextPage()}
        >
          Load more steps
        </button>
      )}
      <a href={`#/tasks/${id}`}>View task</a>
    </div>
  );
}
function ActivityItem({ id, errorsOnly }: { id: string; errorsOnly: boolean }) {
  const { data: p } = usePublication(id);
  // Opened chains stay mounted during updates; filtering uses current recoverable failures.
  if (!p || (errorsOnly && p.status !== "interrupted")) return null;
  return (
    <details className="operation">
      <summary>
        <div>
          <strong>Upload video</strong>
          <span>{p.title}</span>
        </div>
        <span>YouTube</span>
        <time dateTime={p.created_at}>
          {new Date(p.created_at).toLocaleString()}
        </time>
        <Badge value={states[p.status]} />
      </summary>
      <JournalOnOpen id={id} />
    </details>
  );
}
function JournalOnOpen({ id }: { id: string }) {
  // Lazy history fetch uses native details' toggle event through a small observer component.
  const [opened, setOpened] = useState(false);
  return (
    <div
      ref={(node) => {
        if (!node) return;
        const parent = node.parentElement as HTMLDetailsElement;
        parent.ontoggle = () => setOpened(parent.open);
      }}
    >
      {opened && <JournalSteps id={id} />}
    </div>
  );
}
function Activity({
  task,
  errorsOnly,
}: {
  task: string | null;
  errorsOnly: boolean;
}) {
  const loaded = usePublications();
  return (
    <>
      <Heading title="Activity log" />
      <div className="toolbar">
        <a
          className={!errorsOnly ? "selected" : ""}
          href={`#/activity${task ? `?task=${task}` : ""}`}
        >
          All activity
        </a>
        <a
          className={errorsOnly ? "selected" : ""}
          href={`#/activity?errors=1${task ? `&task=${task}` : ""}`}
        >
          Needs attention
        </a>
        {task && <a href="#/activity">Clear task filter</a>}
      </div>
      {loaded.isPending ? (
        <Loading />
      ) : loaded.error ? (
        <ErrorMessage error={loaded.error} />
      ) : (
        <section className="panel">
          {(task ? loaded.ids.filter((id) => id === task) : loaded.ids).map(
            (id) => (
              <ActivityItem key={id} id={id} errorsOnly={errorsOnly} />
            ),
          )}
          {!loaded.ids.length && (
            <div className="empty">
              <h2>No activity yet</h2>
            </div>
          )}
        </section>
      )}
    </>
  );
}
function SettingsPage() {
  const [theme, setTheme] = useState(
    document.documentElement.dataset.theme || "dark",
  );
  const y = useYoutube();
  const save = useMutation({
    mutationFn: (url: string) =>
      request<YoutubeStatus>("/api/publishing/bridge", { url }),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  const policy = useMutation({
    mutationFn: (private_only: boolean) =>
      request<YoutubeStatus>("/api/youtube/policy", { private_only }),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  return (
    <>
      <Heading title="Settings" />
      <div className="narrow stack">
        <section className="panel">
          <div className="panel-title">
            <h2>Appearance</h2>
          </div>
          <fieldset className="pad theme-options">
            <legend className="sr-only">Theme</legend>
            {[
              ["light", "Light", Sun],
              ["dark", "Dark", Moon],
            ].map(([value, label, Icon]) => {
              const Symbol = Icon as typeof Sun;
              return (
                <label key={value as string} className="theme-choice">
                  <input
                    type="radio"
                    name="theme"
                    checked={theme === value}
                    onChange={() => {
                      const v = value as string;
                      document.documentElement.dataset.theme = v;
                      try {
                        localStorage.setItem("agentway-theme", v);
                      } catch {}
                      setTheme(v);
                    }}
                  />
                  <Symbol size={20} />
                  {label as string}
                </label>
              );
            })}
          </fieldset>
        </section>
        {y.isPending ? (
          <Loading />
        ) : y.error ? (
          <ErrorMessage error={y.error} />
        ) : (
          <>
            <section className="panel">
              <div className="panel-title">
                <h2>Agent endpoint</h2>
              </div>
              <form
                className="pad"
                onSubmit={(e) => {
                  e.preventDefault();
                  save.mutate(
                    String(new FormData(e.currentTarget).get("url") ?? ""),
                  );
                }}
              >
                <label>
                  Public HTTPS address
                  <input
                    name="url"
                    type="url"
                    defaultValue={y.data.bridge_url}
                  />
                </label>
                <div className="actions">
                  <button disabled={save.isPending}>Save address</button>
                  {save.isSuccess && <span role="status">Saved</span>}
                </div>
                <ErrorMessage error={save.error} />
              </form>
            </section>
            <section className="panel">
              <div className="panel-title">
                <h2>Publishing restrictions</h2>
              </div>
              <div className="pad">
                <label className="check">
                  <input
                    type="checkbox"
                    checked={y.data.private_only}
                    disabled={policy.isPending}
                    onChange={(e) => policy.mutate(e.target.checked)}
                  />
                  Restrict uploads to private
                </label>
                <ErrorMessage error={policy.error} />
              </div>
            </section>
          </>
        )}
      </div>
    </>
  );
}
export function Workspace({ route }: { route: string }) {
  const [path, search] = route.split("?");
  const query = new URLSearchParams(search);
  if (path === "agents") return <Agents />;
  if (path === "agents/publishing" || path === "agents/connect")
    return <AgentDetail />;
  if (path === "platforms") return <Platforms />;
  if (path === "platforms/youtube" || path === "platforms/connect")
    return <Youtube />;
  if (path === "tasks") return <Tasks filter={query.get("state") || "all"} />;
  if (path.startsWith("tasks/"))
    return <TaskDetail key={path} id={path.split("/")[1]} />;
  if (path === "activity")
    return (
      <Activity
        task={query.get("task")}
        errorsOnly={query.get("errors") === "1"}
      />
    );
  if (path === "settings") return <SettingsPage />;
  return (
    <>
      <Heading title="Page not found" />
      <a href="#/agents">Go to Agents</a>
    </>
  );
}
