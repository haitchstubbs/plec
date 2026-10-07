export async function resolve(specifier, context, nextResolve) {
  if (
    specifier.endsWith('.js') &&
    context.parentURL?.includes('/packages/plec-node/src/')
  ) {
    try {
      return await nextResolve(
        `${specifier.slice(0, -'.js'.length)}.ts`,
        context,
      );
    } catch (error) {
      if (
        !(error instanceof Error) ||
        !('code' in error) ||
        error.code !== 'ERR_MODULE_NOT_FOUND'
      )
        throw error;
    }
  }
  return nextResolve(specifier, context);
}
