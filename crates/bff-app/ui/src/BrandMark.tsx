import { MARK_PATHS, MARK_VIEWBOX } from './brand/geometry';
/** Decorative: the adjacent wordmark supplies the accessible name. */
export type LeafTone = 'blue' | 'apricot' | 'green';
export default function BrandMark({ className = '', leafTone = 'apricot' }: { className?: string; leafTone?: LeafTone }) {
  return <svg className={`brand-mark brand-leaves-${leafTone} ${className}`} viewBox={MARK_VIEWBOX} fill="currentColor" aria-hidden="true" focusable="false">
    <path className="brand-ribbon" d={MARK_PATHS.ribbon} />
    <path className="brand-leaf brand-leaf-left" d={MARK_PATHS.leafLeft} />
    <path className="brand-leaf brand-leaf-right" d={MARK_PATHS.leafRight} />
  </svg>;
}
