import { z } from 'zod';

export const SqlQuerySchema = z.object({
  text: z.string(),
  raw: z.string(),
  values: z.array(
    z.union([
      z.string(),
      z.number(),
      z.boolean(),
      z.bigint(),
      z.null(),
      z.date(),
    ]),
  ),
});
