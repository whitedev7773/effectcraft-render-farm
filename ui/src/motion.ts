import type { Theme } from "./api";

export type ChangeTheme = (theme: Theme, anchor: HTMLElement | null) => void;

let activeTransition: ViewTransition | undefined;
const revealDuration = 430;

export function transitionView(apply: () => void, themeAnchor?: HTMLElement) {
  const root = document.documentElement;
  activeTransition?.skipTransition();
  if (!document.startViewTransition || root.hasAttribute("data-reduce-motion") ||
    window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    apply();
    return;
  }

  const clipPath = themeAnchor ? revealCircle(themeAnchor) : null;
  root.dataset.viewMotion = themeAnchor ? "theme" : "page";
  const transition = document.startViewTransition(apply);
  activeTransition = transition;

  const cleanUp = () => {
    if (activeTransition !== transition) return;
    activeTransition = undefined;
    delete root.dataset.viewMotion;
  };
  // A second navigation can skip the animation. Its DOM update still runs.
  void transition.ready.then(() => {
    if (!clipPath || activeTransition !== transition) return;
    root.animate({ clipPath }, { duration: revealDuration, easing: "cubic-bezier(.42, 0, .24, 1)", fill: "both",
      pseudoElement: "::view-transition-new(root)" });
  }).catch(() => undefined);
  void transition.finished.then(cleanUp, cleanUp);
  return transition;
}

function revealCircle(anchor: HTMLElement) {
  const bounds = anchor.getBoundingClientRect();
  const x = bounds.left + bounds.width / 2;
  const y = bounds.top + bounds.height / 2;
  const radius = Math.hypot(Math.max(x, innerWidth - x), Math.max(y, innerHeight - y));
  return [`circle(0px at ${x}px ${y}px)`, `circle(${Math.ceil(radius) + 1}px at ${x}px ${y}px)`];
}
