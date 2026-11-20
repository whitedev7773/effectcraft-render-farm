# UI assets

- `WantedSans-Regular.ttf` is from [Wanted Sans](https://github.com/wanteddev/wanted-sans), licensed under the SIL Open Font License 1.1. See `OFL.txt`.
- The SVG icons in `icons/` are from [Gravity UI Icons](https://github.com/gravity-ui/icons), licensed under MIT. See `icons/LICENSE`.
- The matching PNG files are 40 px rasterizations of those SVG icons, tinted `#eaeff7` for the dark UI. The app displays them at 18 logical px.
- The native egui interface uses a custom slate and blue palette with dark and light modes. Theme colors and typography are defined in `src/main.rs`; navigation and page headings live in `src/interface/shell.rs`, and layout measurements in `src/interface/layout.rs`. Navigation and empty-state illustrations are drawn with egui vector primitives.

- Reusable segmented controls and status badges live in `src/interface/components.rs`. See [UI component references](../../docs/ui-component-references.md) for the official web-framework patterns adapted for the native UI.

- `src/interface/import.rs` owns the isolated project/composition import draft and modal. Queue card settings are revealed on hover in `src/interface/queue.rs`.
