import type { ButtonHTMLAttributes, ReactNode } from "react";

export interface IconButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  label: string;
  icon: ReactNode;
  variant?: "ghost" | "solid";
  "data-testid"?: string;
}

export function IconButton({
  label,
  icon,
  variant = "ghost",
  type = "button",
  className,
  "data-testid": testId,
  ...rest
}: IconButtonProps) {
  const classes = [
    "lw-icon-button",
    variant === "solid" ? "lw-icon-button--solid" : "",
    className ?? "",
  ].filter(Boolean).join(" ");
  return (
    <button
      {...rest}
      type={type}
      aria-label={label}
      title={label}
      className={classes}
      data-testid={testId ?? "ui-icon-button"}
    >
      {icon}
    </button>
  );
}

export default IconButton;
