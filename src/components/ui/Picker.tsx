import { useEffect, useRef, useState, type ReactNode } from "react";
import { ChevronDown } from "lucide-react";
import { useTranslation } from "react-i18next";
import TextInput from "./TextInput";

export interface PickerOption<T extends string> {
  value: T;
  label: string;
  description?: string;
  icon?: ReactNode;
  disabled?: boolean;
}

export interface PickerProps<T extends string> {
  value: T;
  options: PickerOption<T>[];
  onChange: (next: T) => void;
  placeholder?: string;
  searchable?: boolean;
  allowCustomValue?: boolean;
  customValueLabel?: (value: string) => string;
  renderTrigger?: (opt: PickerOption<T> | undefined) => ReactNode;
  footer?: ReactNode;
  disabled?: boolean;
  "data-testid"?: string;
}

export function Picker<T extends string>({
  value,
  options,
  onChange,
  placeholder,
  searchable,
  allowCustomValue,
  customValueLabel,
  renderTrigger,
  footer,
  disabled,
  "data-testid": testId,
}: PickerProps<T>) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement | null>(null);

  const selected = options.find((o) => o.value === value);

  useEffect(() => {
    if (!open) return;
    const onDocClick = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDocClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDocClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const filtered = searchable && query
    ? options.filter((o) => `${o.label} ${o.description ?? ""}`.toLowerCase().includes(query.toLowerCase()))
    : options;
  const customValue = query.trim();
  const showCustomValue =
    Boolean(searchable && allowCustomValue && customValue)
    && !options.some((o) => o.value === customValue || o.label.toLowerCase() === customValue.toLowerCase());

  const popoverTestId = testId ? `${testId}-popover` : undefined;

  return (
    <div className="lw-picker" ref={rootRef}>
      <button
        type="button"
        className="lw-picker-trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled}
        data-testid={testId}
        onClick={() => setOpen((v) => !v)}
      >
        {renderTrigger ? (
          renderTrigger(selected)
        ) : (
          <span className="lw-picker-trigger-label">
            {selected ? selected.label : placeholder ?? ""}
          </span>
        )}
        <ChevronDown size={14} aria-hidden="true" />
      </button>
      {open && (
        <div className="lw-picker-popover" role="listbox" data-testid={popoverTestId}>
          {searchable && (
            <div className="lw-picker-search">
              <TextInput
                value={query}
                onChange={setQuery}
                placeholder={placeholder}
                autoFocus
                data-testid={testId ? `${testId}-search` : undefined}
              />
            </div>
          )}
          <div className="lw-picker-list">
            {filtered.length === 0 ? (
              showCustomValue ? null : <div className="lw-picker-empty">{t("settings.noMatchingProvider")}</div>
            ) : (
              filtered.map((opt) => (
                <button
                  key={opt.value}
                  type="button"
                  role="option"
                  aria-selected={opt.value === value}
                  disabled={opt.disabled}
                  className={`lw-picker-option ${opt.value === value ? "lw-picker-option--selected" : ""}`}
                  data-testid={testId ? `${testId}-option-${opt.value}` : undefined}
                  onClick={() => {
                    onChange(opt.value);
                    setOpen(false);
                    setQuery("");
                  }}
                >
                  <span className="lw-picker-option-label">
                    {opt.icon}
                    {opt.label}
                  </span>
                  {opt.description && <span className="lw-picker-option-desc">{opt.description}</span>}
                </button>
              ))
            )}
            {showCustomValue && (
              <button
                type="button"
                role="option"
                aria-selected={false}
                className="lw-picker-option"
                data-testid={testId ? `${testId}-option-custom-value` : undefined}
                onClick={() => {
                  onChange(customValue as T);
                  setOpen(false);
                  setQuery("");
                }}
              >
                <span className="lw-picker-option-label">
                  {customValueLabel ? customValueLabel(customValue) : customValue}
                </span>
              </button>
            )}
          </div>
          {footer && <div className="lw-picker-footer">{footer}</div>}
        </div>
      )}
    </div>
  );
}

export default Picker;
