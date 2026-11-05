/**
 * What the model table's column heads show: one grade column per language up
 * to 3, dense columns for 4–5, and past 5 a single overall grade (at the
 * Models page's ~540px a sixth column crowds out the name), then speed and
 * size.
 */
const MAX_COLUMNS = 5;
const MAX_ROOMY = 3;

export const gradeMode = (langs: string[]) =>
  langs.length > MAX_COLUMNS ? "overall" : "each";
export const isDense = (langs: string[]) =>
  langs.length > MAX_ROOMY && langs.length <= MAX_COLUMNS;

export type HeadColumn = "lang" | "all" | "speed" | "size";

/** The columns' widths in px; `columnsClass` in columns.tsx uses the same. */
export const COLUMN_PX: Record<
  "roomy" | "dense" | "overall",
  Partial<Record<HeadColumn, number>>
> = {
  roomy: { lang: 32, speed: 40, size: 52 },
  dense: { lang: 26, speed: 36, size: 50 },
  overall: { all: 44, speed: 40, size: 52 },
};

export const layoutOf = (langs: string[]) =>
  gradeMode(langs) === "overall"
    ? "overall"
    : isDense(langs)
      ? "dense"
      : "roomy";

/** One column head: its visible text, or none when it shows an icon, and
 * the label for the tooltip and screen readers. Only the language codes are
 * text; the words for "all languages", speed and size are too long for their
 * columns in many locales (German "Geschwindigkeit" ran over its
 * neighbours), so those heads are icons. */
export interface HeadCell {
  column: HeadColumn;
  key: string;
  text: string | null;
  label: string;
}

export const headCells = (
  langs: string[],
  t: (key: string) => string,
  languageName: (code: string) => string,
): HeadCell[] => {
  const grades: HeadCell[] =
    gradeMode(langs) === "each"
      ? langs.map((l) => ({
          column: "lang",
          key: l,
          text: l,
          label: languageName(l),
        }))
      : [
          {
            column: "all",
            key: "all",
            text: null,
            label: `${t("modelTable.accuracy")}: ${t("modelTable.allLanguages")}`,
          },
        ];
  return [
    ...grades,
    {
      column: "speed",
      key: "speed",
      text: null,
      label: t("modelTable.speed"),
    },
    {
      column: "size",
      key: "size",
      text: null,
      label: t("modelTable.size"),
    },
  ];
};
