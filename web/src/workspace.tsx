import { Fragment, useState, type ReactNode } from "react";
import { useInfiniteQuery, useMutation, useQuery } from "@tanstack/react-query";
import { Moon, Sun, ArrowLeft, ExternalLink, ChevronRight } from "lucide-react";
import { cache, request } from "./api";
import {
  useConnections,
  upsertConnection,
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
  const c = useConnections();
  return (
    <>
      <Heading title="Agents">
        <a className="button primary" href="#/agents/connect">
          Connect agent
        </a>
      </Heading>
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
              {c.data.map((connection) => (
                <tr key={connection.id}>
                  <td data-label="Agent">
                    <a href={`#/agents/${connection.id}`}>
                      <strong>{connection.name}</strong>
                    </a>
                  </td>
                  <td data-label="Connection">
                    <ConnectionBadge c={connection} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}
    </>
  );
}
function AgentDetail({ id }: { id: string }) {
  const c = useConnections(),
    y = useYoutube();
  if (c.isPending || y.isPending) return <Loading />;
  if (c.error || y.error) return <ErrorMessage error={c.error || y.error} />;
  const connection = c.data.find((c) => c.id === id);
  if (!connection)
    return (
      <>
        <Back to="agents">Agents</Back>
        <p role="alert">Connection not found.</p>
      </>
    );
  return (
    <>
      <Back to="agents">Agents</Back>
      <Heading title={connection.name}>
        <ConnectionBadge c={connection} />
      </Heading>
      <AgentControls
        key={connection.id}
        connection={connection}
        youtube={y.data}
      />
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
      request<BridgeConnection>(
        `/api/agent-connections/${c.id}/${action}`,
        body,
      ),
    onSuccess: (r) => {
      upsertConnection(r);
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
              Publish and manage YouTube
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
      {c.state !== "Disconnected" && (
        <ConnectionInstructions
          key={c.id + c.state}
          status={y}
          connection={c}
        />
      )}
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
function AgentLinks({ connections }: { connections: BridgeConnection[] }) {
  const enabled = connections.filter(
    (c) => c.publish_enabled && c.state === "Connected",
  );
  return enabled.length ? (
    <>
      {enabled.map((c, i) => (
        <Fragment key={c.id}>
          {i > 0 && ", "}
          <a href={`#/agents/${c.id}`}>{c.name}</a>
        </Fragment>
      ))}
    </>
  ) : (
    <>None</>
  );
}
function ConnectAgent() {
  const [product, setProduct] = useState("Grok Bot");
  const [permission, setPermission] = useState(false);
  const y = useYoutube();
  const create = useMutation({
    mutationFn: () =>
      request<BridgeConnection>("/api/agent-connections", {
        product,
        publish_enabled: permission,
      }),
    onSuccess: (c) => {
      upsertConnection(c);
      window.location.hash = `/agents/${c.id}`;
    },
  });
  return (
    <>
      <Back to="agents">Agents</Back>
      <Heading title="Connect agent" />
      <form
        className="settings-list"
        onSubmit={(e) => {
          e.preventDefault();
          create.mutate();
        }}
      >
        <div className="setting-row">
          <h2>
            <label htmlFor="agent-product">Agent</label>
          </h2>
          <div className="connection-field">
            <select
              id="agent-product"
              value={product}
              onChange={(e) => setProduct(e.target.value)}
            >
              <option>Grok Bot</option>
              <option>Muse</option>
              <option>Claude</option>
              <option>ChatGPT</option>
            </select>
          </div>
        </div>
        {y.data?.account && (
          <div className="setting-row">
            <h2>YouTube</h2>
            <div className="connection-field">
              <span>{y.data.account.name}</span>
              <label className="settings-check">
                <input
                  type="checkbox"
                  checked={permission}
                  onChange={(e) => setPermission(e.target.checked)}
                />
                Publish and manage YouTube
              </label>
            </div>
          </div>
        )}
        <div className="setting-row">
          <span />
          <div className="connection-field">
            <button
              className="primary"
              disabled={create.isPending || y.isPending || !!y.error}
            >
              {create.isPending ? "Creating…" : "Continue"}
            </button>
            <ErrorMessage error={create.error || y.error} />
          </div>
        </div>
      </form>
    </>
  );
}
function Platforms() {
  const y = useYoutube(),
    c = useConnections();
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
                  <AgentLinks connections={c.data} />
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
    c = useConnections();
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
                ) : (
                  <AgentLinks connections={c.data} />
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
        <section className="panel">
          <WorkTable kind="Task">
            {loaded.ids.map((id) => (
              <TaskRow key={id} id={id} filter={filter} />
            ))}
          </WorkTable>
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
function WorkTable({ kind, children }: { kind: string; children: ReactNode }) {
  return (
    <table className="work-table">
      <colgroup>
        <col className="work-title" />
        <col />
        <col />
        <col className="work-date" />
        <col />
      </colgroup>
      <thead>
        <tr>
          {[kind, "Agent", "Platform", "Created", "Status"].map((label) => (
            <th key={label} scope="col">
              {label}
            </th>
          ))}
        </tr>
      </thead>
      <tbody>{children}</tbody>
    </table>
  );
}
function WorkMetadata({ p }: { p: Publication }) {
  return (
    <>
      <td data-label="Agent">{p.agent_name || "Not recorded"}</td>
      <td data-label="Platform">YouTube</td>
      <td data-label="Created">
        <time dateTime={p.created_at}>
          {new Date(p.created_at).toLocaleString()}
        </time>
      </td>
      <td data-label="Status">
        <Badge value={states[p.status]} />
      </td>
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
    <tr>
      <td data-label="Task">
        <a href={`#/tasks/${id}`} className="work-title-link">
          {p.title}
        </a>
        {p.status === "uploading" && (
          <progress
            aria-label={`Upload progress for ${p.title}`}
            value={p.uploaded_bytes}
            max={p.total_bytes}
          />
        )}
        {p.error && <p className="error">{p.error}</p>}
        {p.status === "interrupted" && (
          <div className="work-retry">
            <Retry id={id} />
          </div>
        )}
      </td>
      <WorkMetadata p={p} />
    </tr>
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
            <dt>Agent</dt>
            <dd>{p.agent_name || "Not recorded"}</dd>
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
  const [open, setOpen] = useState(false);
  if (!p || (errorsOnly && p.status !== "interrupted")) return null;
  return (
    <>
      <tr className={open ? "work-expanded" : ""}>
        <td data-label="Activity">
          <button
            className="work-expand"
            aria-expanded={open}
            aria-controls={`history-${id}`}
            onClick={() => setOpen(!open)}
          >
            <ChevronRight size={16} aria-hidden="true" />
            <span>
              <span className="work-title-link">{p.title}</span>
              <span className="work-operation">Upload video</span>
            </span>
          </button>
        </td>
        <WorkMetadata p={p} />
      </tr>
      {open && (
        <tr className="work-history" id={`history-${id}`}>
          <td colSpan={5}>
            <JournalSteps id={id} />
          </td>
        </tr>
      )}
    </>
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
          <WorkTable kind="Activity">
            {(task ? loaded.ids.filter((id) => id === task) : loaded.ids).map(
              (id) => (
                <ActivityItem key={id} id={id} errorsOnly={errorsOnly} />
              ),
            )}
          </WorkTable>
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
  const [address, setAddress] = useState<string | null>(null);
  const y = useYoutube();
  const save = useMutation({
    mutationFn: (url: string) =>
      request<YoutubeStatus>("/api/publishing/bridge", { url }),
    onSuccess: (s) => {
      cache.setQueryData(["youtube"], s);
      setAddress(null);
    },
  });
  const policy = useMutation({
    mutationFn: (private_only: boolean) =>
      request<YoutubeStatus>("/api/youtube/policy", { private_only }),
    onSuccess: (s) => cache.setQueryData(["youtube"], s),
  });
  const value = address ?? y.data?.bridge_url ?? "";
  const dirty = value.trim() !== (y.data?.bridge_url ?? "");
  return (
    <>
      <Heading title="Settings" />
      <div className="settings-list">
        <section className="setting-row" aria-labelledby="appearance-label">
          <h2 id="appearance-label">Appearance</h2>
          <fieldset className="settings-theme">
            <legend className="sr-only">Theme</legend>
            {(
              [
                ["light", "Light", Sun],
                ["dark", "Dark", Moon],
              ] as const
            ).map(([value, label, Icon]) => (
              <label key={value}>
                <input
                  className="sr-only"
                  type="radio"
                  name="theme"
                  checked={theme === value}
                  onChange={() => {
                    document.documentElement.dataset.theme = value;
                    try {
                      localStorage.setItem("agentway-theme", value);
                    } catch {}
                    setTheme(value);
                  }}
                />
                <span>
                  <Icon size={16} aria-hidden="true" />
                  {label}
                </span>
              </label>
            ))}
          </fieldset>
        </section>
        {y.isPending ? (
          <Loading />
        ) : y.error ? (
          <ErrorMessage error={y.error} />
        ) : (
          <>
            <section className="setting-row">
              <h2>
                <label htmlFor="agent-address">Agent endpoint</label>
              </h2>
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  if (dirty) save.mutate(value.trim());
                }}
              >
                <div className="settings-address">
                  <input
                    id="agent-address"
                    name="url"
                    type="url"
                    aria-label="Agent endpoint"
                    value={value}
                    disabled={save.isPending}
                    onChange={(e) => {
                      setAddress(e.target.value);
                      save.reset();
                    }}
                  />
                  <button disabled={!dirty || save.isPending}>
                    {save.isPending ? "Saving…" : "Save"}
                  </button>
                </div>
                {save.isSuccess && (
                  <span className="setting-feedback" role="status">
                    Saved
                  </span>
                )}
                <ErrorMessage error={save.error} />
              </form>
            </section>
            <section className="setting-row">
              <h2>YouTube</h2>
              <div>
                <label className="settings-check">
                  <input
                    type="checkbox"
                    checked={y.data.private_only}
                    disabled={policy.isPending}
                    onChange={(e) => policy.mutate(e.target.checked)}
                  />
                  Restrict all agent uploads to private
                </label>
                {policy.isPending && (
                  <span className="setting-feedback" role="status">
                    Saving…
                  </span>
                )}
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
  if (path === "agents/connect") return <ConnectAgent />;
  if (path.startsWith("agents/"))
    return <AgentDetail key={path} id={path.split("/")[1]} />;
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
