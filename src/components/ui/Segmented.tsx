import type { ReactNode } from "react";

export interface SegmentedProps<T extends string> {
  value: T;
  options: Array<{ value: T; label: string; icon?: ReactNode }>;
  onChange: (next: T) => void;
  "data-testid"?: string;
}

export function Segmented<T extends string>({ value, options, onChange, "data-testid": testId }: SegmentedProps<T>) {
  return (
    <div role="radiogroup" className="lw-segmented" data-testid={testId}>
      {options.map((opt) => {
        const active = opt.value === value;
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={active}
            className="lw-segmented-btn"
            data-testid={testId ? `${testId}-seg-${opt.value}` : undefined}
            onClick={() => onChange(opt.value)}
          >
            {opt.icon}
            {opt.label}
          </button>
        );
      })}
    </div>
  );
}

export default Segmented;
