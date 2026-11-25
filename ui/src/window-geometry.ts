import { cornerRadius, welcomeCorner } from "./window-geometry.json";
import { request, type WindowCornerColor, type WindowCornerReveal } from "./api";

let colorCanvas: CanvasRenderingContext2D | null | undefined;
let previousColor = "";
let colorFrame = 0;

export function readWindowCornerColor(): WindowCornerColor | undefined {
  if (!window.ipc) return undefined;
  const background = getComputedStyle(document.documentElement).getPropertyValue("--background").trim();
  if (!/^#[0-9a-f]{6}$/i.test(background)) return undefined;
  const value = Number.parseInt(background.slice(1), 16);
  const backdrops = document.querySelectorAll<HTMLElement>(".modal__backdrop:has(.farm-dialog)");
  if (backdrops.length) {
    if (colorCanvas === undefined) {
      const canvas = document.createElement("canvas");
      canvas.width = canvas.height = 1;
      colorCanvas = canvas.getContext("2d", { willReadFrequently: true });
    }
    if (colorCanvas) {
      colorCanvas.globalAlpha = 1;
      colorCanvas.fillStyle = background;
      colorCanvas.fillRect(0, 0, 1, 1);
      for (const backdrop of backdrops) {
        const style = getComputedStyle(backdrop);
        colorCanvas.globalAlpha = Number(style.opacity);
        colorCanvas.fillStyle = style.backgroundColor;
        colorCanvas.fillRect(0, 0, 1, 1);
      }
      const [red, green, blue] = colorCanvas.getImageData(0, 0, 1, 1).data;
      return [red, green, blue];
    }
  }
  return [value >> 16, (value >> 8) & 255, value & 255];
}

export function syncWindowCornerColor(color: WindowCornerColor | undefined, reveal?: WindowCornerReveal) {
  if (!color || (!reveal && document.documentElement.dataset.viewMotion === "theme")) return;
  const signature = reveal ? `${color}:${reveal.from}:${reveal.x}:${reveal.y}:${reveal.radius}` : color.join(",");
  if (signature === previousColor) return;
  previousColor = signature;
  void request({ action: "window_corner_color", color, reveal }).catch(() => {
    if (previousColor === signature) previousColor = "";
  });
}

export function scheduleWindowCornerColor() {
  if (!window.ipc) return;
  cancelAnimationFrame(colorFrame);
  colorFrame = requestAnimationFrame(() => {
    colorFrame = requestAnimationFrame(() => syncWindowCornerColor(readWindowCornerColor()));
  });
}

export function followWindowCornerBackdrop(backdrop: HTMLElement | null) {
  if (!backdrop || !window.ipc) return;
  let frame = 0;
  const update = () => {
    const color = readWindowCornerColor();
    const animating = backdrop.getAnimations().some((animation) => animation.pending || animation.playState === "running");
    frame = requestAnimationFrame(() => {
      syncWindowCornerColor(color);
      if (animating) update();
    });
  };
  frame = requestAnimationFrame(update);
  return () => cancelAnimationFrame(frame);
}

export function applyWindowGeometry() {
  const root = document.documentElement;
  root.style.setProperty("--window-radius", `${cornerRadius}px`);

  function resize() {
    const { clientWidth: width, clientHeight: height } = root;
    const { extent, segments } = welcomeCorner;
    const corners = [
      (x: number, y: number) => [x, y],
      (x: number, y: number) => [width - y, x],
      (x: number, y: number) => [width - x, height - y],
      (x: number, y: number) => [y, height - x],
    ];
    // Figma's R20, 60% corner smoothing on the welcome frame.
    const path = corners.map((point, index) => {
      const controlPoints = segments.map(([a, b, c, d, x, y]) =>
        `C${[...point(a, b), ...point(c, d), ...point(x, y)].join(" ")}`);
      return `${index === 0 ? "M" : "L"}${point(0, extent).join(" ")}${controlPoints.join("")}`;
    }).join("");
    root.style.setProperty("--welcome-window-clip", `path("${path}Z")`);
  }

  resize();
  window.addEventListener("resize", resize);
}
