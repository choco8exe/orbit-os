# Echo module

Echo stores clipboard history on the local device. Monitoring is **off by default** and must be
enabled explicitly from the Echo screen. Pausing stops new capture without deleting existing items.

## Native integration

`src-tauri/src/modules/echo.rs` provides Tauri commands used by `src/modules/echo/tauri.ts`:

`echo_snapshot`, `echo_get_entry`, `echo_set_monitoring`, `echo_set_preferences`, `echo_copy_entry`,
`echo_set_favorite`, `echo_set_pinned`, `echo_assign_collection`, `echo_create_collection`,
`echo_delete_entry`, `echo_delete_entries`, `echo_clear_history`, `echo_search`,
`echo_find_duplicates`, `echo_remove_duplicates`.

Core must initialize Echo with `echo::initialize(&app)` after creating the Tauri `AppHandle` and add
all listed command functions to `generate_handler!`. The native state stores data in an Echo-owned
SQLite database under the app data directory. It uses `arboard` only to read/copy clipboard content,
`windows::Win32::System::DataExchange::GetClipboardSequenceNumber` to detect changes, `png` to
encode image previews and `base64` to pass those local previews to the UI.

Clipboard change detection polls the Win32 sequence number every 500 ms while monitoring is enabled.
It avoids reading the clipboard on unchanged polls, but may miss rapid successive copies inside one
interval. Windows clipboard-owner metadata is resolved to an executable basename through the owner
window's process ID; no full path is stored. It can be absent when Windows no longer has an owner
window or cannot resolve that process. Configured exclusions match this basename, and when any
exclusion exists Echo skips capture if the owner cannot be identified. The current screen does not
yet expose an exclusion editor. Configurable global shortcuts, collection deletion, export/backup
and pagination are not implemented in this iteration.

Supported captured values are plain text, URL-like text and images readable by `arboard`. Rich text,
files, and Windows-only proprietary clipboard formats are not captured. Images are opt-in and bounded
by the configured size limit. Unsupported or oversized content is skipped without storing its data.
`echo_snapshot` contains at most 200 entry records, collection metadata for at most 100 collections,
previews and no raw text. Its aggregate image data URLs are capped at 4 MiB; older/larger image
records remain copyable but may have no preview in that snapshot. Search uses `echo_search` to
inspect retained text locally and returns at most 200 hits containing only IDs, previews, favorite/pin
state and up to 32 collection IDs per hit. Search input is capped at 512 UTF-8 bytes.
Search previews are capped at 220 characters in SQL, including for older database rows.

An explicit **Détail** action calls `echo_get_entry({ id })` for one validated UUID at a time. It
first reads only the selected record's type, recorded size, and bounded short preview to prepare a
native Windows confirmation. The full text or image is read and returned only after approval.
Declining reports a cancellation and prevents that detailed payload from being read or returned; the
metadata used to show the prompt has already been read. Only one detail confirmation may be pending;
concurrent requests fail closed until it is closed. Outside Windows, the detail operation fails closed
and returns no content. List and search previews remain available without that confirmation.

The detail query bounds its text and image reads to 5,000,000 raw bytes; oversized text is cut at a
valid UTF-8 boundary and oversized images are omitted, with `truncated` indicating that the stored
value exceeded the return limit. Snapshot image reads are independently bounded before they are
loaded, and previews are length-limited in SQL. Images are base64 encoded in the detail response, so
their IPC representation can be larger than their stored bytes. `echo_copy_entry` continues to
retrieve the stored value natively and copy it to the Windows clipboard without sending the full
content through the webview.

Duplicate detection compares exact text or encoded image bytes. Removal requires an explicit user
confirmation in the UI, keeps the newest copy, and merges favorites, pin state and collection links.
Bulk deletion is capped at 200 selected entries per request. Deleted cells are overwritten through
SQLite `secure_delete`, followed by a WAL checkpoint/truncate on deletion and retention cleanup. This
is best-effort local cleanup, not guaranteed forensic erasure: SSD wear-leveling, filesystem
snapshots, backups, or other copies can retain old bytes. History snapshots show the newest 200
entries; search queries can scan older retained rows in SQLite.

The Rust backend is Windows-specific. A non-Windows compile should use the explicit unsupported
response path rather than claiming clipboard monitoring works.

## Cross-module boundary

`echoService` implements the shared `EchoService` contract. Search returns IDs and previews only;
detail requires an explicit selected entry ID; copy takes an existing Echo entry ID. Neither detail
nor search is exposed to any cloud provider. The service does not contact any cloud provider and
does not put clipboard content in Orbit events or timeline records. The UI requests a fresh snapshot
over the Tauri command bridge when a private local change event is received.

## Dependencies requested for the native package

```toml
arboard = "3"
base64 = "0.22"
chrono = { version = "0.4", features = ["serde"] }
png = "0.17"
rusqlite = { version = "0.37", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
sha2 = "0.10"
windows = { version = "0.62", features = ["Win32_System_DataExchange", "Win32_UI_WindowsAndMessaging"] }
sysinfo = "0.33"
```
