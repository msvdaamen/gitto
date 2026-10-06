import { useMutation, useQueryClient, type QueryClient } from "@tanstack/solid-query";
import { createStore, produce } from "solid-js/store";

type Operations = Record<string, { running: boolean; error: Error | null }>;

/**
 * Each repository's operations (a pull, say), by query client: whether one is running, and why the
 * last one failed, until that's dismissed or it's run again. Kept here rather than in a component,
 * so it outlives the toolbar showing it: switching repositories shows another one's, and the same
 * one's again when switching back. Set by the mutation's own callbacks, which run whatever is on
 * show. (Not in the mutation cache: `useMutationState` doesn't follow a change of repository.)
 */
const operationsByClient = new WeakMap<QueryClient, ReturnType<typeof createStore<Operations>>>();

function operationsOf(client: QueryClient) {
  let operations = operationsByClient.get(client);
  if (!operations) operationsByClient.set(client, (operations = createStore<Operations>({})));
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
  const [, setOperations] = operationsOf(useQueryClient());
  const key = (id: string) => `${name}:${id}`;
  const mutation = useMutation(() => ({
    mutationFn: ({ id, input }: { id: string; input: T }) => run(id, input),
    onMutate: ({ id }) => setOperations(key(id), { running: true, error: null }),
    onError: (error, { id }) => setOperations(key(id), { running: false, error }),
    // Nothing to keep for one that went through.
    onSuccess: (_data, { id }) => setOperations(produce((all) => delete all[key(id)])),
  }));

  const state = useRepositoryOperationState(name, repositoryId);
  return {
    /**
     * Runs it, unless it's running; `onSuccess` runs once it's done, with what it resolved to, if
     * this is still on show.
     */
    run(input: T, options?: { onSuccess?: (result: R) => void }) {
      if (state.isPending()) return;
      mutation.mutate(
        { id: repositoryId(), input },
        { onSuccess: (result) => options?.onSuccess?.(result) },
      );
    },
    ...state,
  };
}

/**
 * Whether the repository's operation `name` is running, and why it last failed, for showing it
 * somewhere other than where it's run.
 */
export function useRepositoryOperationState(name: string, repositoryId: () => string) {
  const [operations, setOperations] = operationsOf(useQueryClient());
  const key = () => `${name}:${repositoryId()}`;
  return {
    isPending: () => !!operations[key()]?.running,
    error: () => operations[key()]?.error ?? null,
    /** Forgets why it last failed. */
    dismiss() {
      const id = key();
      if (!operations[id]?.running) setOperations(produce((all) => delete all[id]));
    },
  };
}
