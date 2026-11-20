# UI component references

The Render Farm UI is implemented in native Rust/egui. The references below guide component behavior, hierarchy, and layout; React, CSS, and browser components are not installed or embedded in the application. Existing GPU rendering and scheduling policies are independent of these UI changes.

## Framework comparison

| Reference | Relevant official guidance | Native adaptation |
| --- | --- | --- |
| [Blueprint](https://github.com/palantir/blueprint) | Designed for complex, data-dense desktop interfaces; includes an interactive table package. | Compact, aligned computer rows replace large repeated fleet cards. The desktop monitor allocates enough room for six computers before scrolling. |
| [shadcn/ui Data Table](https://ui.shadcn.com/docs/components/base/data-table) | Composes tables with row actions, filtering, sorting, and selection. | Render queue has a real column header, search, status filtering, progress, and row-level actions. Queue processing order remains unchanged by filtering. |
| [shadcn/ui Sidebar](https://ui.shadcn.com/docs/components/aria/sidebar) | Separates header, grouped navigation, content, and footer regions. | Sidebar separates the application identity, workspace navigation, utilities, and local connection information. Compact windows use top navigation. |
| [Fluent 2 Badge](https://fluent2.microsoft.design/components/web/react/core/badge/usage) | Use short descriptive state labels near the content they describe; use semantic color intentionally and consistent sizes in a context. | Small status badges remain adjacent to each computer or queue task. Text communicates state as well as color. |
| [Fluent 2 Progress Bar](https://fluent2.microsoft.design/components/web/react/core/progressbar/usage) | Prefer determinate progress when possible and pair it with a clear label and useful units. | Render progress uses percentage and completed/total frame counts. Indeterminate preparation is shown separately from completed frames. |
| [Fluent 2 Toolbar](https://fluent2.microsoft.design/components/web/react/core/toolbar/usage) | Group related actions; use meaningful labels and keep destructive actions separate. | Search, state filter, and view selection form a queue toolbar. Task removal remains separated from save and dismissal actions in the detail dialog. |
| [Ant Design Segmented](https://ant.design/components/segmented/) | Single-choice controls can switch associated content or views. | Connected segments select queue list/card views with a clearly persistent selection. |

## Layout decisions

These are application-specific decisions inferred from the references, rather than prescribed framework defaults:

- A render farm needs a fleet overview more than large decorative computer cards. At 1440 × 940, all six fixture computer names must be fully inside their clip rectangles with no scroll interaction.
- A live frame preview and render progress belong above the fleet. Activity details are secondary to current work and computer participation.
- Queue filtering changes the visible subset, not scheduling order, task identifiers, or participation choices.
- Compact windows can scroll; controls and labels must stay inside the window width.
- Slate surfaces, a restrained blue accent, and consistent border, spacing, and typography tokens keep the native components visually related.

## Verification

`src/interface/review.rs` contains isolated fixtures, pointer-driven navigation/filter checks, clipping regressions, theme switching, and modal checks. Debug capture mode (`EFFECTCRAFT_FARM_UI_CAPTURE`) renders the actual native UI in dark and light themes without opening user projects or connecting to the network.

## Project-to-queue workflow

Project selection opens an isolated composition-selection modal. Selecting one renderable composition sets its work area and a suggested MP4 path. Committing validates and adds a queue task; cancellation discards the draft. The queue defaults to cards, with a settings gear revealed on item hover (or keyboard focus). This modal opens the existing per-task render-property editor; running and completed tasks retain their existing editing restrictions.
