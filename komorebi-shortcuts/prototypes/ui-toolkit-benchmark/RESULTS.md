# Windows 11 UI toolkit benchmark

## Decision

Use GPUI plus GPUI Components for new first-party shell surfaces, behind a renderer-neutral Rust session core. Keep existing egui surfaces working while callers migrate; do not build new shell behavior into egui.

GPUI wins the interaction, composition, theming, and steady-state runtime comparison. It loses clean-build speed, artifact size, dependency count, private memory, and current Windows accessibility completeness. The accessibility gap is a production gate, not a reason to keep two permanent UI architectures.

## Method

Both release binaries render the same 18 result identities through the same `PaletteState`. The core owns filtering, stable selection, wraparound, and activation identity. Each toolkit only projects that state and translates input. Five fresh-process runs supplied first-frame samples. Three four-second samples supplied steady-state CPU and memory figures. Keyboard and accessibility behavior were driven through the Windows accessibility surface while komorebi and the 50-pixel AppBar were active.

Versions were pinned to eframe 0.33.3, Zed GPUI commit `797e5dc9`, and GPUI Components plus `gpui-base` commit `6d07863f`. The crates.io `gpui-base` 0.1.0 package is an empty placeholder, so it is not an equivalent comparison input.

## Measurements

| Measure | egui | GPUI Components | Result |
|---|---:|---:|---|
| Median first frame | 254.6 ms | 308.4 ms | egui is 53.8 ms faster |
| Median window observation | 513 ms | 535 ms | effectively tied at tool granularity |
| Median idle CPU | 0.341% | 0.098% | GPUI is 71% lower |
| Median working set | 82.6 MB | 64.4 MB | GPUI is 22% lower |
| Median private bytes | 66.7 MB | 82.2 MB | egui is 19% lower |
| Release executable | 5.04 MB | 21.72 MB | egui is 4.3x smaller |
| Clean release build | 35.2 s | 267.9 s | egui is 7.6x faster |
| Clean target directory | 0.61 GB | 2.23 GB | egui is 3.6x smaller |
| Unique normal dependency lines | 166 | 540 | egui is 3.3x smaller |
| Surface source | 237 lines | 245 lines | equivalent prototype complexity |

Raw samples are in `measurements.json`.

## Windows behavior

- Keyboard: both surfaces accepted immediate typing, filtered to the stable result identity, navigated with arrows, and activated with Enter.
- Accessibility: egui exposed the search edit as focused. GPUI accepted input internally but Windows UI Automation continued to report the window root as focused. GPUI list rows required an explicit `gpui-base::Button` semantic child to expose useful names. Neither prototype exposed the selected row through UI Automation.
- AppBar: both windows stayed below the active work-area top of 50 pixels. GPUI remained centered at 722 by 521 logical pixels. The active manager tiled egui to 1216 by 283; egui reflowed without footer overlap after its height constraint was corrected.
- Window-manager treatment: GPUI was not tiled by the running manager; egui was. Production shell windows still need an explicit manager-owned window-role policy instead of depending on either toolkit's incidental native styles.
- Resize: egui rendered correctly under the manager-imposed compact size. GPUI declared a 520 by 360 minimum and remained stable at its requested size, but edge-resize automation could not move its border while the manager was active. This is not evidence of a resize failure.
- DPI: current-monitor rendering was sharp at the active Windows scale. Per-monitor transition behavior is unmeasured because the machine has one monitor. A second-DPI-monitor smoke test remains required before production acceptance.

## Primitive-first production boundary

The production state owner must not be `egui::Context`, `ListState`, `InputState`, or any other toolkit object.

```text
RawShellInput
  -> ToolkitInputAdapter::translate
  -> PaletteIntent
  -> PaletteSession::apply
  -> PaletteSnapshot<ResultHandle>
  -> ToolkitProjection::render
  -> PresentedFrame

Activate(ResultHandle)
  -> PaletteSession::begin_activation
  -> ShellActionPort::execute
  -> ActivationSettlement
  -> PaletteSession::settle
```

`ResultHandle` is the stable identity boundary. Toolkit row indexes are temporary projections and cannot cross into activation. The shell host owns window role, focus restoration, AppBar/work-area placement, DPI changes, and failure settlement. A toolkit adapter may fail to create or present a window; it may not mutate manager state or silently substitute a different result.

## Adoption constraints

1. Make GPUI UI Automation focus, names, and selected state pass before replacing an accessible surface.
2. Pin GPUI and GPUI Components commits until their APIs stabilize; update them as one reviewed dependency unit.
3. Prefer `gpui-base` primitives for always-resident shell controls. Pull higher-level GPUI Components only where they materially reduce interaction code.
4. Keep the shared session core toolkit-free so existing egui callers can migrate and then be deleted without compatibility state leaking into the domain.
5. Validate mixed-DPI movement and manager-owned window roles on the production shell host; this single-monitor prototype cannot settle those system boundaries.
