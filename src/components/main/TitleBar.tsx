import { Minus, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import IconButton from "@/components/ui/IconButton";

export interface TitleBarProps {
  title?: string;
  leftAction?: { icon: ReactNode; label: string; onClick: () => void };
  onMinimize?: () => void;
  onClose?: () => void;
  rightExtras?: ReactNode;
  "data-testid"?: string;
}

export function TitleBar({
  title,
  leftAction,
  onMinimize,
  onClose,
  rightExtras,
  "data-testid": testId,
}: TitleBarProps) {
  const { t } = useTranslation();
  const displayTitle = title ?? t("app.title");
  return (
    <header className="lw-titlebar" data-testid={testId ?? "titlebar"}>
      <div className="lw-titlebar-left">
        {leftAction && (
          <IconButton
            label={leftAction.label}
            icon={leftAction.icon}
            onClick={leftAction.onClick}
            data-testid="titlebar-left-action"
          />
        )}
      </div>
      <div className="lw-titlebar-drag" data-app-region="drag" data-tauri-drag-region>
        <span className="lw-titlebar-title">{displayTitle}</span>
      </div>
      <div className="lw-titlebar-right">
        {rightExtras}
        {onMinimize && (
          <IconButton
            label={t("common.minimize")}
            icon={<Minus size={14} />}
            onClick={onMinimize}
            data-testid="titlebar-minimize"
          />
        )}
        {onClose && (
          <IconButton
            label={t("common.close")}
            icon={<X size={14} />}
            onClick={onClose}
            data-testid="titlebar-close"
          />
        )}
      </div>
    </header>
  );
}

export default TitleBar;
