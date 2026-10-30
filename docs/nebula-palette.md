# Nebula Dark — Aubergine Observatory

Mission Control takes the reference's dense instrument-board composition and orbital illustration, then gives it a less generic color signature: an aubergine-black ground, plasma violet for the primary telemetry, warm brass for the second series, and cool mint for actions and focus. Cobalt is confined to the Earth scene. Panels stay neutral and dark; semantic green, amber, and coral identify service states.

## Semantic tokens

| Role | Token | Value | Use |
| --- | --- | --- | --- |
| Canvas | `--color-bg-canvas` | `#090815` | Main desktop ground |
| Rail | `--color-bg-rail` | `#100f21` | Navigation rail |
| Panel | `--color-bg-panel` | `#19172c` | Operational panels |
| Raised | `--color-bg-raised` | `#24213a` | Raised surfaces |
| Input | `--color-bg-input` | `#141327` | Fields and chart wells |
| Selected | `--color-bg-selected` | `#2c2446` | Current destination and selected control |
| Primary text | `--color-text-primary` | `#f1edff` | Titles, values, important labels |
| Secondary text | `--color-text-secondary` | `#c4bfd9` | Explanations and service details |
| Muted icon | `--color-icon-muted` | `#9d97b4` | Quiet icons and non-status marks |
| Primary data | `--color-data-primary` | `#bc9bff` | CPU history and primary orbit |
| Secondary data | `--color-data-secondary` | `#e6b47a` | Memory history and second orbit |
| Action / focus | `--color-action` | `#75ddd0` | Focus, navigation selection, actions |
| Action fill | `--color-action-fill` | `#a9e8de` | Primary action button fill |
| Success | `--color-success` | `#48c994` | Ready states |
| Warning | `--color-warning` | `#f2b96b` | Pending and caution states |
| Error | `--color-error` | `#f27f91` | Failure states |
| Planet ocean | `--earth-ocean` | `#101f52` | Cobalt globe water |
| Planet land | `--earth-land` / `--earth-land-lit` | `#3659a2` / `#5979c2` | Broad stylized continents |
| Planet clouds | `--earth-cloud` | `#c9e5ff` | Low-opacity atmospheric bands |
| Planet rim | `--earth-rim` | `#70e5f4` | Cool edge light |

The secondary data series intentionally uses brass instead of another blue. That separates memory from CPU at a glance and echoes the reference's violet/amber controls. Mint is reserved for interactive cues; status colors remain paired with their text and icons.

## Contrast measurements

WCAG relative-luminance ratios use the rendered panel fill `#19172c`:

| Foreground | Background | Ratio |
| --- | --- | ---: |
| Primary text `#f1edff` | Panel `#19172c` | 15.24:1 |
| Secondary text `#c4bfd9` | Panel `#19172c` | 9.83:1 |
| Muted icon `#9d97b4` | Panel `#19172c` | 6.26:1 |
| Primary data `#bc9bff` | Panel `#19172c` | 7.72:1 |
| Secondary data `#e6b47a` | Panel `#19172c` | 9.31:1 |
| Action/focus `#75ddd0` | Panel `#19172c` | 10.84:1 |
| Success `#48c994` | Panel `#19172c` | 8.38:1 |
| Warning `#f2b96b` | Panel `#19172c` | 9.94:1 |
| Error `#f27f91` | Panel `#19172c` | 6.85:1 |
| Action label `#101022` | Action fill `#a9e8de` | 13.66:1 |
| Control border `#6a6381` | Panel `#19172c` | 3.09:1 |
| Subtle border `#39354e` | Panel `#19172c` | 1.49:1 |

Text and functional control boundaries exceed WCAG AA contrast thresholds. The subtle border is used only for non-text panel edges; orbit guides and chart rules are decorative.
