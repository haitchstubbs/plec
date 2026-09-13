import { z } from 'zod';
export const SqlIdentifierSchema = z.object({
  __kind: z.literal('identifier'),
  parts: z.array(z.string()).min(1),
});
