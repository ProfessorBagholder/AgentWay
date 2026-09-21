import { useEffect, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider } from "@tanstack/react-query";
import { Cable, Radio, Workflow } from "lucide-react";
import { Snapshot, cache, request } from "./api";
import {
  AgentActivity,
  PublishingTasks,
  upsertActivity,
  renameConnection,
  type BridgeActivity,
} from "./activity";
import "./style.css";
import { Publishing, upsertPublication, type Publication } from "./publishing";

type Page = "Agents" | "Tasks" | "Publishing";
function pageFromHash(): Page {
  return window.location.hash === "#tasks"
    ? "Tasks"
    : window.location.hash === "#publishing"
      ? "Publishing"
      : "Agents";
}
function App() {
  const [page, setPage] = useState<Page>(pageFromHash);
  function navigate(next: Page) {
    window.location.hash = next.toLowerCase();
    setPage(next);
  }
  useEffect(() => {
    const changed = () => {
      if (window.location.hash !== "#main") setPage(pageFromHash());
    };
    window.addEventListener("hashchange", changed);
    return () => window.removeEventListener("hashchange", changed);
  }, []);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("connecting");
  useEffect(() => {
    let active = true;
    let stream: EventSource | undefined;
    request<Snapshot>("/api/bootstrap")
      .then((snapshot) => {
        if (!active) return;
        setReady(true);
        stream = new EventSource(`/api/events?after=${snapshot.cursor}`);
        stream.onopen = () => setConnection("connected");
        stream.addEventListener("publication.upsert", (event) =>
          upsertPublication(
            JSON.parse((event as MessageEvent).data) as Publication,
          ),
        );
        stream.addEventListener("youtube.status", (event) => {
          const status = JSON.parse((event as MessageEvent).data);
          void cache.cancelQueries({ queryKey: ["youtube"] }).then(() => {
            cache.setQueryData(["youtube"], status);
          });
        });
        stream.onerror = () => setConnection("reconnecting");
        stream.addEventListener("bridge.name", (event) =>
          renameConnection(JSON.parse((event as MessageEvent).data).name),
        );
        stream.addEventListener("bridge.activity", (event) =>
          upsertActivity(
            JSON.parse((event as MessageEvent).data) as BridgeActivity,
          ),
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
            navigate("Agents");
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
              onClick={() => navigate(name)}
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
              <AgentActivity openPublishing={() => navigate("Publishing")} />
            )}
            {page === "Tasks" && <PublishingTasks />}
            {page === "Publishing" && <Publishing />}
          </>
        )}
      </main>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <QueryClientProvider client={cache}>
    <App />
  </QueryClientProvider>,
);
