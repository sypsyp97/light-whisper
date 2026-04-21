import type { ReactNode } from "react";

export interface BadgeProps {
  tone?: "neutral" | "success" | "warn" | "danger" | "accent";
  children: ReactNode;
  "data-testid"?: string;
}

export default function Badge({ tone = "neutral", children, "data-testid": testId }: BadgeProps) {
  const cls = tone !== "neutral" ? `lw-badge lw-badge--${tone}` : "lw-badge";
  return <span className={cls} data-testid={testId}>{children}</span>;
}
