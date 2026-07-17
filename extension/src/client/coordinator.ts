export interface MutationLease {
  release(): void;
}

export interface MutationCoordinator {
  acquire(signal?: AbortSignal): Promise<MutationLease>;
}

interface Waiter {
  resolve: (lease: MutationLease) => void;
  reject: (error: Error) => void;
  signal?: AbortSignal;
  onAbort?: () => void;
}

export class SerialMutationCoordinator implements MutationCoordinator {
  private locked = false;
  private readonly waiters: Waiter[] = [];

  async acquire(signal?: AbortSignal): Promise<MutationLease> {
    if (signal?.aborted) throw abortError();

    if (!this.locked) {
      this.locked = true;
      return this.createLease();
    }

    return new Promise<MutationLease>((resolve, reject) => {
      const waiter: Waiter = { resolve, reject, signal };
      if (signal) {
        waiter.onAbort = () => {
          const index = this.waiters.indexOf(waiter);
          if (index !== -1) this.waiters.splice(index, 1);
          reject(abortError());
        };
        signal.addEventListener("abort", waiter.onAbort, { once: true });
      }
      this.waiters.push(waiter);
    });
  }

  get queueLength(): number {
    return this.waiters.length;
  }

  private createLease(): MutationLease {
    let released = false;
    return {
      release: () => {
        if (released) return;
        released = true;
        this.releaseNext();
      },
    };
  }

  private releaseNext(): void {
    const waiter = this.waiters.shift();
    if (!waiter) {
      this.locked = false;
      return;
    }
    if (waiter.signal && waiter.onAbort) {
      waiter.signal.removeEventListener("abort", waiter.onAbort);
    }
    waiter.resolve(this.createLease());
  }
}

function abortError(): Error {
  const error = new Error("Operation cancelled while waiting for Mage runtime");
  error.name = "AbortError";
  return error;
}
