import { useState } from "react";
import { Eye, EyeOff } from "lucide-react";
import { useTranslation } from "react-i18next";
import IconButton from "./IconButton";

export interface SecretInputProps {
  value: string;
  onChange: (next: string) => void;
  placeholder?: string;
  "data-testid"?: string;
}

export function SecretInput({ value, onChange, placeholder, "data-testid": testId }: SecretInputProps) {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  return (
    <div className="lw-secret-input">
      <input
        type={visible ? "text" : "password"}
        className="lw-text-input"
        value={value}
        placeholder={placeholder}
        aria-label={placeholder}
        data-testid={testId}
        onChange={(e) => onChange(e.target.value)}
      />
      <div className="lw-secret-input-reveal">
        <IconButton
          label={visible ? t("common.hide") : t("common.show")}
          icon={visible ? <EyeOff size={14} /> : <Eye size={14} />}
          onClick={() => setVisible((v) => !v)}
          data-testid={testId ? `${testId}-reveal` : undefined}
        />
      </div>
    </div>
  );
}

export default SecretInput;
