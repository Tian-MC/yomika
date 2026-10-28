// Fake event bus used by the mock backend (browser demo + tests only).
import type { EventMap } from "../../contract";

type Handler<E extends keyof EventMap> = (payload: EventMap[E]) => void;

const handlers: { [E in keyof EventMap]?: Set<Handler<E>> } = {};

export function mockListen<E extends keyof EventMap>(event: E, handler: Handler<E>): () => void {
  let set = handlers[event] as Set<Handler<E>> | undefined;
  if (!set) {
    set = new Set();
    (handlers as Record<string, Set<Handler<E>>>)[event] = set;
  }
  set.add(handler);
  return () => {
    set.delete(handler);
  };
}

export function mockEmit<E extends keyof EventMap>(event: E, payload: EventMap[E]): void {
  const set = handlers[event] as Set<Handler<E>> | undefined;
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
  for (const key of Object.keys(handlers)) delete (handlers as Record<string, unknown>)[key];
}
