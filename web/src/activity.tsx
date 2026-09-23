import {
  useInfiniteQuery,
  useQuery,
  type InfiniteData,
} from "@tanstack/react-query";
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
export interface MediaTransfer {
  cursor: number;
  id: string;
  agent_name: string | null;
  mime: string;
  size: number;
  offset: number;
  status:
    | "receiving"
    | "ready"
    | "interrupted"
    | "checksum_mismatch"
    | "cancelling"
    | "cancelled"
    | "expiring"
    | "expired";
  last_error: string | null;
  created_at: string | null;
  has_publication: boolean;
}
interface TransferPage {
  items: MediaTransfer[];
  next: number | null;
}
export function upsertTransfer(transfer: MediaTransfer) {
  cache.setQueryData(["media-transfer", transfer.id], transfer);
  cache.setQueryData<InfiniteData<TransferPage>>(["media-transfers"], (old) => {
    if (!old) return old;
    const found = old.pages.some((page) =>
      page.items.some((row) => row.id === transfer.id),
    );
    return {
      ...old,
      pages: old.pages.map((page, index) => ({
        ...page,
        items: found
          ? page.items.map((row) => (row.id === transfer.id ? transfer : row))
          : index === 0
            ? [transfer, ...page.items]
            : page.items,
      })),
    };
  });
}
export function useTransfers() {
  const loaded = useInfiniteQuery({
    queryKey: ["media-transfers"],
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      request<TransferPage>(
        `/api/media-transfers${pageParam ? `?before=${pageParam}` : ""}`,
      ),
    getNextPageParam: (last) => last.next ?? undefined,
  });
  return {
    ...loaded,
    data: loaded.data?.pages.flatMap((page) => page.items),
  };
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
