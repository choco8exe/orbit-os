# Plugin platform

## Packaging and permissions

The Plugin Center lists only first-party integrations compiled into ORBIT. The frontend manifest,
static page provider, native allowlist and command handlers are explicit. Installing a plugin never
downloads code. Windows Credential Manager stores provider-managed secrets; values cannot be read by
the renderer and are not copied into settings, diagnostics or plugin SQLite state.

Each action checks plugin installation, activation and its specific local grant in native code.
Optional grants start disabled. Uninstalling while retaining data (`deleteData=false`) leaves local
module data in place. Choosing data deletion calls a module-owned cleanup hook before registry state
is removed. Missing credentials count as already deleted; Credential Manager failures stop the
uninstall and are shown to the user.

## Included integrations

| Plugin | Behavior |
| --- | --- |
| Project Shelf / FL Studio | Stores references to user-selected `.flp` files and optional notes; it does not inspect project contents. |
| ORBIT Pulse | Records foreground application durations and, optionally, estimated visible background-window time. It cannot identify sites or media playback. |
| Stream Deck | Ships the companion `.streamDeckPlugin`; a temporary pairing code links it to the local ORBIT bridge. |
| Echo Stream Deck Access | Separate opt-ins for copying the latest/previous Echo item or pausing/resuming capture. Clipboard content is not sent to the device or logs. |
| Workspaces | Stores user-selected executables and launches them without shell commands or arguments. |
| Game Sessions | Tracks only explicitly added process executable names; it does not read window titles, game accounts or input. |
| Downloads Organizer | Works only inside one selected folder, previews intended moves, requires confirmation and can undo recorded moves where possible. |

OBS, YouTube Hub and GitHub Control Room are removed. The old OBS password has only a cleanup path;
old YouTube/GitHub secrets are similarly presence-only and deletable. ORBIT cannot revoke external
provider grants. The app retains the internal Stream Deck plugin ID
`com.orbitos.creator-suite` and keyring secret ID `creator.streamdeck.pair-token` across the display
rename.

## Stream Deck pairing

Install the companion from **Plugins → Stream Deck** on a PC with Elgato Stream Deck 7.1+. Activate
the plugin and grant local pairing permission, generate a temporary code, then add **ORBIT Control**
to a Stream Deck key and enter the code in its property panel. The code is single-use and expires in
90 seconds. The companion stores its token through its Windows helper; ORBIT stores the paired token
in Windows Credential Manager. Revoke pairing in ORBIT or dissociate it in the companion inspector.

The fixed action set opens supported ORBIT modules, pauses/resumes Pulsar and Pulse, runs a selected
enabled Pulsar scenario after native confirmation, copies the latest or previous Echo item, and
controls Echo capture when separately permitted. Pulsar actions require the additional optional
`pulsar.streamdeck.control` permission on the Stream Deck plugin; Echo copy and capture remain separate
optional grants in Echo Stream Deck Access. It offers no shell, arbitrary command, general file
access, OBS controls or clipboard display on Stream Deck keys.

## Update and release limits

The maintenance view can query the latest public GitHub release for an owner/repository the user
enters. It only links to the release; it does not download or run an installer. Automatic update is
not configured: there is no published repository, signed update metadata, code-signing identity or
release pipeline yet. Until those exist, distribute the MSI or NSIS installer manually.

## Adding an integration

Add typed contracts, mirrored Rust/TypeScript manifests, a statically imported page, native
permission checks, data lifecycle hooks and bounded failure handling. Test permission denial and
revocation, preservation versus deletion, invalid state and privacy boundaries before packaging.
