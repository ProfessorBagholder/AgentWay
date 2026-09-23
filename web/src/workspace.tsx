import { Fragment, useState, type ReactNode } from "react";
import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  type InfiniteData,
} from "@tanstack/react-query";
import { Moon, Sun, ArrowLeft, ExternalLink, ChevronRight } from "lucide-react";
import { cache, request } from "./api";
import {
  useConnections,
  upsertConnection,
  useYoutube,
  usePublications,
  usePublication,
  useTransfers,
  type MediaTransfer,
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
export interface AgentHandoff {
  cursor: number;
  id: string;
  request_id: string;
  sender_id: string;
  sender_name: string;
  recipient_id: string;
  recipient_name: string;
  title: string;
  instructions: string;
  status: "queued" | "claimed" | "completed" | "failed" | "cancelled";
  result: string | null;
  error: string | null;
  lease_until: number | null;
  created_at: string;
  updated_at: string;
  revision: number;
}
interface HandoffPage {
  items: AgentHandoff[];
  next: number | null;
}
interface HandoffGrant {
  sender_id: string;
  recipient_id: string;
}
function useHandoffs() {
  const loaded = useInfiniteQuery<HandoffPage, Error>({
    queryKey: ["agent-handoffs"],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      request<HandoffPage>(
        `/api/agent-handoffs${pageParam ? `?before=${pageParam}` : ""}`,
      ),
    getNextPageParam: (last) => last.next ?? undefined,
  });
  return {
    ...loaded,
    rows: loaded.data?.pages.flatMap((page) => page.items) ?? [],
  };
}
const handoffStates: Record<AgentHandoff["status"], string> = {
  queued: "Queued",
  claimed: "In progress",
  completed: "Completed",
  failed: "Needs attention",
  cancelled: "Cancelled",
};
export function upsertHandoff(row: AgentHandoff) {
  cache.setQueryData<InfiniteData<HandoffPage>>(["agent-handoffs"], (old) => {
    if (!old) return old;
    const prior = old.pages
      .flatMap((page) => page.items)
      .find((task) => task.id === row.id);
    if (prior && prior.revision >= row.revision) return old;
    if (!prior && row.cursor <= (old.pages[0]?.items[0]?.cursor ?? 0))
      return old;
    return {
      ...old,
      pages: old.pages.map((page, index) => ({
        ...page,
        items: prior
          ? page.items.map((task) => (task.id === row.id ? row : task))
          : index === 0
            ? [row, ...page.items]
            : page.items,
      })),
    };
  });
  cache.setQueryData<AgentHandoff>(["agent-handoff", row.id], (old) =>
    old && old.revision >= row.revision ? old : row,
  );
}
function Badge({ value }: { value: string }) {
  return (
    <span
      className={`badge ${["Connected", "Uploaded", "Ready"].includes(value) ? "good" : value === "Needs attention" ? "warning" : ""}`}
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
      <AgentsTabs active="connections" />
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
function AgentsTabs({ active }: { active: "connections" | "access" }) {
  return (
    <div className="toolbar" aria-label="Agents sections">
      <a
        className={active === "connections" ? "selected" : ""}
        aria-current={active === "connections" ? "page" : undefined}
        href="#/agents"
      >
        Connections
      </a>
      <a
        className={active === "access" ? "selected" : ""}
        aria-current={active === "access" ? "page" : undefined}
        href="#/agents/access"
      >
        Task access
      </a>
    </div>
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
function AgentTaskAccess() {
  const connections = useConnections();
  const grants = useQuery<HandoffGrant[]>({
    queryKey: ["agent-handoff-grants"],
    queryFn: () => request<HandoffGrant[]>("/api/agent-handoff-grants"),
  });
  const [sender, setSender] = useState("");
  const [recipient, setRecipient] = useState("");
  const [search, setSearch] = useState("");
  const change = useMutation({
    mutationFn: ({
      sender_id,
      recipient_id,
      enabled,
    }: HandoffGrant & { enabled: boolean }) =>
      request<string[]>(`/api/agent-handoff-grants/${sender_id}`, {
        recipient_id,
        enabled,
      }),
    onSuccess: (_ids, changed) => {
      cache.setQueryData<HandoffGrant[]>(
        ["agent-handoff-grants"],
        (old = []) =>
          changed.enabled
            ? [
                ...old,
                {
                  sender_id: changed.sender_id,
                  recipient_id: changed.recipient_id,
                },
              ]
            : old.filter(
                (grant) =>
                  grant.sender_id !== changed.sender_id ||
                  grant.recipient_id !== changed.recipient_id,
              ),
      );
      if (changed.enabled) setRecipient("");
    },
  });
  const connected = connections.data.filter((c) => c.state === "Connected");
  const names = new Map(connections.data.map((c) => [c.id, c.name]));
  const label = (id: string) => {
    const name = names.get(id) ?? "Disconnected agent";
    return connections.data.filter((c) => c.name === name).length > 1
      ? `${name} · ${id.slice(0, 8)}`
      : name;
  };
  const eligible = connected.filter(
    (c) =>
      c.id !== sender &&
      !grants.data?.some(
        (grant) => grant.sender_id === sender && grant.recipient_id === c.id,
      ),
  );
  const visible = (grants.data ?? [])
    .filter((grant) =>
      `${label(grant.sender_id)} ${label(grant.recipient_id)}`
        .toLocaleLowerCase()
        .includes(search.trim().toLocaleLowerCase()),
    )
    .sort((a, b) =>
      `${label(a.sender_id)} ${label(a.recipient_id)}`.localeCompare(
        `${label(b.sender_id)} ${label(b.recipient_id)}`,
      ),
    );
  return (
    <>
      <Heading title="Agents">
        <a className="button primary" href="#/agents/connect">
          Connect agent
        </a>
      </Heading>
      <AgentsTabs active="access" />
      {connections.isPending || grants.isPending ? (
        <Loading />
      ) : connections.error || grants.error ? (
        <ErrorMessage error={connections.error || grants.error} />
      ) : connected.length < 2 ? (
        <p>
          <a href="#/agents/connect">Connect another agent</a> to set task
          access.
        </p>
      ) : (
        <>
          <form
            className="access-form"
            onSubmit={(event) => {
              event.preventDefault();
              if (sender && recipient && sender !== recipient)
                change.mutate({
                  sender_id: sender,
                  recipient_id: recipient,
                  enabled: true,
                });
            }}
          >
            <label>
              Assigning agent
              <select
                value={sender}
                onChange={(event) => {
                  setSender(event.target.value);
                  setRecipient("");
                }}
              >
                <option value="">Select agent</option>
                {connected.map((c) => (
                  <option key={c.id} value={c.id}>
                    {label(c.id)}
                  </option>
                ))}
              </select>
            </label>
            <label>
              Receiving agent
              <select
                value={recipient}
                disabled={!sender}
                onChange={(event) => setRecipient(event.target.value)}
              >
                <option value="">Select agent</option>
                {eligible.map((c) => (
                  <option key={c.id} value={c.id}>
                    {label(c.id)}
                  </option>
                ))}
              </select>
            </label>
            <button
              className="primary"
              disabled={!sender || !recipient || change.isPending}
            >
              Allow task assignment
            </button>
          </form>
          <ErrorMessage error={change.error} />
          {!!grants.data.length && (
            <>
              {grants.data.length > 8 && (
                <label className="access-search">
                  Search task access
                  <input
                    type="search"
                    value={search}
                    onChange={(event) => setSearch(event.target.value)}
                  />
                </label>
              )}
              {visible.length ? (
                <section className="panel">
                  <table className="agent-table access-table">
                    <thead>
                      <tr>
                        <th>Assigning agent</th>
                        <th>Receiving agent</th>
                        <th>Action</th>
                      </tr>
                    </thead>
                    <tbody>
                      {visible.map((grant) => (
                        <tr key={`${grant.sender_id}:${grant.recipient_id}`}>
                          <td data-label="Assigning agent">
                            <a href={`#/agents/${grant.sender_id}`}>
                              {label(grant.sender_id)}
                            </a>
                          </td>
                          <td data-label="Receiving agent">
                            <a href={`#/agents/${grant.recipient_id}`}>
                              {label(grant.recipient_id)}
                            </a>
                          </td>
                          <td data-label="Action">
                            <button
                              disabled={change.isPending}
                              aria-label={`Remove task access from ${label(grant.sender_id)} to ${label(grant.recipient_id)}`}
                              onClick={() =>
                                change.mutate({ ...grant, enabled: false })
                              }
                            >
                              Remove
                            </button>
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </section>
              ) : (
                <p>No matches</p>
              )}
            </>
          )}
        </>
      )}
    </>
  );
}
function Tasks({ filter }: { filter: string }) {
  const loaded = usePublications(filter);
  const transfers = useTransfers();
  const handoffs = useHandoffs();
  const handoffRows = handoffs.rows;
  const standalone =
    transfers.data?.filter(
      (t) =>
        !t.has_publication &&
        (filter === "all" ||
          (filter === "attention" &&
            ["checksum_mismatch", "interrupted"].includes(t.status)) ||
          (filter === "complete" && t.status === "ready")),
    ) ?? [];
  const records = [
    ...loaded.ids.map((id) => ({
      kind: "publication" as const,
      id,
      created:
        cache.getQueryData<Publication>(["publication", id])?.created_at ?? "",
    })),
    ...standalone.map((t) => ({
      kind: "transfer" as const,
      id: t.id,
      created: t.created_at ?? "",
    })),
    ...handoffRows
      .filter(
        (task) =>
          filter === "all" ||
          (filter === "attention" && task.status === "failed") ||
          (filter === "complete" && task.status === "completed"),
      )
      .map((task) => ({
        kind: "handoff" as const,
        id: task.id,
        created: task.created_at,
      })),
  ].sort((a, b) => b.created.localeCompare(a.created));
  return (
    <>
      <Heading title="Tasks" />
      <div className="toolbar" aria-label="Task filters">
        {[
          ["all", "All tasks"],
          ["attention", "Needs attention"],
          ["complete", "Completed"],
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
      {loaded.isPending || transfers.isPending || handoffs.isPending ? (
        <Loading />
      ) : loaded.error || transfers.error || handoffs.error ? (
        <ErrorMessage
          error={loaded.error || transfers.error || handoffs.error}
        />
      ) : records.length ? (
        <section className="panel">
          <WorkTable kind="Task">
            {records.map((record) =>
              record.kind === "publication" ? (
                <TaskRow key={record.id} id={record.id} filter={filter} />
              ) : record.kind === "handoff" ? (
                <HandoffRow
                  key={record.id}
                  task={handoffRows.find((t) => t.id === record.id)!}
                />
              ) : (
                <TransferTaskRow
                  key={record.id}
                  t={standalone.find((t) => t.id === record.id)!}
                />
              ),
            )}
          </WorkTable>
          {(transfers.hasNextPage || handoffs.hasNextPage) && (
            <button
              className="load-more"
              disabled={
                transfers.isFetchingNextPage || handoffs.isFetchingNextPage
              }
              onClick={() => {
                if (transfers.hasNextPage) void transfers.fetchNextPage();
                if (handoffs.hasNextPage) void handoffs.fetchNextPage();
              }}
            >
              Load more tasks
            </button>
          )}
        </section>
      ) : (
        <div className="empty">
          <h2>No tasks yet</h2>
        </div>
      )}
    </>
  );
}
function HandoffRow({ task }: { task: AgentHandoff }) {
  return (
    <tr>
      <td data-label="Task">
        <a href={`#/tasks/handoffs/${task.id}`} className="work-title-link">
          {task.title}
        </a>
      </td>
      <WorkMetadata
        agent={task.sender_name}
        destination={task.recipient_name}
        created={task.created_at}
        status={handoffStates[task.status]}
      />
    </tr>
  );
}
function HandoffDetail({ id }: { id: string }) {
  const task = useQuery<AgentHandoff>({
    queryKey: ["agent-handoff", id],
    queryFn: () => request<AgentHandoff>(`/api/agent-handoffs/${id}`),
  });
  if (task.isPending) return <Loading />;
  if (task.error) return <ErrorMessage error={task.error} />;
  const row = task.data;
  return (
    <>
      <Back to="tasks">Tasks</Back>
      <Heading title={row.title}>
        <Badge value={handoffStates[row.status]} />
      </Heading>
      <div className="narrow stack">
        <section className="panel pad">
          <dl>
            <dt>From</dt>
            <dd>{row.sender_name}</dd>
            <dt>To</dt>
            <dd>{row.recipient_name}</dd>
            <dt>Created</dt>
            <dd>
              <time dateTime={row.created_at}>
                {new Date(row.created_at).toLocaleString()}
              </time>
            </dd>
            <dt>Instructions</dt>
            <dd className="handoff-text">{row.instructions}</dd>
            {row.result && (
              <>
                <dt>Result</dt>
                <dd className="handoff-text">{row.result}</dd>
              </>
            )}
            {row.error && (
              <>
                <dt>Error</dt>
                <dd className="handoff-text">{row.error}</dd>
              </>
            )}
          </dl>
        </section>
        <a className="button" href={`#/activity?task=${row.id}`}>
          View activity
        </a>
      </div>
    </>
  );
}
function TransferTaskRow({ t }: { t: MediaTransfer }) {
  return (
    <tr>
      <td data-label="Task">
        <a href={`#/tasks/transfers/${t.id}`} className="work-title-link">
          {transferTitle(t)}
        </a>
        {t.status === "receiving" && (
          <progress
            aria-label={`Transfer progress for ${transferTitle(t)}`}
            value={t.offset}
            max={t.size}
          />
        )}
        {t.status === "checksum_mismatch" && (
          <p className="error">File checksum did not match</p>
        )}
        {t.status === "interrupted" && (
          <p className="error">{t.last_error || "Transfer interrupted"}</p>
        )}
      </td>
      <TransferMetadata t={t} />
    </tr>
  );
}
function TransferDetail({ id }: { id: string }) {
  const t = useQuery({
    queryKey: ["media-transfer", id],
    queryFn: () => request<MediaTransfer>(`/api/media-transfers/${id}`),
  });
  if (t.isPending) return <Loading />;
  if (t.error) return <ErrorMessage error={t.error} />;
  const row = t.data;
  return (
    <>
      <Back to="tasks">Tasks</Back>
      <Heading title={transferTitle(row)}>
        <Badge value={transferStates[row.status]} />
      </Heading>
      <div className="narrow">
        <section className="panel pad">
          <dl>
            <dt>Agent</dt>
            <dd>{row.agent_name || "Not recorded"}</dd>
            <dt>Transferred</dt>
            <dd>
              {row.offset.toLocaleString()} / {row.size.toLocaleString()} bytes
            </dd>
            <dt>Media ID</dt>
            <dd>
              <code>{row.id}</code>
            </dd>
          </dl>
          {row.status === "checksum_mismatch" && (
            <p className="error" role="alert">
              File checksum did not match. Ask the agent to check its source
              file and create a new transfer.
            </p>
          )}
          {row.status === "interrupted" && (
            <p className="error" role="alert">
              {row.last_error || "Transfer interrupted"}. The agent can check
              the saved offset and resume the same transfer.
            </p>
          )}
          <div className="actions">
            <a className="button" href={`#/activity?transfer=${id}`}>
              View activity
            </a>
          </div>
        </section>
      </div>
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
          {[kind, "Agent", "Destination", "Created", "Status"].map((label) => (
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
function WorkMetadata({
  agent,
  destination,
  created,
  status,
}: {
  agent: string | null | undefined;
  destination: string;
  created: string | null;
  status: string;
}) {
  return (
    <>
      <td data-label="Agent">{agent || "Not recorded"}</td>
      <td data-label="Destination">{destination}</td>
      <td data-label="Created">
        {created ? (
          <time dateTime={created}>{new Date(created).toLocaleString()}</time>
        ) : (
          "—"
        )}
      </td>
      <td data-label="Status">
        <Badge value={status} />
      </td>
    </>
  );
}
function PublicationMetadata({ p }: { p: Publication }) {
  return (
    <WorkMetadata
      agent={p.agent_name}
      destination="YouTube"
      created={p.created_at}
      status={states[p.status]}
    />
  );
}
const transferStates: Record<MediaTransfer["status"], string> = {
  receiving: "Receiving",
  ready: "Ready",
  checksum_mismatch: "Needs attention",
  interrupted: "Needs attention",
  cancelling: "Cancelling",
  cancelled: "Cancelled",
  expiring: "Expiring",
  expired: "Expired",
};
function transferTitle(t: MediaTransfer) {
  const kind = t.mime.startsWith("video/")
    ? "Video"
    : t.mime.startsWith("image/")
      ? "Image"
      : "File";
  if (t.size < 1024)
    return `${kind} transfer · ${t.size.toLocaleString()} bytes`;
  const unit = t.size >= 1024 * 1024 ? "MiB" : "KiB";
  const divisor = unit === "MiB" ? 1024 * 1024 : 1024;
  const amount = new Intl.NumberFormat(undefined, {
    maximumFractionDigits: 1,
  }).format(t.size / divisor);
  return `${kind} transfer · ${amount} ${unit}`;
}
function TransferMetadata({ t }: { t: MediaTransfer }) {
  return (
    <WorkMetadata
      agent={t.agent_name}
      destination="—"
      created={t.created_at}
      status={transferStates[t.status]}
    />
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
      <PublicationMetadata p={p} />
    </tr>
  );
}
// Display labels for YouTube's stable category IDs; discovery remains the authority for assignability.
const youtubeCategoryNames: Record<string, string> = {
  "1": "Film & Animation",
  "2": "Autos & Vehicles",
  "10": "Music",
  "15": "Pets & Animals",
  "17": "Sports",
  "19": "Travel & Events",
  "20": "Gaming",
  "22": "People & Blogs",
  "23": "Comedy",
  "24": "Entertainment",
  "25": "News & Politics",
  "26": "Howto & Style",
  "27": "Education",
  "28": "Science & Technology",
};
function languageName(code: string | null | undefined) {
  if (!code) return null;
  try {
    return (
      new Intl.DisplayNames(undefined, { type: "language" }).of(code) ?? code
    );
  } catch {
    return code;
  }
}
interface Detail {
  publication: Publication;
  settings: {
    privacy: string;
    made_for_kids: boolean;
    contains_synthetic_media: boolean;
    description: string;
    notify_subscribers: boolean;
    settings?: {
      category_id?: string | null;
      tags?: string[] | null;
      default_language?: string | null;
      default_audio_language?: string | null;
      publish_at?: string | null;
      license?: string | null;
      embeddable?: boolean | null;
      public_stats_viewable?: boolean | null;
      paid_product_placement?: boolean | null;
      recording_date?: string | null;
      localizations?: Record<
        string,
        { title: string; description: string }
      > | null;
    };
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
            <h2>Submitted video settings</h2>
          </div>
          <div className="pad">
            <dl>
              <dt>Requested visibility</dt>
              <dd>{s.privacy}</dd>
              <dt>Made for kids</dt>
              <dd>{s.made_for_kids ? "Yes" : "No"}</dd>
              <dt>Altered or synthetic content</dt>
              <dd>{s.contains_synthetic_media ? "Yes" : "No"}</dd>
              <dt>Subscriber notifications</dt>
              <dd>{s.notify_subscribers ? "On" : "Off"}</dd>
              <dt>Category</dt>
              <dd>
                {youtubeCategoryNames[s.settings?.category_id ?? "24"] ??
                  s.settings?.category_id}
              </dd>
              {[
                ["Tags", s.settings?.tags?.join(", ")],
                [
                  "Title and description language",
                  languageName(s.settings?.default_language),
                ],
                [
                  "Audio language",
                  languageName(s.settings?.default_audio_language),
                ],
                [
                  "Scheduled publication",
                  s.settings?.publish_at
                    ? new Date(s.settings.publish_at).toLocaleString(
                        undefined,
                        { timeZoneName: "short" },
                      )
                    : null,
                ],
                [
                  "License",
                  s.settings?.license === "creativeCommon"
                    ? "Creative Commons"
                    : s.settings?.license === "youtube"
                      ? "Standard YouTube"
                      : null,
                ],
                [
                  "Embedding",
                  s.settings?.embeddable == null
                    ? null
                    : s.settings.embeddable
                      ? "Allowed"
                      : "Disabled",
                ],
                [
                  "Public statistics",
                  s.settings?.public_stats_viewable == null
                    ? null
                    : s.settings.public_stats_viewable
                      ? "Shown"
                      : "Hidden",
                ],
                [
                  "Paid promotion",
                  s.settings?.paid_product_placement == null
                    ? null
                    : s.settings.paid_product_placement
                      ? "Yes"
                      : "No",
                ],
                [
                  "Recording date",
                  s.settings?.recording_date
                    ? new Date(s.settings.recording_date).toLocaleDateString(
                        undefined,
                        { timeZone: "UTC" },
                      )
                    : null,
                ],
              ]
                .filter(([, value]) => value != null)
                .map(([label, value]) => (
                  <Fragment key={label}>
                    <dt>{label}</dt>
                    <dd>{value || "None"}</dd>
                  </Fragment>
                ))}
              {Object.entries(s.settings?.localizations ?? {}).map(
                ([language, translation]) => (
                  <Fragment key={language}>
                    <dt>Translation ({languageName(language)})</dt>
                    <dd className="preserve-lines">
                      {translation.title}
                      {"\n"}
                      {translation.description}
                    </dd>
                  </Fragment>
                ),
              )}
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
  media_id?: string | null;
  items: {
    sequence: number;
    event_at?: string | null;
    publication: Publication;
  }[];
  next: number | null;
}
interface TransferJournal {
  items: {
    sequence: number;
    event_at?: string;
    status: string;
    offset?: number;
    size?: number;
    error?: string | null;
  }[];
  next: number | null;
}
function TransferSteps({ id }: { id: string }) {
  const h = useInfiniteQuery({
    queryKey: ["media-transfer-history", id],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      request<TransferJournal>(
        `/api/media-transfers/${id}/history?after=${pageParam}`,
      ),
    getNextPageParam: (last) => last.next ?? undefined,
  });
  if (h.isPending) return <Loading />;
  if (h.error) return <ErrorMessage error={h.error} />;
  return (
    <div className="journal">
      <ol>
        {h.data.pages
          .flatMap((page) => page.items)
          .map((step) => (
            <li key={step.sequence}>
              {step.event_at && (
                <time dateTime={step.event_at}>
                  {new Date(step.event_at).toLocaleString()}
                </time>
              )}
              <strong>
                {transferStates[step.status as MediaTransfer["status"]] ??
                  (step.status === "reserved" ? "Reserved" : step.status)}
              </strong>
              {step.size != null && step.offset != null && (
                <span>
                  {step.offset.toLocaleString()} / {step.size.toLocaleString()}{" "}
                  bytes
                </span>
              )}
              {step.status === "checksum_mismatch" && (
                <p className="error">File checksum did not match</p>
              )}
              {step.error && <p className="error">{step.error}</p>}
            </li>
          ))}
      </ol>
      {h.hasNextPage && (
        <button
          disabled={h.isFetchingNextPage}
          onClick={() => void h.fetchNextPage()}
        >
          Load more steps
        </button>
      )}
    </div>
  );
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
      {h.data.pages[0].media_id && (
        <TransferSteps id={h.data.pages[0].media_id} />
      )}
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
function TransferActivityItem({ t }: { t: MediaTransfer }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <tr className={open ? "work-expanded" : ""}>
        <td data-label="Activity">
          <button
            className="work-expand"
            aria-expanded={open}
            aria-controls={`transfer-history-${t.id}`}
            onClick={() => setOpen(!open)}
          >
            <ChevronRight size={16} aria-hidden="true" />
            <span>
              <span className="work-title-link">{transferTitle(t)}</span>
              <span className="work-operation">Transfer media</span>
            </span>
          </button>
        </td>
        <TransferMetadata t={t} />
      </tr>
      {open && (
        <tr className="work-history" id={`transfer-history-${t.id}`}>
          <td colSpan={5}>
            <TransferSteps id={t.id} />
            <a href={`#/tasks/transfers/${t.id}`}>View task</a>
          </td>
        </tr>
      )}
    </>
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
        <PublicationMetadata p={p} />
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
interface HandoffHistoryPage {
  items: { sequence: number; task: AgentHandoff & { action: string } }[];
  next: number | null;
}
function HandoffSteps({ id }: { id: string }) {
  const history = useInfiniteQuery<HandoffHistoryPage, Error>({
    queryKey: ["agent-handoff-history", id],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      request<HandoffHistoryPage>(
        `/api/agent-handoffs/${id}/history?after=${pageParam}`,
      ),
    getNextPageParam: (last) => last.next ?? undefined,
  });
  if (history.isPending) return <Loading />;
  if (history.error) return <ErrorMessage error={history.error} />;
  return (
    <div className="journal">
      <ol>
        {history.data.pages
          .flatMap((page) => page.items)
          .map(({ sequence, task }) => (
            <li key={sequence}>
              <time dateTime={task.updated_at}>
                {new Date(task.updated_at).toLocaleString()}
              </time>
              <strong>
                {(
                  {
                    created: "Queued",
                    claimed: "Claimed",
                    renewed: "Claim renewed",
                    completed: "Completed",
                    failed: "Failed",
                    cancelled: "Cancelled",
                  } as Record<string, string>
                )[task.action] ?? handoffStates[task.status]}
              </strong>
              {task.action === "completed" && task.result && (
                <span>{task.result}</span>
              )}
              {task.action === "failed" && task.error && (
                <p className="error">{task.error}</p>
              )}
            </li>
          ))}
      </ol>
      {history.hasNextPage && (
        <button
          disabled={history.isFetchingNextPage}
          onClick={() => void history.fetchNextPage()}
        >
          Load more steps
        </button>
      )}
      <a href={`#/tasks/handoffs/${id}`}>View task</a>
    </div>
  );
}
function HandoffActivityItem({ task }: { task: AgentHandoff }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <tr className={open ? "work-expanded" : ""}>
        <td data-label="Activity">
          <button
            className="work-expand"
            aria-expanded={open}
            aria-controls={`handoff-history-${task.id}`}
            onClick={() => setOpen(!open)}
          >
            <ChevronRight size={16} aria-hidden="true" />
            <span>
              <span className="work-title-link">{task.title}</span>
              <span className="work-operation">Agent handoff</span>
            </span>
          </button>
        </td>
        <WorkMetadata
          agent={task.sender_name}
          destination={task.recipient_name}
          created={task.created_at}
          status={handoffStates[task.status]}
        />
      </tr>
      {open && (
        <tr className="work-history" id={`handoff-history-${task.id}`}>
          <td colSpan={5}>
            <HandoffSteps id={task.id} />
          </td>
        </tr>
      )}
    </>
  );
}
function Activity({
  task,
  transfer,
  errorsOnly,
}: {
  task: string | null;
  transfer: string | null;
  errorsOnly: boolean;
}) {
  const loaded = usePublications();
  const transfers = useTransfers();
  const handoffs = useHandoffs();
  const standalone =
    transfers.data?.filter(
      (t) =>
        !t.has_publication &&
        (!transfer || t.id === transfer) &&
        (!errorsOnly ||
          ["checksum_mismatch", "interrupted"].includes(t.status)),
    ) ?? [];
  const rows = [
    ...(transfer
      ? []
      : loaded.ids
          .filter((id) => !task || task === id)
          .map((id) => ({
            kind: "publication" as const,
            id,
            created:
              cache.getQueryData<Publication>(["publication", id])
                ?.created_at ?? "",
          }))),
    ...(task
      ? []
      : standalone.map((t) => ({
          kind: "transfer" as const,
          id: t.id,
          created: t.created_at ?? "",
        }))),
    ...(transfer
      ? []
      : handoffs.rows
          .filter(
            (row) =>
              (!task || row.id === task) &&
              (!errorsOnly || row.status === "failed"),
          )
          .map((row) => ({
            kind: "handoff" as const,
            id: row.id,
            created: row.created_at,
          }))),
  ].sort((a, b) => b.created.localeCompare(a.created));
  const suffix = task
    ? `&task=${task}`
    : transfer
      ? `&transfer=${transfer}`
      : "";
  return (
    <>
      <Heading title="Activity log" />
      <div className="toolbar">
        <a
          className={!errorsOnly ? "selected" : ""}
          href={`#/activity${suffix ? `?${suffix.slice(1)}` : ""}`}
        >
          All activity
        </a>
        <a
          className={errorsOnly ? "selected" : ""}
          href={`#/activity?errors=1${suffix}`}
        >
          Needs attention
        </a>
        {(task || transfer) && <a href="#/activity">Clear task filter</a>}
      </div>
      {loaded.isPending || transfers.isPending || handoffs.isPending ? (
        <Loading />
      ) : loaded.error || transfers.error || handoffs.error ? (
        <ErrorMessage
          error={loaded.error || transfers.error || handoffs.error}
        />
      ) : (
        <section className="panel">
          <WorkTable kind="Activity">
            {rows.map((row) =>
              row.kind === "publication" ? (
                <ActivityItem
                  key={row.id}
                  id={row.id}
                  errorsOnly={errorsOnly}
                />
              ) : row.kind === "handoff" ? (
                <HandoffActivityItem
                  key={row.id}
                  task={handoffs.rows.find((task) => task.id === row.id)!}
                />
              ) : (
                <TransferActivityItem
                  key={row.id}
                  t={standalone.find((t) => t.id === row.id)!}
                />
              ),
            )}
          </WorkTable>
          {(transfers.hasNextPage || handoffs.hasNextPage) && (
            <button
              className="load-more"
              disabled={
                transfers.isFetchingNextPage || handoffs.isFetchingNextPage
              }
              onClick={() => {
                if (transfers.hasNextPage) void transfers.fetchNextPage();
                if (handoffs.hasNextPage) void handoffs.fetchNextPage();
              }}
            >
              Load more activity
            </button>
          )}
          {!rows.length && (
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
  if (path === "agents/access") return <AgentTaskAccess />;
  if (path === "agents/connect") return <ConnectAgent />;
  if (path.startsWith("agents/"))
    return <AgentDetail key={path} id={path.split("/")[1]} />;
  if (path === "platforms") return <Platforms />;
  if (path === "platforms/youtube" || path === "platforms/connect")
    return <Youtube />;
  if (path === "tasks") return <Tasks filter={query.get("state") || "all"} />;
  if (path.startsWith("tasks/handoffs/"))
    return <HandoffDetail key={path} id={path.split("/")[2]} />;
  if (path.startsWith("tasks/transfers/"))
    return <TransferDetail key={path} id={path.split("/")[2]} />;
  if (path.startsWith("tasks/"))
    return <TaskDetail key={path} id={path.split("/")[1]} />;
  if (path === "activity")
    return (
      <Activity
        task={query.get("task")}
        transfer={query.get("transfer")}
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
