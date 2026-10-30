import { invoke, isTauri } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';

export interface WorkspaceApplication { id: string; name: string; path: string }
export interface Workspace { id: string; name: string; createdAt: string; applications: WorkspaceApplication[] }
export interface WorkspaceLaunchResult { workspaceName: string; launchedCount: number }

function desktop<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error('Les espaces de travail sont disponibles dans ORBIT OS sous Windows.'));
  return invoke<T>(command, args);
}

export const workspacesService = {
  list: () => desktop<Workspace[]>('workspaces_list'),
  create: (name: string) => desktop<Workspace[]>('workspaces_create', { name }),
  rename: (id: string, name: string) => desktop<Workspace[]>('workspaces_rename', { id, name }),
  remove: (id: string) => desktop<Workspace[]>('workspaces_delete', { id }),
  addApp: (workspaceId: string, path: string) => desktop<Workspace[]>('workspaces_add_app', { workspaceId, path }),
  removeApp: (workspaceId: string, appId: string) => desktop<Workspace[]>('workspaces_remove_app', { workspaceId, appId }),
  launch: (workspaceId: string) => desktop<WorkspaceLaunchResult>('workspaces_launch', { workspaceId }),
  async chooseExecutable(): Promise<string | null> {
    if (!isTauri()) throw new Error('Le sélecteur d’applications est disponible dans ORBIT OS sous Windows.');
    const selected = await open({ directory: false, multiple: false, title: 'Choisir une application Windows', filters: [{ name: 'Application Windows', extensions: ['exe'] }] });
    return typeof selected === 'string' ? selected : null;
  },
};
