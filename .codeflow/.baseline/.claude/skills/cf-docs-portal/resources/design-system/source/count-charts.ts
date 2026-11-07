import { floors, geometry, stageNames } from '../tokens.ts';
import { segments, ringPath } from './stage-geometry.ts';
export type Stage = typeof stageNames[number];
const namespace = 'http://www.w3.org/2000/svg';

/** Shared kit primitive. Counts remain exact in the accessible title and key;
 * minimum visual extents affect the drawing only. */
export function renderCountChart(host: HTMLElement, counts: Partial<Record<Stage, number>>, kind: 'bar' | 'ring'): void {
  const entries = stageNames.filter(stage => stage in counts).map(stage => [stage, counts[stage]!] as const);
  if (Object.keys(counts).some(stage => !(stageNames as readonly string[]).includes(stage))) throw new Error('Unknown chart stage');
  const width = host.getBoundingClientRect().width;
  if (width <= 0) return;
  const extent = kind === 'bar' ? width : 360;
  const marks = segments(entries.map(([, count]) => count), extent, kind === 'bar' ? floors.minimumSegmentPx : floors.minimumArcDegrees);
  const svg = document.createElementNS(namespace, 'svg');
  svg.setAttribute('role', 'img');
  const title = document.createElementNS(namespace, 'title');
  title.textContent = entries.map(([stage, count]) => `${stage}: ${count}`).join('; ') || 'No counts';
  svg.append(title);
  const radius = geometry.ringRadius, pad = geometry.ringPadding;
  svg.setAttribute('viewBox', kind === 'bar' ? `0 0 ${width} ${geometry.barHeight}` : `${-radius - pad} ${-radius - pad} ${2 * (radius + pad)} ${2 * (radius + pad)}`);
  svg.classList.add(kind === 'bar' ? 'cf-count-bar' : 'cf-count-ring');
  for (const [index, mark] of marks.entries()) {
    if (!mark.count) continue;
    const stage = entries[index]![0], node = document.createElementNS(namespace, kind === 'bar' ? 'rect' : 'path');
    node.setAttribute('data-stage', stage); node.setAttribute('data-count', String(mark.count));
    node.setAttribute('data-extent', String(mark.extent));
    if (kind === 'bar') {
      node.setAttribute('x', String(mark.start)); node.setAttribute('y', '0');
      node.setAttribute('width', String(mark.extent)); node.setAttribute('height', String(geometry.barHeight));
      node.setAttribute('fill', `var(--cf-stage-${stage})`);
    } else {
      node.setAttribute('d', ringPath(mark.start, mark.extent, radius)); node.setAttribute('fill', 'none');
      node.setAttribute('stroke', `var(--cf-stage-${stage})`); node.setAttribute('stroke-width', String(geometry.ringStroke));
    }
    svg.append(node);
  }
  const key = document.createElement('ul'); key.className = 'cf-count-key';
  for (const [stage, count] of entries) { const item = document.createElement('li'); item.textContent = `${stage}: ${count}`; key.append(item); }
  host.replaceChildren(svg, key);
}
