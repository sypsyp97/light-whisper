import type { ReactNode } from "react";

export interface SettingsNavItem {
  id: string;
  label: string;
  icon: ReactNode;
}

export interface SettingsNavProps {
  items: SettingsNavItem[];
  activeId: string;
  onNavigate: (id: string) => void;
}

export default function SettingsNav({ items, activeId, onNavigate }: SettingsNavProps) {
  return (
    <nav className="lw-settings-nav" data-testid="settings-nav" aria-label="Settings sections">
      {items.map((item) => {
        const active = item.id === activeId;
        return (
          <button
            key={item.id}
            type="button"
            className={`lw-settings-nav-item ${active ? "lw-settings-nav-item--active" : ""}`}
            aria-current={active ? "true" : undefined}
            data-testid={`settings-nav-${item.id}`}
            onClick={() => onNavigate(item.id)}
          >
            {item.icon}
            <span>{item.label}</span>
          </button>
        );
      })}
    </nav>
  );
}
