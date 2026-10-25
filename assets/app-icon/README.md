# Render Farm icon

Approved design B: the original EffectCraft unicorn engraving, blue field
(`#427cdd`), and a small connected-computers badge at the bottom right.
Approved by the user on 2026-10-25.

Original artwork: EffectCraft contributors / ArtCraft Team.
Source: https://github.com/storytold/effectcraft/blob/ef74d82902ba486891b02afa87e42963cc9b84cc/assets/app-icon/effectcraft.svg
Original SVG SHA-256: `cc723cd8894f2346ece281aab192de33cb92a442dd6466b957fc0dbc539df3f0`.
The engraving paths are unchanged. Render Farm adds the blue field and network badge.
Licensed under MIT OR Apache-2.0; see `LICENSE.txt` here and the repository licenses.

- `farm.svg`: editable master, matching the approved preview.
- `farm-512.png`: large preview.
- `farm-256.png`: embedded runtime window/taskbar icon.
- `farm.ico`: embedded Windows executable, Start Menu and installed-app icon;
  contains 16, 20, 24, 32, 40, 48, 64, 128 and 256px images with transparency.

To regenerate, run `node packaging/icons.cjs` from the repository with the
`sharp` Node package available (or set `NODE_PATH` to its containing node_modules).
Normal Rust and MSI builds do not require Node or Sharp.
