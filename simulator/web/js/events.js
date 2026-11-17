// Port of brain-rs/src/core/events.rs (only the events the sim needs).
// The Rust bus is a tokio broadcast channel: publish never re-enters a
// subscriber. We mimic that with a FIFO that is drained after each publish.
export class EventBus {
  constructor() { this.subs = []; this.queue = []; this.draining = false; }
  subscribe(fn) { this.subs.push(fn); return () => { this.subs = this.subs.filter((s) => s !== fn); }; }
  publish(event) {
    this.queue.push(event);
    if (this.draining) return;
    this.draining = true;
    try {
      while (this.queue.length) {
        const e = this.queue.shift();
        for (const s of this.subs) s(e);
      }
    } finally { this.draining = false; }
  }
}
