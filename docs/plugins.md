# ORBIT Plugin Center (API v1)

## Trust and packaging

ORBIT's catalogue is a closed list compiled into the application. Installing an entry saves local
state; it does not download, import or execute plugin code. Every page is statically registered in
`src/app/plugin-providers.tsx`, while native commands enforce permissions in Rust.

## Included plugins

| Plugin | Route | Purpose |
| --- | --- | --- |
| Project Shelf (`com.orbitos.fl-studio`) | `project-shelf` | Local `.flp` references, tags and optional session notes. |
| ORBIT Pulse (`com.orbitos.activity-pulse`) | `usage-analytics` | Local application duration history. |
| Stream Deck (`com.orbitos.creator-suite`) | `stream-deck` | Local companion pairing and approved ORBIT controls. The stable internal ID and `creator.streamdeck.pair-token` secret ID are retained. |
| Echo Stream Deck Access (`com.orbitos.echo-streamdeck`) | none | Separate copy and capture permissions for Stream Deck actions. |
| Workspaces (`com.orbitos.workspaces`) | `workspaces` | User-selected application groups. |
| Game Sessions (`com.orbitos.game-sessions`) | `game-sessions` | Opt-in durations for explicitly selected process names. |
| Downloads Organizer (`com.orbitos.downloads-organizer`) | `downloads-organizer` | Simulated and confirmed moves inside a selected directory. |

OBS, GitHub Control Room and YouTube Hub are not catalog entries. The old OBS password is
addressable only to report its presence and remove it. Old GitHub/YouTube credentials also have a
presence-and-delete cleanup path; ORBIT cannot revoke consent with those external providers.

## Contracts, permissions and cleanup

Frontend interfaces live in `src/shared/contracts.ts` and manifests in `src/shared/plugins.ts`.
Native IDs, routes, permissions and secret slots are mirrored in `src-tauri/src/plugins.rs`. Keep
these registries in sync. Do not create routes or executable behavior from JSON metadata.

Each native operation checks the plugin's installed/enabled state and its narrow permission. Optional
permissions default off. Revoking a required permission disables the plugin. Secret values use
Windows Credential Manager and are never returned to the renderer. Uninstall has two explicit paths:
`deleteData=false` preserves local module data; `deleteData=true` invokes the module cleanup hook.
The Stream Deck pairing secret is removed with that plugin's declared secret slot, independently of
FL Studio notes.

## Adding a first-party plugin

1. Define typed route, permission, secret and event contracts.
2. Mirror the manifest and permission allowlist in TypeScript and Rust; add a static page provider.
3. Implement native operations with per-call permission checks and clear failure handling.
4. Add tests for disabled state, denied/revoked permission, uninstall preservation/deletion, and
   sensitive-data boundaries.
5. Include the module in backup, diagnostics, docs and Windows packaging as appropriate.

ORBIT is not an external marketplace and does not load third-party plugins.
