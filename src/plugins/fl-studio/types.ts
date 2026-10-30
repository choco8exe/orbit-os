import type { ProjectShelfEntry } from "../../shared/contracts";

export type FlStudioProject = ProjectShelfEntry;

/** The page owns the confirmed file-picker flow; provider validates at the native boundary too. */

export interface ProjectShelfProvider {
  readonly available: boolean;
  list(): Promise<FlStudioProject[]>;
  add(path: string): Promise<FlStudioProject[]>;
  setPinned(id: string, pinned: boolean): Promise<FlStudioProject[]>;
  open(id: string): Promise<void>;
  remove(id: string): Promise<FlStudioProject[]>;
  addTag(id: string, tag: string): Promise<FlStudioProject[]>;
  removeTag(id: string, tag: string): Promise<FlStudioProject[]>;
}
