import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import {
  QueryClientProvider,
  useMutation,
  useQuery,
} from "@tanstack/react-query";
import {
  ArrowUpRight,
  ArrowRight,
  AudioLines,
  Boxes,
  Cable,
  Check,
  ChevronRight,
  CircleHelp,
  Command,
  Layers3,
  Plus,
  Radio,
  Workflow,
  X,
} from "lucide-react";
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

type Page = "Overview" | "Agents" | "Tasks" | "Publishing";
function App() {
  const [page, setPage] = useState<Page>("Overview");
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("Connecting");
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
        stream.onopen = () => setConnection("Live");
        stream.onerror = () => setConnection("Reconnecting");
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
    ["Overview", <Boxes size={18} />],
    ["Agents", <Cable size={18} />],
    ["Tasks", <Workflow size={18} />],
    ["Publishing", <Radio size={18} />],
  ];
  return (
    <div className="shell">
      <aside>
        <a
          className="brand"
          href="#"
          onClick={(e) => {
            e.preventDefault();
            setPage("Overview");
          }}
        >
          <span className="brand-icon">
            <Layers3 size={22} />
          </span>
          AgentWay<span className="alpha">α</span>
        </a>
        <div className="workspace">
          <span className="workspace-icon">P</span>
          <div>
            Personal workspace<small>Local instance</small>
          </div>
          <ChevronRight size={16} />
        </div>
        <div className="nav-label">WORKSPACE</div>
        <nav>
          {nav.map(([name, icon]) => (
            <button
              key={name}
              className={page === name ? "selected" : ""}
              onClick={() => setPage(name)}
            >
              {icon}
              {name}
              {page === name && <span className="nav-dot" />}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="local-note">
            <span className="dot" />
            Your infrastructure.
            <br />
            <span>Your agents. Your control.</span>
          </div>
          <div className="version">
            <Command size={14} /> AgentWay <span>v0.1.0</span>
          </div>
        </div>
      </aside>
      <div className="body">
        <header>
          <span>
            Workspace <ChevronRight size={13} /> <strong>{page}</strong>
          </span>
          <span className="instance">
            <span className={`dot ${connection === "Live" ? "" : "muted"}`} />
            {connection}
            <span className="separator" />
            LOCAL
          </span>
        </header>
        <main>
          <div className="page-heading">
            <div>
              <div className="eyebrow">YOUR AGENT CONTROL ROOM</div>
              <h1>{page === "Overview" ? "Everything, connected." : page}</h1>
              <p>
                {page === "Overview"
                  ? "A shared way for your agents to work. Built around the access you already have."
                  : page === "Agents"
                    ? "Register the actual agents you use. Keep their identity and capabilities."
                    : page === "Tasks"
                      ? "A durable record of assignments, ready for connected agents."
                      : "Full episodes, short clips, and the channels that connect them."}
              </p>
            </div>
            <button
              className="primary"
              onClick={() => setDialog(page === "Tasks" ? "task" : "agent")}
              disabled={!ready}
            >
              <Plus size={16} />
              {page === "Tasks" ? "Create task" : "Register agent"}
            </button>
          </div>
          <div className="notice">
            <span className="notice-icon">
              <Command size={16} />
            </span>
            <div>
              <strong>Foundation preview</strong>
              <span>
                {" "}
                Registrations and tasks are saved locally. Agent execution,
                quota reporting and publishing adapters are not connected yet.
              </span>
            </div>
          </div>
          {error ? (
            <div role="alert" className="error">
              {error}
            </div>
          ) : !ready ? (
            <div className="loading" role="status">
              Opening your workspace…
            </div>
          ) : (
            <>
              {page === "Overview" && (
                <>
                  <Stats />
                  <section className="grid">
                    <div className="panel">
                      <PanelHeading
                        title="Your agents"
                        note="Bring your own intelligence"
                        action={() => setPage("Agents")}
                      />
                      <AgentList onAdd={() => setDialog("agent")} />
                    </div>
                    <div className="panel">
                      <PanelHeading
                        title="Activity"
                        note="Assignments across your workspace"
                        action={() => setPage("Tasks")}
                      />
                      <TaskList compact onAdd={() => setDialog("task")} />
                    </div>
                  </section>
                  <HowItWorks />
                </>
              )}
              {page === "Agents" && (
                <div className="panel">
                  <PanelHeading
                    title="Agent registry"
                    note="Registration is separate from a verified connection"
                  />
                  <AgentList onAdd={() => setDialog("agent")} />
                </div>
              )}
              {page === "Tasks" && (
                <div className="panel">
                  <PanelHeading
                    title="Task queue"
                    note="Queued tasks remain here until execution adapters are implemented"
                  />
                  <TaskList onAdd={() => setDialog("task")} />
                </div>
              )}
              {page === "Publishing" && <Publishing />}
            </>
          )}
          <footer>
            <span>
              <span className="dot muted" /> Stored on this machine
            </span>
            <span>No model hosting. No hidden inference.</span>
          </footer>
        </main>
      </div>
      {dialog && (
        <Modal
          title={
            dialog === "agent" ? "Register an agent" : "Create an assignment"
          }
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
function Stats() {
  const agents = useIds("agents");
  const tasks = useIds("tasks");
  return (
    <section className="stats">
      {[
        [
          "Registered agents",
          String(agents.length),
          "Connections pending",
          <Cable size={18} />,
        ],
        [
          "Saved assignments",
          String(tasks.length),
          "Persisted across restarts",
          <Workflow size={18} />,
        ],
        [
          "Account capacity",
          "Unknown",
          "Provider reporting not connected",
          <AudioLines size={18} />,
        ],
        [
          "Publishing destinations",
          "0",
          "Adapters pending",
          <Radio size={18} />,
        ],
      ].map(([label, value, note, icon]) => (
        <div className="stat" key={String(label)}>
          <div>
            {label}
            {icon}
          </div>
          <strong>{value}</strong>
          <small>{note}</small>
        </div>
      ))}
    </section>
  );
}
function PanelHeading({
  title,
  note,
  action,
}: {
  title: string;
  note: string;
  action?: () => void;
}) {
  return (
    <div className="panel-heading">
      <div>
        <h2>{title}</h2>
        <p>{note}</p>
      </div>
      {action && (
        <button
          className="icon-button"
          aria-label={`View ${title}`}
          onClick={action}
        >
          <ArrowUpRight size={18} />
        </button>
      )}
    </div>
  );
}
function AgentList({ onAdd }: { onAdd: () => void }) {
  const ids = useIds("agents");
  return ids.length ? (
    <div className="rows">
      {ids.map((id) => (
        <AgentRow key={id} id={id} />
      ))}
    </div>
  ) : (
    <div className="empty">
      <div className="empty-icon">
        <Cable size={25} />
      </div>
      <h3>Your team starts here</h3>
      <p>
        Register a manager or worker from
        <br />
        Grok Bot, Muse, ChatGPT or Claude.
      </p>
      <button className="secondary" onClick={onAdd}>
        <Plus size={14} />
        Register your first agent
      </button>
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
      <span className="badge">Unconfigured</span>
    </div>
  );
}
function TaskList({
  onAdd,
  compact = false,
}: {
  onAdd: () => void;
  compact?: boolean;
}) {
  const ids = useIds("tasks");
  return ids.length ? (
    <div className="rows">
      {(compact ? ids.slice(0, 4) : ids).map((id) => (
        <TaskRow id={id} key={id} />
      ))}
    </div>
  ) : (
    <div className="empty">
      <div className="empty-icon">
        <Workflow size={25} />
      </div>
      <h3>A clear path from idea to done</h3>
      <p>
        Assignments and results will stay together.
        <br />
        Your agents do the thinking.
      </p>
      <button className="text-button" onClick={onAdd}>
        Create an assignment <ArrowRight size={14} />
      </button>
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
          {t.status === "queued"
            ? "Waiting for an execution adapter"
            : "Cancelled"}{" "}
          · {new Date(t.created_at).toLocaleDateString()}
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
function HowItWorks() {
  return (
    <section className="flow">
      <div>
        <span className="eyebrow">THE WAY IT WORKS</span>
        <h2>One bridge. Your whole team.</h2>
        <p>Your existing agents stay in charge.</p>
      </div>
      <div className="flow-step">
        <span>01</span>
        <strong>Connect</strong>
        <small>Your agents & accounts</small>
      </div>
      <ArrowRight size={18} />
      <div className="flow-step">
        <span>02</span>
        <strong>Delegate</strong>
        <small>Instructions & artifacts</small>
      </div>
      <ArrowRight size={18} />
      <div className="flow-step">
        <span>03</span>
        <strong>Publish</strong>
        <small>Each platform’s format</small>
      </div>
    </section>
  );
}
function Publishing() {
  return (
    <div className="destinations">
      {[
        ["YouTube", "Episodes & Shorts"],
        ["Instagram", "Reels & posts"],
        ["TikTok", "Short videos"],
        ["Spotify", "Episodes & Clips"],
        ["Facebook", "Reels & Page posts"],
        ["Podcast RSS", "Audio distribution"],
      ].map(([name, formats]) => (
        <article className="panel destination" key={name}>
          <Radio size={23} />
          <h2>{name}</h2>
          <p>{formats}</p>
          <span className="badge">Not implemented</span>
        </article>
      ))}
    </div>
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
        Save your agent’s identity. Platform authorization will be added with
        the connection adapters.
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
        {mutation.isPending ? "Saving…" : "Register agent"}
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
        <CircleHelp size={22} />
        <p>Register an agent before creating an assignment.</p>
      </div>
    );
  return (
    <form onSubmit={submit}>
      <p className="form-note">
        This saves an assignment locally. It will not run or consume AI usage
        yet.
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
        {mutation.isPending ? "Saving…" : "Save assignment"}
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
