import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import IconButton from "./IconButton";
import Button from "./Button";

export interface BannerProps {
  tone: "error" | "info" | "warn";
  message: string;
  action?: { label: string; onClick: () => void; testId?: string };
  onDismiss?: () => void;
  "data-testid"?: string;
}

export function Banner({ tone, message, action, onDismiss, "data-testid": testId }: BannerProps) {
  const { t } = useTranslation();
  return (
    <div role="alert" className={`lw-banner lw-banner--${tone}`} data-testid={testId}>
      <p className="lw-banner-message">{message}</p>
      {action && (
        <div className="lw-banner-action">
          <Button size="sm" variant="ghost" onClick={action.onClick} data-testid={action.testId}>
            {action.label}
          </Button>
        </div>
      )}
      {onDismiss && (
        <IconButton
          label={t("common.close")}
          icon={<X size={14} />}
          onClick={onDismiss}
          className="lw-banner-dismiss"
        />
      )}
    </div>
  );
}

export default Banner;
