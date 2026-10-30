import type { PluginPageProvider } from '../shared/contracts';
import { Activity, FolderOpen, Gamepad2, Keyboard } from 'lucide-react';
import { ProjectShelfPage } from './ProjectShelfPage';
import { ActivityPulsePage } from '../plugins/activity/ActivityPulsePage';
import { StreamDeckPage } from '../plugins/creator/StreamDeckPage';
import { WorkspacesPage } from '../plugins/workspaces/WorkspacesPage';
import { GameSessionsPage } from '../plugins/game-sessions/GameSessionsPage';
import { DownloadsOrganizerPage } from '../plugins/downloads-organizer/DownloadsOrganizerPage';

/** Only statically compiled, first-party providers are registered here. */
export const pluginPageProviders: PluginPageProvider[] = [
  { pluginId: 'com.orbitos.fl-studio', routeId: 'project-shelf', label: 'Project Shelf', icon: FolderOpen, Page: ProjectShelfPage },
  { pluginId: 'com.orbitos.activity-pulse', routeId: 'usage-analytics', label: 'Pulse', icon: Activity, Page: ({ navigate }) => <ActivityPulsePage navigate={() => navigate('plugins')} /> },
  { pluginId: 'com.orbitos.creator-suite', routeId: 'stream-deck', label: 'Stream Deck', icon: Keyboard, Page: ({ navigate }) => <StreamDeckPage navigate={() => navigate('plugins')} /> },
  { pluginId: 'com.orbitos.workspaces', routeId: 'workspaces', label: 'Espaces', icon: FolderOpen, Page: ({ navigate }) => <WorkspacesPage navigate={() => navigate('plugins')} /> },
  { pluginId: 'com.orbitos.game-sessions', routeId: 'game-sessions', label: 'Game Sessions', icon: Gamepad2, Page: ({ navigate }) => <GameSessionsPage navigate={() => navigate('plugins')} /> },
  { pluginId: 'com.orbitos.downloads-organizer', routeId: 'downloads-organizer', label: 'Downloads Organizer', icon: FolderOpen, Page: ({ navigate }) => <DownloadsOrganizerPage navigate={() => navigate('plugins')} /> },
];
