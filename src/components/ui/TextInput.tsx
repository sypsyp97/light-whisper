import type { InputHTMLAttributes } from "react";

export interface TextInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "onChange"> {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  "data-testid"?: string;
}

export function TextInput({
  value,
  onChange,
  placeholder,
  className,
  type = "text",
  "data-testid": testId,
  ...rest
}: TextInputProps) {
  return (
    <input
      {...rest}
      type={type}
      value={value}
      placeholder={placeholder}
      className={`lw-text-input ${className ?? ""}`.trim()}
      data-testid={testId}
      onChange={(e) => onChange(e.target.value)}
    />
  );
}

export default TextInput;
