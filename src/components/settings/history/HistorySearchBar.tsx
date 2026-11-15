// DanyWhere: the search fields above History (`history_search.rs`): the
// text, the window it was dictated in, and the toggles for regular
// expressions, case and whole words.
import React from "react";
import { useTranslation } from "react-i18next";
import {
  AppWindow,
  CaseSensitive,
  Regex,
  Search,
  WholeWord,
  X,
} from "lucide-react";
import { splitHighlights, type SearchFields } from "./historySearch";

const Toggle: React.FC<{
  active: boolean;
  onClick: () => void;
  title: string;
  children: React.ReactNode;
}> = ({ active, onClick, title, children }) => (
  <button
    type="button"
    onClick={onClick}
    title={title}
    aria-label={title}
    aria-pressed={active}
    className={`p-1.5 rounded-md border flex items-center justify-center transition-colors cursor-pointer ${
      active
        ? "border-logo-primary bg-logo-primary/15 text-logo-primary"
        : "border-transparent text-ink-3 hover:text-logo-primary"
    }`}
  >
    {children}
  </button>
);

const Field: React.FC<{
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  title: string;
  invalid: boolean;
  icon: React.ReactNode;
  className?: string;
}> = ({ value, onChange, placeholder, title, invalid, icon, className }) => {
  const { t } = useTranslation();
  return (
    <div className={`relative flex items-center min-w-0 ${className ?? ""}`}>
      <span className="absolute start-2 text-ink-3 pointer-events-none">
        {icon}
      </span>
      <input
        type="text"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape" && value.length > 0) {
            event.preventDefault();
            onChange("");
          }
        }}
        placeholder={placeholder}
        title={title}
        aria-label={placeholder}
        aria-invalid={invalid || undefined}
        spellCheck={false}
        autoComplete="off"
        className={`w-full ps-7 pe-7 py-1.5 text-sm bg-wash border rounded-md transition-colors focus:outline-none ${
          invalid
            ? "border-error focus:border-error"
            : "border-line-strong hover:border-logo-primary focus:border-logo-primary"
        }`}
      />
      {value.length > 0 && (
        <button
          type="button"
          onClick={() => onChange("")}
          title={t("settings.history.search.clear")}
          aria-label={t("settings.history.search.clear")}
          className="absolute end-1.5 p-0.5 rounded text-ink-3 hover:text-logo-primary cursor-pointer"
        >
          <X width={14} height={14} />
        </button>
      )}
    </div>
  );
};

/** The search fields; `invalid` marks the field whose regular expression
 * doesn't compile. */
export const HistorySearchBar: React.FC<{
  fields: SearchFields;
  onChange: (fields: SearchFields) => void;
  invalid: "text" | "window" | null;
}> = ({ fields, onChange, invalid }) => {
  const { t } = useTranslation();
  const set = (change: Partial<SearchFields>) =>
    onChange({ ...fields, ...change });
  return (
    <div className="px-4 flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-2">
        <Field
          className="flex-[2_1_14rem]"
          value={fields.text}
          onChange={(text) => set({ text })}
          placeholder={t("settings.history.search.text")}
          title={t("settings.history.search.textTooltip")}
          invalid={invalid === "text"}
          icon={<Search width={14} height={14} />}
        />
        <div className="flex items-center gap-0.5">
          <Toggle
            active={fields.case_sensitive}
            onClick={() => set({ case_sensitive: !fields.case_sensitive })}
            title={t("settings.history.search.caseSensitive")}
          >
            <CaseSensitive width={16} height={16} />
          </Toggle>
          <Toggle
            active={fields.whole_word}
            onClick={() => set({ whole_word: !fields.whole_word })}
            title={t("settings.history.search.wholeWord")}
          >
            <WholeWord width={16} height={16} />
          </Toggle>
          <Toggle
            active={fields.regex}
            onClick={() => set({ regex: !fields.regex })}
            title={t("settings.history.search.regex")}
          >
            <Regex width={16} height={16} />
          </Toggle>
        </div>
        <Field
          className="flex-[1_1_10rem]"
          value={fields.window}
          onChange={(window) => set({ window })}
          placeholder={t("settings.history.search.window")}
          title={t("settings.history.search.windowTooltip")}
          invalid={invalid === "window"}
          icon={<AppWindow width={14} height={14} />}
        />
      </div>
      {invalid !== null && (
        <p role="alert" className="text-xs text-error">
          {t("settings.history.search.invalidPattern")}
        </p>
      )}
    </div>
  );
};

/** `text` with the parts in `ranges` highlighted. A match gets no
 * horizontal padding or shadow: WebKit paints either over the letter before
 * a match that starts mid-word. `box-decoration-clone` rounds each line of a
 * match that wraps. */
export const Highlighted: React.FC<{
  text: string;
  ranges: [number, number][];
}> = ({ text, ranges }) => {
  if (ranges.length === 0) return <>{text}</>;
  return (
    <>
      {splitHighlights(text, ranges).map((part, index) =>
        part.mark ? (
          <mark
            key={index}
            className="bg-search-match text-search-match-text rounded-sm box-decoration-clone"
          >
            {part.text}
          </mark>
        ) : (
          <React.Fragment key={index}>{part.text}</React.Fragment>
        ),
      )}
    </>
  );
};
