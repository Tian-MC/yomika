// Fake event bus used by the mock backend (browser demo + tests only).
import type { UiEventMap } from "../api";

type AnyHandler = (payload: unknown) => void;
const handlers = new Map<string, Set<AnyHandler>>();

export function mockListen<E extends keyof UiEventMap>(event: E, handler: (payload: UiEventMap[E]) => void): () => void {
  let set = handlers.get(event);
  if (!set) {
    set = new Set();
    handlers.set(event, set);
  }
  const h = handler as AnyHandler;
  set.add(h);
  return () => {
    set.delete(h);
  };
}

export function mockEmit<E extends keyof UiEventMap>(event: E, payload: UiEventMap[E]): void {
  const set = handlers.get(event);
  if (!set) return;
  for (const h of [...set]) {
    try {
      h(structuredClone(payload));
    } catch (e) {
      console.error("mock event handler failed", e);
    }
  }
}

export function clearMockListeners(): void {
  handlers.clear();
}
