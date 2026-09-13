import v8 from 'node:v8';
import { json } from '../_todos';
export function GET() {
  const memory = process.memoryUsage();
  return json({
    timestamp: new Date().toISOString(),
    pid: process.pid,
    uptimeSeconds: Math.round(process.uptime()),
    rssBytes: memory.rss,
    heapTotalBytes: memory.heapTotal,
    heapUsedBytes: memory.heapUsed,
    externalBytes: memory.external,
    arrayBuffersBytes: memory.arrayBuffers,
    heapLimitBytes: v8.getHeapStatistics().heap_size_limit,
    activeResources: process.getActiveResourcesInfo(),
  });
}
