export interface ProgressBarProps {
  value: number;
  indeterminate?: boolean;
}

export default function ProgressBar({ value, indeterminate }: ProgressBarProps) {
  const clamped = Math.max(0, Math.min(100, value));
  return (
    <div
      role="progressbar"
      aria-valuenow={indeterminate ? undefined : Math.round(clamped)}
      aria-valuemin={0}
      aria-valuemax={100}
      className={`lw-progress ${indeterminate ? "lw-progress--indeterminate" : ""}`}
    >
      <div className="lw-progress-fill" style={indeterminate ? undefined : { width: `${clamped}%` }} />
    </div>
  );
}
