// Types for the byte copy of the figure grammar module the figure block uses.
export function validateDeclaration(value: unknown, context?: string): void;
export function renderFigure(
  declaration: unknown,
  options?: { idPrefix?: string; bound?: unknown; facts?: unknown; number?: number | null },
): string;
export function entityLabel(value: string): string;
export const ENTITY_ID: RegExp;
export const FAMILIES: readonly string[];
export const SHAPE_CLASSES: readonly string[];
export const TEXT_CLASSES: readonly string[];
