import type { ButtonHTMLAttributes, ReactNode } from "react";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "lg";
  loading?: boolean;
  icon?: ReactNode;
  "data-testid"?: string;
}

export default function Button({
  variant = "secondary",
  size = "md",
  loading = false,
  icon,
  type = "button",
  className,
  children,
  disabled,
  "data-testid": testId,
  ...rest
}: ButtonProps) {
  const classes = [
    "lw-button",
    variant !== "secondary" ? `lw-button--${variant}` : "",
    size !== "md" ? `lw-button--${size}` : "",
    className ?? "",
  ].filter(Boolean).join(" ");

  return (
    <button
      {...rest}
      type={type}
      disabled={disabled || loading}
      className={classes}
      data-testid={testId ?? "ui-button"}
    >
      {loading ? <span className="lw-spinner" aria-hidden="true" /> : icon}
      {children}
    </button>
  );
}
