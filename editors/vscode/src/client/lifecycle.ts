export interface ManagedClient { start(): Promise<void>; dispose(): Promise<void> }

/** One replacement queue; a pending start can be disposed before it completes. */
export class ClientLifecycle<T extends ManagedClient> {
  private generation = 0;
  private queue = Promise.resolve();
  private candidate: T | undefined;
  private current: T | undefined;
  private closed = false;
  get client(): T | undefined { return this.current; }

  replace(factory: () => T, ready: (client: T) => void | Promise<void>): Promise<void> {
    if (this.closed) return Promise.resolve();
    const generation = ++this.generation;
    const previous = this.candidate ?? this.current;
    this.current = undefined;
    // Begin disposing now, including a process still in initialize.
    const stopped = previous?.dispose().catch(() => undefined) ?? Promise.resolve();
    this.queue = this.queue.catch(() => undefined).then(async () => {
      await stopped;
      if (this.closed || generation !== this.generation) return;
      const candidate = factory();
      this.candidate = candidate;
      try {
        await candidate.start();
        if (this.closed || generation !== this.generation) {
          await candidate.dispose();
          return;
        }
        this.current = candidate;
        this.candidate = undefined;
        await ready(candidate);
      } catch (error) {
        await candidate.dispose().catch(() => undefined);
        if (this.candidate === candidate) this.candidate = undefined;
        if (this.current === candidate) this.current = undefined;
        if (!this.closed && generation === this.generation) throw error;
      }
    });
    return this.queue;
  }

  /** Dispose the old process before resolving a replacement executable. */
  stop(): Promise<void> {
    ++this.generation;
    const client = this.candidate ?? this.current;
    this.current = undefined;
    const stopped = client?.dispose().catch(() => undefined) ?? Promise.resolve();
    this.queue = this.queue.catch(() => undefined).then(async () => { await stopped; });
    return this.queue;
  }

  async shutdown(): Promise<void> {
    this.closed = true;
    ++this.generation;
    const client = this.candidate ?? this.current;
    this.current = undefined;
    await client?.dispose().catch(() => undefined);
    await this.queue.catch(() => undefined);
    this.candidate = undefined;
  }
}
