import { currentRendering } from '../root/render-context';

export interface PlecMutation<TInput, TData, TError = unknown> {
  run(input: TInput): Promise<TData>;
  pending: boolean;
  error: TError | null;
  data: TData | null;
}

/** Compiler-owned declaration. Runtime execution is supplied by compiled actions. */
export function useMutation<TInput, TData, TError = unknown>(
  _callback: (input: TInput) => Promise<TData>,
): PlecMutation<TInput, TData, TError> {
  if (!currentRendering())
    throw new Error(
      'Plec.useMutation can only run while rendering a Plec component.',
    );
  return undefined as never;
}
