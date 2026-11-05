import React from "react";
import { useTranslation } from "react-i18next";
import type { CatalogModel } from "@handy-computer/models";
import { Tooltip } from "@/components/ui/Tooltip";
import { GradeChip } from "@/components/shared/GradeChip";
import { SpeedBars, speedLevel } from "@/components/shared/SpeedBars";
import { languageName } from "@/lib/constants/languages";
import { formatModelSize } from "@/lib/utils/format";
import type { Grading } from "@/lib/models/grading";
import { Gauge, Globe, HardDrive, type LucideIcon } from "lucide-react";
import { gradeMode, headCells, isDense, type HeadColumn } from "./headLabels";

export { gradeMode, isDense };

/** Heads shown as icons: their words don't fit the columns in many locales. */
const HEAD_ICONS: Partial<Record<HeadColumn, LucideIcon>> = {
  all: Globe,
  speed: Gauge,
  size: HardDrive,
};

/** The grade, speed and size columns: the same grid in heads and rows (the
 * widths are COLUMN_PX in headLabels.ts). */
export const columnsClass = (langs: string[]) =>
  gradeMode(langs) === "overall"
    ? "grid-cols-[44px_40px_52px] gap-x-[6px]"
    : isDense(langs)
      ? "grid-cols-[repeat(var(--n),26px)_36px_50px] gap-x-[3px]"
      : "grid-cols-[repeat(var(--n),32px)_40px_52px] gap-x-[6px]";

export const columnsStyle = (langs: string[]) =>
  ({ "--n": langs.length }) as React.CSSProperties;

/** A row's grid: mark, name, columns, and the delete/stop slot if any. */
export const rowGrid = (actions: boolean) =>
  `grid ${actions ? "grid-cols-[18px_minmax(0,1fr)_auto_24px]" : "grid-cols-[18px_minmax(0,1fr)_auto]"} items-center gap-x-[10px]`;

export const CODE_CLASS =
  "text-tag font-semibold leading-[14px] tracking-[0.02em] text-ink-3 uppercase";

/** A part's view-transition name (onboarding's moves), or nothing. */
export type VtName = (part?: string) => Record<string, string> | undefined;

/** The column heads (EN ES … SPEED SIZE). */
export const Codes: React.FC<{ langs: string[]; vt?: VtName }> = ({
  langs,
  vt,
}) => {
  const { t, i18n } = useTranslation();
  return (
    <span
      {...vt?.("c")}
      style={columnsStyle(langs)}
      className={`grid items-center justify-items-center ${columnsClass(langs)}`}
    >
      {headCells(langs, t, (l) => languageName(l, i18n.language)).map(
        (cell) => {
          const Icon = cell.text === null ? HEAD_ICONS[cell.column] : null;
          // The app's tooltip, not `title`: the webview shows no native
          // tooltips. It also opens on a click.
          return (
            <Tooltip key={cell.key} text={cell.label} showOnPress>
              <span aria-label={cell.label} className={CODE_CLASS}>
                {Icon ? (
                  <Icon aria-hidden="true" className="h-[13px] w-[13px]" />
                ) : (
                  cell.text
                )}
              </span>
            </Tooltip>
          );
        },
      )}
    </span>
  );
};

/** Tooltip for a grade chip: what the letter means, or why there's no grade. */
export const GradeTip: React.FC<{
  grading: Grading;
  model: CatalogModel | undefined;
  /** The languages the chip grades: one, or all of yours. */
  langs: string[];
  children: React.ReactElement;
}> = ({ grading, model, langs, children }) => {
  const { t, i18n } = useTranslation();
  const ui = i18n.language;
  const nameOf = (l: string) => languageName(l, ui);
  const list = (codes: string[]) =>
    new Intl.ListFormat(ui, { type: "conjunction" }).format(codes.map(nameOf));
  const one = langs.length === 1;
  const grade = one ? grading.grade(model, langs[0]) : grading.overall(model);
  const key = (k: string) => `modelTable.rubric.${k}`;
  const miss = langs.filter((l) => grading.grade(model, l).unsupported);
  let text: React.ReactNode;
  if (!model) text = t(key("unrated"), { language: list(langs) });
  else if (miss.length) text = t(key("unsupported"), { language: list(miss) });
  else if (grade.letter === "none")
    text = t(key("unrated"), { language: list(langs) });
  else text = t(key(`meaning.${grade.letter}`));
  // Per-language breakdown as text (chip greys don't work on the tooltip);
  // letters and +/− in separate columns so they line up.
  const breakdown = !one && (
    <span className="mt-[6px] grid grid-cols-[1fr_auto_auto]">
      {langs.map((l) => {
        const { label } = grading.grade(model, l);
        return (
          <React.Fragment key={l}>
            <span className="me-[16px]">{nameOf(l)}</span>
            <b className="text-center font-semibold">{label.slice(0, 1)}</b>
            <b className="font-semibold">{label.slice(1)}</b>
          </React.Fragment>
        );
      })}
    </span>
  );
  return (
    <Tooltip
      title={
        one
          ? t(key(grade.letter !== "none" ? "gradeTitle" : "accuracyTitle"), {
              language: nameOf(langs[0]),
              grade: grade.label,
            })
          : t(key(grade.letter !== "none" ? "overallTitle" : "overall"), {
              grade: grade.label,
            })
      }
      text={
        <>
          {text}
          {breakdown}
        </>
      }
    >
      {children}
    </Tooltip>
  );
};

/** How fast, in words: a benchmark band, never measured seconds. */
export const useSpeedWords = () => {
  const { t } = useTranslation();
  return (wait: number | undefined, preview = false) =>
    wait == null
      ? t("modelTable.speedUntested")
      : t(
          `modelTable.${preview ? "speedWordsPreview" : "speedWords"}.${speedLevel(wait)}`,
        );
};

/** Tooltip for the speed bars, shaped like the grade hint. */
export const SpeedTip: React.FC<{
  grading: Grading;
  model: CatalogModel | undefined;
  /** It previews your words while you talk. */
  streaming: boolean;
  className?: string;
  children: React.ReactElement;
}> = ({ grading, model, streaming, className, children }) => {
  const { t } = useTranslation();
  const key = (k: string) => `modelTable.rubric.${k}`;
  const wait = grading.wait(model);
  const level = speedLevel(wait);
  return (
    <Tooltip
      className={className}
      title={
        wait == null
          ? t("modelTable.speed")
          : t(key("speedTitle"), { speed: t(key(`speedName.${level}`)) })
      }
      text={
        wait == null
          ? t(key("untimed"))
          : streaming
            ? t(key("speedPreview"), {
                meaning: t(key(`speedMeaning.${level}`)),
              })
            : t(key(`speedMeaning.${level}`))
      }
    >
      {children}
    </Tooltip>
  );
};

/** A row's grade, speed and size cells. */
export const Values: React.FC<{
  grading: Grading;
  model: CatalogModel | undefined;
  streaming: boolean;
  sizeMb: number;
  vt?: VtName;
}> = ({ grading, model, streaming, sizeMb, vt }) => {
  const { langs } = grading;
  return (
    <span
      {...vt?.("g")}
      style={columnsStyle(langs)}
      className={`grid items-center justify-items-center ${columnsClass(langs)}`}
    >
      {gradeMode(langs) === "each" ? (
        langs.map((l) => {
          const g = grading.grade(model, l);
          return (
            <GradeTip key={l} grading={grading} model={model} langs={[l]}>
              <span className="inline-grid">
                <GradeChip
                  letter={g.letter}
                  label={g.label}
                  dense={isDense(langs)}
                />
              </span>
            </GradeTip>
          );
        })
      ) : (
        <GradeTip grading={grading} model={model} langs={langs}>
          <span className="inline-grid">
            <GradeChip
              letter={grading.overall(model).letter}
              label={grading.overall(model).label}
            />
          </span>
        </GradeTip>
      )}
      <SpeedTip grading={grading} model={model} streaming={streaming}>
        <span className="inline-grid h-[22px] w-[36px] place-items-center">
          <SpeedBars
            level={speedLevel(grading.wait(model))}
            className="h-[12px]!"
          />
        </span>
      </SpeedTip>
      <span
        {...vt?.("s")}
        className="whitespace-nowrap text-label tabular-nums text-ink-3"
      >
        {sizeMb > 0 ? formatModelSize(sizeMb) : "—"}
      </span>
    </span>
  );
};
