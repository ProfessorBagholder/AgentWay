import { useQuery } from "@tanstack/react-query";
import { cache, request } from "./api";
import { UploadRow, upsertPublication, type Publication } from "./publishing";

export interface BridgeActivity {
  last_seen: string;
  operation: string;
  revision: number;
}
interface BridgeConnection {
  name: string;
  activity: BridgeActivity | null;
}
export function renameConnection(name: string) {
  cache.setQueryData<BridgeConnection>(["bridge-connection"], (old) =>
    old ? { ...old, name } : old,
  );
}
export function upsertActivity(activity: BridgeActivity) {
  cache.setQueryData<BridgeConnection>(["bridge-connection"], (old) =>
    old?.activity && old.activity.revision >= activity.revision
      ? old
      : { name: old?.name ?? "Publishing connection", activity },
  );
}
export function AgentActivity({
  openPublishing,
}: {
  openPublishing: () => void;
}) {
  const loaded = useQuery({
    queryKey: ["bridge-connection-loaded"],
    queryFn: ({ signal }) =>
      fetch("/api/publishing/connection", { signal }).then(async (r) => {
        if (!r.ok) throw new Error("Could not load connection activity");
        const result = (await r.json()) as BridgeConnection;
        const current = cache.getQueryData<BridgeConnection>([
          "bridge-connection",
        ]);
        if (
          current?.activity &&
          current.activity.revision > (result.activity?.revision ?? 0)
        ) {
          result.activity = current.activity;
        }
        cache.setQueryData(["bridge-connection"], result);
        return true;
      }),
  });
  const activity = useQuery<BridgeConnection>({
    queryKey: ["bridge-connection"],
    enabled: false,
  });
  return (
    <section
      className="panel publishing-section"
      aria-label="Agent connections"
    >
      <h2>{activity.data?.name ?? "Agent connection"}</h2>
      <p>
        Connectors set up in your agent use this connection to upload to YouTube
        and check results.
      </p>
      {loaded.isPending ? (
        <p role="status">Loading activity…</p>
      ) : loaded.error ? (
        <p className="error" role="alert">
          {loaded.error.message}
        </p>
      ) : activity.data?.activity ? (
        <div className="row">
          <div className="row-main">
            <strong>{activity.data.activity.operation}</strong>
            <small>
              Last authenticated request:{" "}
              <time dateTime={activity.data.activity.last_seen}>
                {new Date(activity.data.activity.last_seen).toLocaleString()}
              </time>
            </small>
          </div>
        </div>
      ) : (
        <p>No requests recorded since activity tracking was enabled.</p>
      )}
      <p className="field-help">
        This connection uses a shared token. AgentWay cannot distinguish agents
        using that token or see work they do outside AgentWay.
      </p>
      <button className="secondary" onClick={openPublishing}>
        Connection settings
      </button>
    </section>
  );
}
export function PublishingTasks() {
  const uploads = useQuery({
    queryKey: ["publications-loaded"],
    queryFn: async () => {
      const rows = await request<Publication[]>("/api/publications");
      rows.slice().reverse().forEach(upsertPublication);
      return true;
    },
  });
  const { data: ids = [] } = useQuery<string[]>({
    queryKey: ["publications"],
    enabled: false,
  });
  return (
    <section className="panel publishing-section" aria-label="Publishing tasks">
      {uploads.isPending ? (
        <p role="status">Loading tasks…</p>
      ) : uploads.error ? (
        <p className="error" role="alert">
          {uploads.error.message}
        </p>
      ) : ids.length ? (
        <div className="rows">
          {ids.map((id) => (
            <UploadRow key={id} id={id} />
          ))}
        </div>
      ) : (
        <div className="empty">
          <h2>No publishing tasks</h2>
          <p>Uploads requested through AgentWay will appear here.</p>
        </div>
      )}
    </section>
  );
}
