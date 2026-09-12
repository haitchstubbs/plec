import { z } from "zod";

export const SqlRawSchema = z.object({
  __kind: z.literal("raw"),
  text: z.string(),
});
