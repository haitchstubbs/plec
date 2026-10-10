export interface PlecTransportContext {
  pathname: string;
  rawQuery: string;
  rawHeaders?: readonly (readonly [string, string])[];
  scheme: 'http' | 'https';
  authority: string;
  signal: AbortSignal;
}
