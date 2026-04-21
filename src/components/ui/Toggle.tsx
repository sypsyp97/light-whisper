export interface ToggleProps {
  checked: boolean;
  onChange: (next: boolean) => void;
  label?: string;
  disabled?: boolean;
  "data-testid"?: string;
}

export function Toggle({ checked, onChange, label, disabled, "data-testid": testId }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      className="lw-toggle"
      data-testid={testId}
      onClick={() => onChange(!checked)}
    >
      <span className="lw-toggle-knob" />
    </button>
  );
}

export default Toggle;
