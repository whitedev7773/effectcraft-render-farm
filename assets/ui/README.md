# UI assets

- `WantedSans-Regular.ttf` is from [Wanted Sans](https://github.com/wanteddev/wanted-sans), licensed under the SIL Open Font License 1.1. See `OFL.txt`.
- The SVG icons in `icons/` are from [Gravity UI Icons](https://github.com/gravity-ui/icons), licensed under MIT. See `icons/LICENSE`.
- The matching PNG files are 40 px rasterizations of those SVG icons, tinted `#eaeff7` for the dark UI. The app displays them at 18 logical px.
- The desktop interface follows [HeroUI v3's default dark theme variables](https://github.com/heroui-inc/heroui/blob/v3/packages/styles/themes/default/variables.css) and its Card, Button, and field patterns. The OKLCH colors are converted to sRGB in `src/interface.rs` and `src/main.rs`. The React components themselves cannot run in the native egui renderer.
