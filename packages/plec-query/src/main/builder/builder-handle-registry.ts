import { runtimeBuilderDrop } from "../../runtime/bridge";
import type { BuilderHandleCell } from "../types";

export const builderHandleRegistry =
  new FinalizationRegistry<BuilderHandleCell>((cell: BuilderHandleCell) => {
    const handles = cell.retired
      ? [...cell.retired, cell.handle]
      : [cell.handle];
    for (const handle of handles) {
      try {
        runtimeBuilderDrop(handle);
      } catch {
        // Binding may not be available during teardown; swallow silently.
      }
    }
  });
