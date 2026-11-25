import type { Theme } from "./api";
import { readWindowCornerColor, scheduleWindowCornerColor, syncWindowCornerColor } from "./window-geometry";

export type ChangeTheme = (theme: Theme, anchor: HTMLElement | null) => void;

let activeTransition: ViewTransition | undefined;
const revealDuration = 430;
const revealPresentationDelay = 64;

export function transitionView(apply: () => void, themeAnchor?: HTMLElement) {
  const root = document.documentElement;
  const previousThemeTransition = root.dataset.viewMotion === "theme";
  activeTransition?.skipTransition();
  if (!document.startViewTransition || root.hasAttribute("data-reduce-motion") ||
    window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    apply();
    return;
  }

  const circle = themeAnchor ? revealCircle(themeAnchor) : null;
  const from = circle ? readWindowCornerColor() : undefined;
  root.dataset.viewMotion = themeAnchor ? "theme" : "page";
  if (previousThemeTransition && !circle) scheduleWindowCornerColor();
  const transition = document.startViewTransition(apply);
  activeTransition = transition;
  let frame = 0;

  const cleanUp = () => {
    cancelAnimationFrame(frame);
    if (activeTransition !== transition) return;
    activeTransition = undefined;
    delete root.dataset.viewMotion;
    if (circle) window.setTimeout(() => syncWindowCornerColor(readWindowCornerColor()), revealPresentationDelay);
  };
  // A second navigation can skip the animation. Its DOM update still runs.
  void transition.ready.then(() => {
    if (!circle || activeTransition !== transition) return;
    const { x, y, radius } = circle;
    const clipPath = [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`];
    const reveal = root.animate({ clipPath }, { duration: revealDuration, easing: "cubic-bezier(.42, 0, .24, 1)", fill: "both",
      pseudoElement: "::view-transition-new(root)" });
    const color = readWindowCornerColor();
    if (!from || !color) return;
    const samples: { readyAt: number; progress: number }[] = [];
    const sample = (now: number) => {
      if (activeTransition !== transition) return;
      let progressToPaint: number | undefined;
      while (samples.length > 0 && samples[0].readyAt <= now) progressToPaint = samples.shift()?.progress;
      if (progressToPaint !== undefined) {
        syncWindowCornerColor(color, { from, x, y, radius: radius * progressToPaint });
      }
      const progress = reveal.effect?.getComputedTiming().progress ?? null;
      // WebView2 and the native corner windows reach the desktop compositor independently.
      if (progress !== null) samples.push({ readyAt: now + revealPresentationDelay, progress });
      frame = requestAnimationFrame(sample);
    };
    frame = requestAnimationFrame(sample);
  }).catch(() => undefined);
  void transition.finished.then(cleanUp, cleanUp);
  return transition;
}

function revealCircle(anchor: HTMLElement) {
  const bounds = anchor.getBoundingClientRect();
  const x = bounds.left + bounds.width / 2;
  const y = bounds.top + bounds.height / 2;
  const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
  return { x, y, radius: Math.ceil(radius) + 1 };
}
