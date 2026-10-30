import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { EchoDuplicateSet, EchoEntryDetail, EchoPort, EchoPreferences, EchoSearchHit, EchoSnapshot } from "./types";

const changedEvent = "echo://changed";

/** Bridge for the native Windows service. It fails closed when called outside Tauri. */
export const createEchoPort = (): EchoPort => ({
  getSnapshot: () => invoke<EchoSnapshot>("echo_snapshot"),
  getEntry: id => invoke<EchoEntryDetail>("echo_get_entry", { id }),
  search: (query, options) => invoke<EchoSearchHit[]>("echo_search", { query, since: options?.since ?? null, limit: options?.limit ?? null }),
  subscribe(listener) {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<void>(changedEvent, () => { void invoke<EchoSnapshot>("echo_snapshot").then(listener).catch(() => undefined); }).then(stop => {
      if (disposed) stop(); else unlisten = stop;
    });
    return () => { disposed = true; unlisten?.(); };
  },
  setMonitoring: enabled => invoke("echo_set_monitoring", { enabled }),
  setPreferences: patch => invoke("echo_set_preferences", { patch }),
  copyEntry: id => invoke("echo_copy_entry", { id }),
  setFavorite: (id, favorite) => invoke("echo_set_favorite", { id, favorite }),
  setPinned: (id, pinned) => invoke("echo_set_pinned", { id, pinned }),
  assignCollection: (id, collectionId) => invoke("echo_assign_collection", { id, collectionId }),
  createCollection: name => invoke("echo_create_collection", { name }),
  deleteEntry: id => invoke("echo_delete_entry", { id }),
  deleteEntries: ids => invoke("echo_delete_entries", { ids }),
  findDuplicates: () => invoke<EchoDuplicateSet[]>("echo_find_duplicates"),
  removeDuplicates: () => invoke<number>("echo_remove_duplicates"),
  clearHistory: () => invoke("echo_clear_history"),
});

let activePort: EchoPort | undefined;

/** Core calls this once during startup before mounting the module registry. */
export function configureEchoPort(port: EchoPort = createEchoPort()) { activePort = port; }

export function getEchoPort(): EchoPort {
  if (!activePort) throw new Error("Le service Echo n’a pas été initialisé par ORBIT CORE.");
  return activePort;
}

export type { EchoPreferences };
