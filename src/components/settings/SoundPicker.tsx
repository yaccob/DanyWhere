import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../ui/Button";
import { Dropdown, DropdownOption } from "../ui/Dropdown";
import { PlayIcon } from "lucide-react";
import { SettingContainer } from "../ui/SettingContainer";
import { useSettingsStore } from "../../stores/settingsStore";
import { useSettings } from "../../hooks/useSettings";
import { SOUND_THEME_NAMES, soundThemeName } from "@/lib/soundThemeNames";

interface SoundPickerProps {
  label: string;
  description: string;
}

export const SoundPicker: React.FC<SoundPickerProps> = ({
  label,
  description,
}) => {
  const { t } = useTranslation();
  const { getSetting, updateSetting } = useSettings();
  const playTestSound = useSettingsStore((state) => state.playTestSound);
  const customSounds = useSettingsStore((state) => state.customSounds);
  const checkCustomSounds = useSettingsStore(
    (state) => state.checkCustomSounds,
  );

  const selectedTheme = getSetting("sound_theme") ?? "marimba";

  useEffect(() => {
    checkCustomSounds();
  }, [checkCustomSounds]);

  // DanyWhere: the names are the ones the page "Profile" shows too, the
  // user's own sounds translated.
  const options: DropdownOption[] = [
    { value: "marimba", label: SOUND_THEME_NAMES.marimba },
    { value: "pop", label: SOUND_THEME_NAMES.pop },
  ];

  // Only add Custom option if both custom sound files exist
  if (customSounds.start && customSounds.stop) {
    options.push({ value: "custom", label: soundThemeName("custom", t) });
  }

  // DanyWhere: the sound that ends a dictation, the done sound unless it is
  // switched off, else the one for a transcript left on the clipboard.
  const doneSound = getSetting("done_sound") ?? true;

  const handlePlayAllSounds = async () => {
    await playTestSound("start");
    await playTestSound("stop");
    await playTestSound(doneSound ? "done" : "clipboard");
  };

  return (
    <SettingContainer
      title={label}
      description={description}
      grouped
      layout="horizontal"
    >
      <div className="flex items-center gap-2">
        <Dropdown
          selectedValue={selectedTheme}
          onSelect={(value) =>
            updateSetting("sound_theme", value as "marimba" | "pop" | "custom")
          }
          options={options}
          onOpen={checkCustomSounds}
        />
        <Button
          variant="ghost"
          size="sm"
          onClick={handlePlayAllSounds}
          title={t("settings.debug.soundTheme.preview")}
        >
          <PlayIcon className="h-4 w-4" />
        </Button>
      </div>
    </SettingContainer>
  );
};
