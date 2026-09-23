import { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { QueryClientProvider, type InfiniteData } from "@tanstack/react-query";
import { Cable, Radio, Workflow, ScrollText, Settings } from "lucide-react";
import { Snapshot, cache, request } from "./api";
import { upsertConnection, type BridgeConnection } from "./activity";
import { upsertPublication, type Publication } from "./publishing";
import { Workspace, type Journal } from "./workspace";
import "./style.css";
function routeFromHash() {
  let route = window.location.hash.replace(/^#\/?/, "");
  if (route === "publishing") route = "platforms/youtube";
  route = route
    .replace(/^destinations/, "platforms")
    .replace(/^events/, "activity");
  return route && route !== "main" ? route : "agents";
}
function App() {
  const [route, setRoute] = useState(routeFromHash);
  const [ready, setReady] = useState(false);
  const [error, setError] = useState("");
  const [connection, setConnection] = useState("connecting");
  useEffect(() => {
    const changed = () => {
      if (window.location.hash !== "#main") setRoute(routeFromHash());
    };
    window.addEventListener("hashchange", changed);
    return () => window.removeEventListener("hashchange", changed);
  }, []);
  useEffect(() => {
    let active = true;
    let stream: EventSource | undefined;
    request<Snapshot>("/api/bootstrap")
      .then((snapshot) => {
        if (!active) return;
        stream = new EventSource(`/api/events?after=${snapshot.cursor}`);
        stream.onopen = () => setConnection("connected");
        stream.onerror = () => setConnection("reconnecting");
        stream.addEventListener("publication.upsert", (event) => {
          const p = JSON.parse((event as MessageEvent).data) as Publication;
          upsertPublication(p);
          const sequence = Number((event as MessageEvent).lastEventId);
          cache.setQueryData<InfiniteData<Journal>>(
            ["history", p.id],
            (old) => {
              if (!old || !sequence) return old;
              const last = old.pages[old.pages.length - 1];
              if (
                last.next !== null ||
                last.items.some((x) => x.sequence >= sequence)
              )
                return old;
              return {
                ...old,
                pages: [
                  ...old.pages.slice(0, -1),
                  {
                    ...last,
                    items: [
                      ...last.items,
                      {
                        sequence,
                        publication: p,
                        event_at: (p as Publication & { event_at?: string })
                          .event_at,
                      },
                    ],
                  },
                ],
              };
            },
          );
        });
        stream.addEventListener("youtube.status", (event) => {
          const status = JSON.parse((event as MessageEvent).data);
          void cache
            .cancelQueries({ queryKey: ["youtube"] })
            .then(() => cache.setQueryData(["youtube"], status));
        });
        stream.addEventListener("agent.connection", (event) => {
          upsertConnection(
            JSON.parse((event as MessageEvent).data) as BridgeConnection,
          );
        });
        setReady(true);
      })
      .catch((e) => {
        if (active) setError(e.message);
      });
    return () => {
      active = false;
      stream?.close();
    };
  }, []);
  const nav = [
    ["agents", "Agents", Cable],
    ["platforms", "Platforms", Radio],
    ["tasks", "Tasks", Workflow],
    ["activity", "Activity log", ScrollText],
    ["settings", "Settings", Settings],
  ] as const;
  return (
    <div className="shell">
      <a className="skip-link" href="#main">
        Skip to content
      </a>
      <aside>
        <a className="brand" href="#/agents">
          AgentWay
        </a>
        <nav aria-label="Main navigation">
          {nav.map(([path, label, Icon]) => (
            <a
              key={path}
              href={`#/${path}`}
              aria-current={
                route.split(/[/?]/)[0] === path ? "page" : undefined
              }
            >
              <Icon size={18} aria-hidden="true" />
              {label}
            </a>
          ))}
        </nav>
      </aside>
      <main id="main" tabIndex={-1}>
        {connection === "reconnecting" && (
          <p className="error" role="status">
            Connection lost. Reconnecting…
          </p>
        )}
        {error ? (
          <p role="alert" className="error">
            {error}
          </p>
        ) : !ready ? (
          <p role="status">Loading…</p>
        ) : (
          <Workspace route={route} />
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
