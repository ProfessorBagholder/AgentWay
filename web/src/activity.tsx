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
  state: string;
  publish_enabled: boolean;
  activity: BridgeActivity | null;
}
export function renameConnection(name: string) {
  cache.setQueryData<BridgeConnection>(["bridge-connection"], (old) =>
    old ? { ...old, name } : old,
  );
}
export function upsertActivity(activity: BridgeActivity) {
  cache.setQueryData<BridgeConnection>(["bridge-connection"], (old) =>
    !old || (old.activity && old.activity.revision >= activity.revision)
      ? old
      : {
          ...old,
          activity,
          state: old.state === "Disconnected" ? old.state : "Connected",
        },
  );
}
export function useConnection() {
  return useQuery({
    queryKey: ["bridge-connection"],
    queryFn: () => request<BridgeConnection>("/api/publishing/connection"),
  });
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
