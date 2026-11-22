import React from "react";
import { useTranslation } from "react-i18next";
import { Dropdown } from "../ui/Dropdown";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettings } from "../../hooks/useSettings";
import type { OverlayPosition, OverlayStyle } from "@/bindings";

interface ShowOverlayProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

// DanyWhere: the style and the position are a setting each, so that each
// row gets its own profile wrapper (qa 2026-11-22_1603) and reads its own
// scope's values; `ShowOverlay` draws both, as before.
const selectedStyleOf = (style: unknown): OverlayStyle =>
  (style || "live") as OverlayStyle;

export const OverlayStyleSetting: React.FC<ShowOverlayProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const styleOptions = [
      {
        value: "none",
        label: t("settings.advanced.overlay.style.options.none"),
      },
      {
        value: "minimal",
        label: t("settings.advanced.overlay.style.options.minimal"),
      },
      {
        value: "live",
        label: t("settings.advanced.overlay.style.options.live"),
      },
    ];

    const selectedStyle = selectedStyleOf(getSetting("overlay_style"));

    return (
      <SettingContainer
        title={t("settings.advanced.overlay.style.title")}
        description={t("settings.advanced.overlay.style.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={styleOptions}
          selectedValue={selectedStyle}
          onSelect={(value) =>
            updateSetting("overlay_style", value as OverlayStyle)
          }
          disabled={isUpdating("overlay_style")}
        />
      </SettingContainer>
    );
  },
);

export const OverlayPositionSetting: React.FC<ShowOverlayProps> = React.memo(
  ({ descriptionMode = "tooltip", grouped = false }) => {
    const { t } = useTranslation();
    const { getSetting, updateSetting, isUpdating } = useSettings();

    const positionOptions = [
      {
        value: "bottom",
        label: t("settings.advanced.overlay.position.options.bottom"),
      },
      {
        value: "top",
        label: t("settings.advanced.overlay.position.options.top"),
      },
    ];

    const selectedStyle = selectedStyleOf(getSetting("overlay_style"));
    // Only "top" and "bottom" are selectable; anything else (empty, or a legacy
    // "none" from before the position was retired) falls back to "bottom".
    const selectedPosition: OverlayPosition =
      getSetting("overlay_position") === "top" ? "top" : "bottom";

    if (selectedStyle === "none") return null;

    return (
      <SettingContainer
        title={t("settings.advanced.overlay.position.title")}
        description={t("settings.advanced.overlay.position.description")}
        descriptionMode={descriptionMode}
        grouped={grouped}
      >
        <Dropdown
          options={positionOptions}
          selectedValue={selectedPosition}
          onSelect={(value) =>
            updateSetting("overlay_position", value as OverlayPosition)
          }
          disabled={isUpdating("overlay_position")}
        />
      </SettingContainer>
    );
  },
);

export const ShowOverlay: React.FC<ShowOverlayProps> = React.memo((props) => (
  <>
    <OverlayStyleSetting {...props} />
    <OverlayPositionSetting {...props} />
  </>
));
