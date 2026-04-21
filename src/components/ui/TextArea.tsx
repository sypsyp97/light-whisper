import type { TextareaHTMLAttributes } from "react";

export interface TextAreaProps extends Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "onChange"> {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  "data-testid"?: string;
}

export default function TextArea({
  value,
  onChange,
  placeholder,
  className,
  rows = 4,
  "data-testid": testId,
  ...rest
}: TextAreaProps) {
  return (
    <textarea
      {...rest}
      rows={rows}
      value={value}
      placeholder={placeholder}
      className={`lw-text-area ${className ?? ""}`.trim()}
      data-testid={testId}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}
