import type { HTMLAttributes, ReactNode } from "react";

export interface CardProps extends HTMLAttributes<HTMLElement> {
  children: ReactNode;
}

export default function Card({ children, className, ...rest }: CardProps) {
  return (
    <section {...rest} className={`lw-card ${className ?? ""}`.trim()}>
      {children}
    </section>
  );
}
