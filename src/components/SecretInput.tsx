import { useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { Eye, EyeOff } from "lucide-react";

interface SecretInputProps {
  value: string;
  placeholder: string;
  onChange: (value: string) => void;
  inputClassName?: string;
  inputStyle?: CSSProperties;
  buttonStyle?: CSSProperties;
  ariaLabel?: string;
  ariaLabelShow?: string;
  ariaLabelHide?: string;
}

export default function SecretInput({
  value,
  placeholder,
  onChange,
  inputClassName = "settings-input",
  inputStyle,
  buttonStyle,
  ariaLabel,
  ariaLabelShow,
  ariaLabelHide,
}: SecretInputProps) {
  const { t } = useTranslation();
  const effectiveShowLabel = ariaLabelShow ?? t("common.show");
  const effectiveHideLabel = ariaLabelHide ?? t("common.hide");
  const [visible, setVisible] = useState(false);

  return (
    <div className="settings-row" style={{ position: "relative" }}>
      <input
        type={visible ? "text" : "password"}
        className={inputClassName}
        placeholder={placeholder}
        aria-label={ariaLabel ?? placeholder}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        style={{ flex: 1, padding: "8px 36px 8px 10px", ...inputStyle }}
      />
      <button
        type="button"
        className="icon-btn plain"
        onClick={() => setVisible((current) => !current)}
        style={{
          position: "absolute",
          right: 4,
          top: "50%",
          transform: "translateY(-50%)",
          ...buttonStyle,
        }}
        aria-label={visible ? effectiveHideLabel : effectiveShowLabel}
      >
        {visible ? <EyeOff size={14} /> : <Eye size={14} />}
      </button>
    </div>
  );
}
