import { useQueryClient, type QueryClient } from "@tanstack/solid-query";
import { createStore } from "solid-js";

import { gitKeys } from "./keys";

/**
 * Each repository's operations (a pull, say), by query client: which are running, and why the last
 * of each failed, until that's dismissed or it's run again. Kept here rather than in a component,
 * so it outlives the toolbar showing it: switching repositories shows another one's, and the same
 * one's again when switching back. Written as an operation starts and ends, whatever is on show.
 * Not run as a query mutation: that's a transition, which holds whatever reads what it writes,
 * like switching to another repository, which reads whether its own operations are running.
 */
interface Operations {
  running: Record<string, true>;
  errors: Record<string, Error>;
}

function createOperations() {
  return {
    running: createStore<Operations["running"]>({}),
    errors: createStore<Operations["errors"]>({}),
  };
}

const operationsByClient = new WeakMap<QueryClient, ReturnType<typeof createOperations>>();

function operationsOf(client: QueryClient) {
  let operations = operationsByClient.get(client);
  if (!operations) operationsByClient.set(client, (operations = createOperations()));
  return operations;
}

/**
 * Runs `run` on the repository, as the operation `name`: whether it's running, and why it last
 * failed. One runs at a time per repository. `run` is passed the repository's id rather than reading
 * it when it ends, so the one it ran on is updated and refreshed even if another is on show by then.
 */
export function useRepositoryOperation<T = void, R = unknown>(
  name: string,
  repositoryId: () => string,
  run: (repositoryId: string, input: T) => Promise<R>,
) {
  const operations = operationsOf(useQueryClient());
  const [, setRunning] = operations.running;
  const [, setErrors] = operations.errors;
  const key = (id: string) => `${name}:${id}`;
  const settle = (id: string, error?: Error) => {
    setRunning((all) => {
      delete all[key(id)];
    });
    if (error) {
      setErrors((all) => {
        all[key(id)] = error;
      });
    }
  };

  const state = useRepositoryOperationState(name, repositoryId);
  return {
    /**
     * Runs it, unless it's running; `onSuccess` runs once it's done, with what it resolved to, if
     * this is still on show.
     */
    run(input: T, options?: { onSuccess?: (result: R) => void }) {
      if (state.isPending()) return;
      const id = repositoryId();
      setRunning((all) => {
        all[key(id)] = true;
      });
      setErrors((all) => {
        delete all[key(id)];
      });
      run(id, input).then(
        (result) => {
          settle(id);
          options?.onSuccess?.(result);
        },
        (error: unknown) => settle(id, error instanceof Error ? error : new Error(String(error))),
      );
    },
    ...state,
  };
}

/**
 * Runs `action` on the repository `id`, then reloads the repository, also after a failure: the
 * action may have changed it partway, e.g. a pop that conflicts.
 */
export async function reloading(
  queryClient: QueryClient,
  id: string,
  action: Promise<unknown>,
): Promise<void> {
  try {
    await action;
  } finally {
    await queryClient.invalidateQueries({ queryKey: gitKeys.repository(id) });
  }
}

/**
 * `operation`, which doesn't run while `busy()` says one of its group is: for operations that
 * are shown together, like the stashes', so what's on show is never from before another.
 */
export function oneAtATime<T, R>(
  operation: ReturnType<typeof useRepositoryOperation<T, R>>,
  busy: () => boolean,
) {
  return {
    ...operation,
    run: (input: T, options?: { onSuccess?: (result: R) => void }) => {
      if (!busy()) operation.run(input, options);
    },
  };
}

/**
 * Whether the repository's operation `name` is running, and why it last failed, for showing it
 * somewhere other than where it's run.
 */
export function useRepositoryOperationState(name: string, repositoryId: () => string) {
  const operations = operationsOf(useQueryClient());
  const [running] = operations.running;
  const [errors, setErrors] = operations.errors;
  const key = () => `${name}:${repositoryId()}`;
  return {
    isPending: () => !!running[key()],
    // Not the last one's while it runs again: that it's gone is only written once it's done.
    error: () => (running[key()] ? null : (errors[key()] ?? null)),
    /** Forgets why it last failed. */
    dismiss() {
      const id = key();
      if (!running[id]) {
        setErrors((all) => {
          delete all[id];
        });
      }
    },
  };
}
