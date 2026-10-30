# Mission Control panorama — UI revision

## Rationale

The previous command screen read as a familiar app shell: a fixed left icon rail, a small brand lockup at its top, and a three-column dashboard. The supplied reference instead reads as one spatial command surface organized around a central planet and the operational controls around it. This revision changes the composition, navigation location and product explanation; it keeps the already approved aubergine, plasma, brass, mint and semantic-status palette.

The Design Taste app draw selected `data-terminal` for a monitoring product used for repeated glances and longer sessions. Its icon-rail shell was deliberately not adopted because the request explicitly removes the fixed rail. The useful parts retained are the measured monitoring density, data-led color and compact controls. The planet and its orbit are ORBIT's product-specific central object.

## What changed

- Removed the fixed sidebar and its left-aligned logo. ORBIT identity is centered in the shared top command header.
- Moved all seven destinations into a horizontal navigation row with visible labels and icon-plus-label accessible names. Active navigation has a persistent underline and `aria-current="page"`.
- Rebuilt Mission Control as an asymmetric panorama: system measurements to the left, the orbital Earth as the largest central scene, and live telemetry plus actual module states on the right. Echo entries and Timeline stay in the lower operational band.
- Kept the three-column panorama through 1103px. At 900px and below, the orbital scene spans first, with controls underneath; at 760px and below, the modules stack into one readable column. The orbital scene appears before the first scroll on the 720px capture.
- Added the product explanation under the Mission Control title and a “Briefing ORBIT” action in both the global header and the home screen.
- Added a native modal `<dialog>` for the briefing. It describes the local modules and queries their current service state; preview-only unavailability is called out explicitly. `showModal()` provides modal focus handling, Escape closes it, and browser focus returns to the invoking control.
- Added a persistent exit control for immersive mode so hiding the command header cannot strand keyboard users.

## Verification

Fresh Chrome captures and the responsive measurement data are in `.taste-skill/orbit-command-panorama/final/`:

- `mission-1440x900.png`
- `mission-1103x929.png`
- `mission-720x900.png`
- `briefing-1440x900.png`

The test profile used `localStorage` with onboarding marked complete in a disposable browser profile only. The shipped source default remains `onboardingComplete: false` in `src/shared/contracts.ts`.

Measured document/body `scrollWidth` equals viewport width at all three sizes (1440, 1103 and 720); no horizontal overflow was observed. At 1103×929, the system, globe and control columns are all visible together. At 720×900, the globe is the first full-width panel and the system/control panels stack below it.

Interaction checks used Chrome DevTools Protocol input: The global briefing action opened the modal, which showed three local module rows and their state text; Escape closed it and returned focus to the invoking button. The modal capture is the file listed above.

`npm run build` passed for this revision. Taste Inspect on the refreshed UI reported **0 FAIL, 4 WARN** in `.taste-skill/orbit-command-panorama/taste-final/report.json`: two tiny uppercase labels, 81% of measured text under 13px (mostly data sublines and technical labels), the development-server caveat, and a 13% saturated-pixel share at 1103px from the intentional Earth/orbit focal scene. The last warning is a deliberate product-identity element, not an invented metric. Exact 1103×929 and other viewport geometry plus interaction evidence is in `final/responsive-interaction-report.json`.

The final `npm run tauri build -- --bundles nsis,msi` exited 0. It produced the Windows NSIS and MSI artifacts listed in the Tauri build output; creation succeeded, but neither installer was run for this UI revision.

Core native states cannot be validated in a browser preview.

## Scope and limits

- This is a composition and product-explanation pass over Mission Control and the shared command navigation. It does not re-audit every Echo, Pulsar, Core, Timeline or Settings screen.
- Browser preview cannot prove Windows Core telemetry, Credential Manager access, clipboard monitoring, scenario execution or native notifications. Those states are described as unavailable in preview, not as running.
- Chromium initialized the Three.js WebGL canvas (`ready=true` at 1440, 1103 and 720px); that does not establish rendering in the packaged Windows WebView2.
- No palette regeneration was done in this pass; the previous distinctive palette was retained to keep this iteration focused on the structural criticism.
