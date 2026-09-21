import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import {
  QueryClientProvider,
  useMutation,
  useQuery,
} from "@tanstack/react-query";
import { Cable, Check, Plus, Radio, Workflow, X } from "lucide-react";
import {
  Agent,
  Task,
  Snapshot,
  Platform,
  cache,
  labels,
  request,
  seed,
  upsertAgent,
  upsertTask,
} from "./api";
import "./style.css";

type Page = "Agents" | "Tasks" | "Publishing";
function App() {
  const [page, setPage] = useState<Page>("Agents");
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("connecting");
  const [dialog, setDialog] = useState<"agent" | "task" | null>(null);
  useEffect(() => {
    let active = true;
    let stream: EventSource | undefined;
    request<Snapshot>("/api/bootstrap")
      .then((snapshot) => {
        if (!active) return;
        seed(snapshot);
        setReady(true);
        stream = new EventSource(`/api/events?after=${snapshot.cursor}`);
        stream.onopen = () => setConnection("connected");
        stream.onerror = () => setConnection("reconnecting");
        stream.addEventListener("agent.upsert", (event) =>
          upsertAgent(JSON.parse((event as MessageEvent).data) as Agent),
        );
        stream.addEventListener("task.upsert", (event) =>
          upsertTask(JSON.parse((event as MessageEvent).data) as Task),
        );
      })
      .catch((e) => {
        if (active) setError(String(e.message));
      });
    return () => {
      active = false;
      stream?.close();
    };
  }, []);
  const nav: [Page, ReactNode][] = [
    ["Agents", <Cable size={18} />],
    ["Tasks", <Workflow size={18} />],
    ["Publishing", <Radio size={18} />],
  ];
  return (
    <div className="shell">
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <aside>
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setPage("Agents");
          }}
        >
          AgentWay
        </a>
        <nav aria-label="Main navigation">
          {nav.map(([name, icon]) => (
            <button
              key={name}
              aria-current={page === name ? "page" : undefined}
              className={page === name ? "selected" : ""}
              onClick={() => setPage(name)}
            >
              {icon}
              {name}
            </button>
          ))}
        </nav>
      </aside>
      <main id="main" tabIndex={-1}>
        <div className="page-heading">
          <h1>{page}</h1>
          {page !== "Publishing" && (
            <button
              className="primary"
              disabled={!ready}
              onClick={() => setDialog(page === "Tasks" ? "task" : "agent")}
            >
              <Plus size={16} />
              {page === "Tasks" ? "Save task" : "Add agent"}
            </button>
          )}
        </div>
        {connection === "reconnecting" && !error && (
          <div className="error" role="status">
            Connection lost. Reconnecting to AgentWay… Changes may not appear
            until the connection is restored.
          </div>
        )}
        {error ? (
          <div role="alert" className="error">
            Could not load AgentWay: {error}
          </div>
        ) : !ready ? (
          <p role="status">Loading…</p>
        ) : (
          <>
            {page === "Agents" && (
              <>
                <p className="page-note">
                  Saved agents are not connected yet. Connecting to agent
                  platforms is not available in this version.
                </p>
                <section className="panel" aria-label="Saved agents">
                  <AgentList />
                </section>
              </>
            )}
            {page === "Tasks" && (
              <>
                <p className="page-note">
                  Tasks can be saved and cancelled. They cannot be sent to
                  agents in this version.
                </p>
                <section className="panel" aria-label="Saved tasks">
                  <TaskList />
                </section>
              </>
            )}
            {page === "Publishing" && <Publishing />}
          </>
        )}
      </main>
      {dialog && (
        <Modal
          title={dialog === "agent" ? "Add agent" : "Save task"}
          close={() => setDialog(null)}
        >
          {dialog === "agent" ? (
            <AgentForm done={() => setDialog(null)} />
          ) : (
            <TaskForm done={() => setDialog(null)} />
          )}
        </Modal>
      )}
    </div>
  );
}

function useIds(key: string) {
  return (
    useQuery<string[]>({
      queryKey: [key],
      queryFn: async () => [],
      enabled: false,
    }).data ?? []
  );
}
function AgentList() {
  const ids = useIds("agents");
  return ids.length ? (
    <div className="rows">
      {ids.map((id) => (
        <AgentRow key={id} id={id} />
      ))}
    </div>
  ) : (
    <div className="empty">
      <h2>No saved agents</h2>
      <p>Add an agent to save its name, platform and role.</p>
    </div>
  );
}
function AgentRow({ id }: { id: string }) {
  const { data: a } = useQuery<Agent>({
    queryKey: ["agent", id],
    enabled: false,
  });
  if (!a) return null;
  return (
    <div className="row">
      <span className={`avatar ${a.platform}`}>{a.name[0].toUpperCase()}</span>
      <div className="row-main">
        <strong>{a.name}</strong>
        <small>
          {labels[a.platform]} · {a.role}
        </small>
      </div>
      <span className="badge">Not connected</span>
    </div>
  );
}
function TaskList() {
  const ids = useIds("tasks");
  return ids.length ? (
    <div className="rows">
      {ids.map((id) => (
        <TaskRow key={id} id={id} />
      ))}
    </div>
  ) : (
    <div className="empty">
      <h2>No saved tasks</h2>
      <p>Tasks you save will appear here.</p>
    </div>
  );
}
function TaskRow({ id }: { id: string }) {
  const { data: t } = useQuery<Task>({
    queryKey: ["task", id],
    enabled: false,
  });
  const cancel = useMutation({
    mutationFn: () => request<Task>(`/api/tasks/${id}/cancel`, {}),
    onSuccess: upsertTask,
  });
  if (!t) return null;
  return (
    <div className="row task-row">
      <span className="task-icon">
        {t.status === "cancelled" ? <X size={17} /> : <Workflow size={17} />}
      </span>
      <div className="row-main">
        <strong>{t.title}</strong>
        <small>
          {t.status === "queued" ? "Not sent" : "Cancelled"} ·{" "}
          {new Date(t.created_at).toLocaleDateString()}
        </small>
        {cancel.error && (
          <small role="alert" className="error">
            {cancel.error.message}
          </small>
        )}
      </div>
      <span className={`badge ${t.status}`}>{t.status}</span>
      {t.status === "queued" && (
        <button
          className="icon-button"
          disabled={cancel.isPending}
          aria-label={`Cancel ${t.title}`}
          onClick={() => cancel.mutate()}
        >
          <X size={15} />
        </button>
      )}
    </div>
  );
}
function Publishing() {
  return (
    <section className="panel empty">
      <h2>Publishing is not available yet</h2>
      <p>This version cannot connect social accounts or publish content.</p>
    </section>
  );
}
function Modal({
  title,
  close,
  children,
}: {
  title: string;
  close: () => void;
  children: ReactNode;
}) {
  useEffect(() => {
    const dialog = document.querySelector("dialog");
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return (
    <dialog onCancel={close} aria-labelledby="dialog-title">
      <div className="dialog-heading">
        <h2 id="dialog-title">{title}</h2>
        <button
          className="icon-button"
          onClick={close}
          aria-label="Close dialog"
        >
          <X size={19} />
        </button>
      </div>
      {children}
    </dialog>
  );
}
function AgentForm({ done }: { done: () => void }) {
  const mutation = useMutation({
    mutationFn: (body: unknown) => request<Agent>("/api/agents", body),
    onSuccess: (a) => {
      upsertAgent(a);
      done();
    },
  });
  function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    mutation.mutate(Object.fromEntries(new FormData(e.currentTarget)));
  }
  return (
    <form onSubmit={submit}>
      <p className="form-note">
        This saves the details below. It does not connect to your agent.
      </p>
      <label>
        Agent name
        <input
          name="name"
          required
          maxLength={80}
          placeholder="e.g. Podcast coordinator"
          autoFocus
        />
      </label>
      <label>
        Platform
        <select name="platform">
          {Object.entries(labels).map(([id, label]) => (
            <option value={id as Platform} key={id}>
              {label}
            </option>
          ))}
        </select>
      </label>
      <label>
        Role
        <select name="role">
          <option value="manager">Manager / coordinator</option>
          <option value="worker">Worker</option>
        </select>
      </label>
      {mutation.error && (
        <p className="error" role="alert">
          {mutation.error.message}
        </p>
      )}
      <button className="primary submit" disabled={mutation.isPending}>
        <Check size={16} />
        {mutation.isPending ? "Saving…" : "Add agent"}
      </button>
    </form>
  );
}
function TaskForm({ done }: { done: () => void }) {
  const ids = useIds("agents");
  const [requestId] = useState(() => crypto.randomUUID());
  const mutation = useMutation({
    mutationFn: (body: unknown) => request<Task>("/api/tasks", body),
    onSuccess: (t) => {
      upsertTask(t);
      done();
    },
  });
  function submit(e: FormEvent<HTMLFormElement>) {
    e.preventDefault();
    mutation.mutate({
      ...Object.fromEntries(new FormData(e.currentTarget)),
      request_id: requestId,
    });
  }
  if (!ids.length)
    return (
      <div className="form-note">
        <p>Add an agent before saving a task.</p>
      </div>
    );
  return (
    <form onSubmit={submit}>
      <p className="form-note">
        This saves a task. It will not be sent to the selected agent.
      </p>
      <label>
        Title
        <input
          name="title"
          required
          maxLength={160}
          placeholder="e.g. Prepare episode captions"
          autoFocus
        />
      </label>
      <label>
        Assign to
        <select name="agent_id">
          {ids.map((id) => (
            <AgentOption key={id} id={id} />
          ))}
        </select>
      </label>
      <label>
        Instructions
        <textarea
          name="instructions"
          rows={4}
          maxLength={8000}
          placeholder="Describe the result and any constraints."
        />
      </label>
      {mutation.error && (
        <p className="error" role="alert">
          {mutation.error.message}
        </p>
      )}
      <button className="primary submit" disabled={mutation.isPending}>
        <Plus size={16} />
        {mutation.isPending ? "Saving…" : "Save task"}
      </button>
    </form>
  );
}
function AgentOption({ id }: { id: string }) {
  const { data: a } = useQuery<Agent>({
    queryKey: ["agent", id],
    enabled: false,
  });
  return <option value={id}>{a?.name ?? id}</option>;
}
createRoot(document.getElementById("root")!).render(
  <QueryClientProvider client={cache}>
    <App />
  </QueryClientProvider>,
);
