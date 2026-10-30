import { invoke, isTauri } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { FlStudioProject, ProjectShelfProvider } from "./types";

const inDesktop = isTauri();

function requireDesktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!inDesktop) return Promise.reject(new Error("Project Shelf nécessite ORBIT OS sous Windows."));
  return invoke<T>(command, args);
}

/** Native-only file picker. It returns selected paths; the native service validates each path again. */
export async function selectFlStudioProjects(): Promise<string[]> {
  if (!inDesktop) throw new Error("La sélection de fichiers .flp est disponible dans l’application ORBIT native.");
  const result = await open({
    title: "Sélectionner des projets FL Studio",
    directory: false,
    multiple: true,
    filters: [{ name: "Projets FL Studio", extensions: ["flp"] }],
  });
  if (!result) return [];
  return (Array.isArray(result) ? result : [result]).filter(path => path.toLocaleLowerCase().endsWith(".flp"));
}

/**
 * Local metadata provider only. It never reads FLP/audio contents and has no network path.
 * Native commands re-check plugin installation and its permissions on every call.
 */
export const flStudioProjectProvider: ProjectShelfProvider = {
  available: inDesktop,
  list: () => requireDesktop<FlStudioProject[]>("fl_studio_list_projects"),
  add: path => requireDesktop<FlStudioProject[]>("fl_studio_add_project", { path }),
  setPinned: (id, pinned) => requireDesktop<FlStudioProject[]>("fl_studio_set_pinned", { id, pinned }),
  open: id => requireDesktop<void>("fl_studio_open_project", { id }),
  remove: id => requireDesktop<FlStudioProject[]>("fl_studio_remove_project", { id }),
  addTag: (id, tag) => requireDesktop<FlStudioProject[]>("fl_studio_add_tag", { id, tag }),
  removeTag: (id, tag) => requireDesktop<FlStudioProject[]>("fl_studio_remove_tag", { id, tag }),
};

export type { FlStudioProject, ProjectShelfProvider };
