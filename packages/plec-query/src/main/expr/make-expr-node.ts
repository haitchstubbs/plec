import type { SqlQuery } from '#types';
import { DiagnosticsCollector, QueryDiagnostics } from '../diagnostics';
import { EMPTY_EXPR_VALUES } from './empty-expr-values';

export function makeExprNode(
  fields: Record<string, unknown>,
): SqlQuery {
  const collector = DiagnosticsCollector.Builder();
  QueryDiagnostics.incrementBuildCounter(collector, 'exprNodeCount');
  return QueryDiagnostics.measureBuildDiagnosticsStep(
    collector,
    'exprNodeBuildMs',
    () => {
      if (!('text' in fields)) fields.text = '';
      if (!('raw' in fields)) fields.raw = '';
      if (!('values' in fields)) fields.values = EMPTY_EXPR_VALUES;
      return fields as unknown as SqlQuery;
    },
  );
}
