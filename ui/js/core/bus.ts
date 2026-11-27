import type { NexusEventMap } from '../../types/models.js';

type Handler<T = unknown> = (payload: T) => void;

const listeners = new Map<keyof NexusEventMap, Set<Handler<never>>>();

/**
 * Subscribe to a typed event on the Nexus UI event bus.
 */
export function on<K extends keyof NexusEventMap>(
  event: K,
  handler: Handler<NexusEventMap[K]>
): () => void {
  let set = listeners.get(event);
  if (!set) {
    set = new Set();
    listeners.set(event, set);
  }
  set.add(handler as Handler<never>);
  return () => off(event, handler);
}

/**
 * Remove a previously registered handler.
 */
export function off<K extends keyof NexusEventMap>(
  event: K,
  handler: Handler<NexusEventMap[K]>
): void {
  listeners.get(event)?.delete(handler as Handler<never>);
}

/**
 * Synchronously emit an event to all subscribers.
 */
export function emit<K extends keyof NexusEventMap>(
  event: K,
  ...args: NexusEventMap[K] extends undefined ? [] : [payload: NexusEventMap[K]]
): void {
  const set = listeners.get(event);
  if (!set) return;
  const payload = args[0] as NexusEventMap[K];
  for (const handler of [...set]) {
    try {
      (handler as Handler<NexusEventMap[K]>)(payload);
    } catch (err) {
      console.error(`[bus:${String(event)}] handler failed:`, err);
    }
  }
}
