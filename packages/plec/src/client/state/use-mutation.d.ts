export interface PlecMutation<TInput, TData, TError = unknown> {
    run(input: TInput): Promise<TData>;
    pending: boolean;
    error: TError | null;
    data: TData | null;
}
/** Compiler-owned declaration. Runtime execution is supplied by compiled actions. */
export declare function useMutation<TInput, TData, TError = unknown>(_callback: (input: TInput) => Promise<TData>): PlecMutation<TInput, TData, TError>;
