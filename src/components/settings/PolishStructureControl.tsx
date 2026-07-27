import type { CSSProperties } from "react";
import { useId } from "react";
import { useTranslation } from "react-i18next";

import type { PolishStructureLevel } from "@/types";

const STRUCTURE_LEVELS = [
  { value: "off", labelKey: "settings.structureLevelOff", descKey: "settings.structureLevelOffDesc" },
  { value: "light", labelKey: "settings.structureLevelLight", descKey: "settings.structureLevelLightDesc" },
  { value: "balanced", labelKey: "settings.structureLevelBalanced", descKey: "settings.structureLevelBalancedDesc" },
  { value: "strong", labelKey: "settings.structureLevelStrong", descKey: "settings.structureLevelStrongDesc" },
] as const satisfies ReadonlyArray<{
  value: PolishStructureLevel;
  labelKey: string;
  descKey: string;
}>;

interface PolishStructureControlProps {
  level: PolishStructureLevel;
  onChange: (level: PolishStructureLevel) => void;
}

export default function PolishStructureControl({ level, onChange }: PolishStructureControlProps) {
  const { t } = useTranslation();
  const controlId = useId();
  const rangeId = `${controlId}-range`;
  const descriptionId = `${controlId}-description`;
  const selectedIndex = Math.max(0, STRUCTURE_LEVELS.findIndex((option) => option.value === level));
  const selected = STRUCTURE_LEVELS[selectedIndex];
  const progress = `${(selectedIndex / (STRUCTURE_LEVELS.length - 1)) * 100}%`;

  return (
    <div
      className="polish-structure-control"
      style={{ "--structure-progress": progress } as CSSProperties}
    >
      <div className="settings-row polish-structure-heading">
        <div className="settings-column" style={{ gap: 2 }}>
          <span className="permission-label">{t("settings.structureLevel")}</span>
          <span className="settings-hint" style={{ margin: 0 }}>
            {t("settings.structureLevelHint")}
          </span>
        </div>
        <output className="polish-structure-value" htmlFor={rangeId}>
          {t(selected.labelKey)}
        </output>
      </div>

      <input
        id={rangeId}
        className="polish-structure-range"
        type="range"
        min={0}
        max={STRUCTURE_LEVELS.length - 1}
        step={1}
        value={selectedIndex}
        aria-label={t("settings.structureLevel")}
        aria-valuetext={t(selected.labelKey)}
        aria-describedby={descriptionId}
        onChange={(event) => onChange(STRUCTURE_LEVELS[Number(event.target.value)].value)}
      />

      <div className="polish-structure-scale" aria-hidden="true">
        {STRUCTURE_LEVELS.map((option) => (
          <span key={option.value} data-active={option.value === selected.value}>
            {t(option.labelKey)}
          </span>
        ))}
      </div>

      <p id={descriptionId} className="polish-structure-description">
        {t(selected.descKey)}
      </p>
    </div>
  );
}
