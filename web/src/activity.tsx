import { useQuery } from "@tanstack/react-query";
import { cache, request } from "./api";
import {
  upsertPublication,
  type Publication,
  type YoutubeStatus,
} from "./publishing";
export interface BridgeActivity {
  last_seen: string;
  operation: string;
  revision: number;
}
export interface BridgeConnection {
  id: string;
  name: string;
  product: string;
  revision: number;
  state: string;
  publish_enabled: boolean;
  activity: BridgeActivity | null;
}
export function upsertConnection(connection: BridgeConnection) {
  cache.setQueryData<BridgeConnection[]>(["agent-connections"], (old) => {
    if (!old) return [connection];
    const existing = old.find((c) => c.id === connection.id);
    if (existing && existing.revision > connection.revision) return old;
    return existing
      ? old.map((c) => (c.id === connection.id ? connection : c))
      : [...old, connection];
  });
}
export function useConnections() {
  const loaded = useQuery({
    queryKey: ["agent-connections-loaded"],
    queryFn: async () => {
      const fetched = await request<BridgeConnection[]>(
        "/api/agent-connections",
      );
      fetched.forEach(upsertConnection);
      return true;
    },
  });
  const { data = [] } = useQuery<BridgeConnection[]>({
    queryKey: ["agent-connections"],
    enabled: false,
  });
  return { ...loaded, data };
}
export function useYoutube() {
  return useQuery({
    queryKey: ["youtube"],
    queryFn: () => request<YoutubeStatus>("/api/youtube"),
  });
}
export function usePublications(filter = "all") {
  const loaded = useQuery({
    queryKey: ["publications-loaded"],
    queryFn: async () => {
      const rows = await request<Publication[]>("/api/publications");
      rows.slice().reverse().forEach(upsertPublication);
      return true;
    },
  });
  const { data: ids = [] } = useQuery<string[]>({
    queryKey: filter === "all" ? ["publications"] : ["publications", filter],
    enabled: false,
  });
  return { ...loaded, ids };
}
export function usePublication(id: string) {
  return useQuery<Publication>({
    queryKey: ["publication", id],
    enabled: false,
  });
}
