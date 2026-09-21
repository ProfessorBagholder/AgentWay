import { QueryClient } from "@tanstack/react-query";
export type Platform = "grok_bot" | "muse" | "chatgpt" | "claude";
export interface Agent {
  id: string;
  name: string;
  platform: Platform;
  role: "manager" | "worker";
  status: "unconfigured";
}
export interface Task {
  id: string;
  title: string;
  instructions: string;
  agent_id: string;
  status: "queued" | "cancelled";
  created_at: string;
  revision: number;
}
export interface Snapshot {
  agents: Agent[];
  tasks: Task[];
  cursor: number;
}
export const labels: Record<Platform, string> = {
  grok_bot: "Grok Bot",
  muse: "Muse",
  chatgpt: "ChatGPT agents",
  claude: "Claude agents",
};
export const cache = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Infinity,
      retry: 1,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
      refetchOnMount: false,
    },
    mutations: { retry: false },
  },
});
export async function request<T>(path: string, body?: unknown): Promise<T> {
  const response = await fetch(
    path,
    body === undefined
      ? undefined
      : {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body),
        },
  );
  if (!response.ok) {
    const error = await response
      .json()
      .catch(() => ({ error: `Request failed (${response.status})` }));
    throw new Error(error.error ?? `Request failed (${response.status})`);
  }
  return response.json() as Promise<T>;
}
export function upsertAgent(agent: Agent) {
  cache.setQueryData<Agent>(["agent", agent.id], (old) =>
    old && JSON.stringify(old) === JSON.stringify(agent) ? old : agent,
  );
  cache.setQueryData<string[]>(["agents"], (old) =>
    old?.includes(agent.id) ? old : [...(old ?? []), agent.id],
  );
}
export function upsertTask(task: Task) {
  cache.setQueryData<Task>(["task", task.id], (old) =>
    old && old.revision >= task.revision ? old : task,
  );
  cache.setQueryData<string[]>(["tasks"], (old) =>
    old?.includes(task.id) ? old : [task.id, ...(old ?? [])],
  );
}
export function seed(snapshot: Snapshot) {
  for (const agent of snapshot.agents) upsertAgent(agent);
  for (const task of snapshot.tasks) upsertTask(task);
  cache.setQueryData(
    ["agents"],
    snapshot.agents.map((a) => a.id),
  );
  cache.setQueryData(
    ["tasks"],
    snapshot.tasks.map((t) => t.id),
  );
}
