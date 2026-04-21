import { useId, type ReactNode } from "react";

export interface FieldProps {
  label: string;
  hint?: string;
  error?: string;
  children: ReactNode;
  "data-testid"?: string;
}

export default function Field({ label, hint, error, children, "data-testid": testId }: FieldProps) {
  const id = useId();
  return (
    <div className="lw-field" data-testid={testId ?? "ui-field"}>
      <label className="lw-field-label" htmlFor={id}>{label}</label>
      <div id={id}>{children}</div>
      {hint && <span className="lw-field-hint">{hint}</span>}
      {error && <span className="lw-field-error">{error}</span>}
    </div>
  );
}
