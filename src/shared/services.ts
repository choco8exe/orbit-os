import { invoke } from '@tauri-apps/api/core';
import type { OrbitServices, ProjectShelfEntry } from './contracts';
import { coreService } from '../modules/core/service';
import { createEventBus, timelineService } from './events';
import { settingsService } from './settings';
import { echoService } from '../modules/echo/module';
import { pulsarService } from '../modules/pulsar/service';
import { pluginService } from './plugins';

const events = createEventBus();
export const orbitServices: OrbitServices = {
  core: coreService,
  events,
  settings: settingsService,
  timeline: timelineService,
  plugins: pluginService,
  projectShelf: {
    list: () => invoke<ProjectShelfEntry[]>('fl_studio_list_projects'),
    add: (path) => invoke<ProjectShelfEntry[]>('fl_studio_add_project', { path }),
    setPinned: (id, pinned) => invoke<ProjectShelfEntry[]>('fl_studio_set_pinned', { id, pinned }),
    open: (id) => invoke<void>('fl_studio_open_project', { id }),
    remove: (id) => invoke<ProjectShelfEntry[]>('fl_studio_remove_project', { id }),
    addTag: (id, tag) => invoke<ProjectShelfEntry[]>('fl_studio_add_tag', { id, tag }),
    removeTag: (id, tag) => invoke<ProjectShelfEntry[]>('fl_studio_remove_tag', { id, tag }),
  },
  echo: echoService,
  pulsar: pulsarService,
};
