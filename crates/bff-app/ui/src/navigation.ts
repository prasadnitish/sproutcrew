/** Return the next option for standard single-selection keyboard navigation. */
export function choiceIndex(key: string, current: number, count: number): number {
  if (count < 1 || current < 0 || current >= count) return -1;
  if (key === 'Home') return 0;
  if (key === 'End') return count - 1;
  if (key === 'ArrowRight' || key === 'ArrowDown') return (current + 1) % count;
  if (key === 'ArrowLeft' || key === 'ArrowUp') return (current - 1 + count) % count;
  return -1;
}
