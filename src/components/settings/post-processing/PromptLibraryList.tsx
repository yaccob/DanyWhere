// DanyWhere: the prompt library as an editable list (idea 1, E2). Each
// prompt shows its name and part of its text in a box that scrolls and opens
// for editing; "Active" decides whether it runs, and the active ones carry
// their number (1, 2, 3 …), the order they run in. Entries move by dragging
// their handle (pointer events: Tauri's file drop, on by default, turns
// HTML5 drag and drop off on Windows) or with ↑/↓ while the handle has the
// focus.
//
// A controlled component: it shows `prompts` in `order` and reports a new
// order through `onOrderChange`, so it can edit any profile's order (or the
// defaults'); the library itself changes through the callbacks.
import React, { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { GripVertical } from "lucide-react";
import type { LLMPrompt, PromptOrderEntry } from "@/bindings";
import { Textarea } from "@/components/ui";
import { Alert } from "../../ui/Alert";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { PromptFrameText, PromptFrameTip } from "./PromptFrame";
import { cancelOnEscape, followPointer } from "@/lib/dragCancel";
import {
  dropIndex,
  isNameTaken,
  moved,
  movedBy,
  normalizedOrder,
  promptRows,
  runsAlone,
  withActive,
} from "@/lib/promptLibrary";

export interface PromptLibraryListProps {
  /** The library, shared by all profiles. */
  prompts: LLMPrompt[];
  /** Which prompts are active, and the order of the whole list. */
  order: PromptOrderEntry[];
  onOrderChange: (order: PromptOrderEntry[]) => void;
  /** Saves a prompt's name and text; whether it was saved. */
  onSave: (id: string, name: string, text: string) => Promise<boolean>;
  /** Adds a prompt at the end; whether it was added. */
  onCreate: (name: string, text: string) => Promise<boolean>;
  /** Deletes a prompt, after asking where it is active. */
  onDelete: (prompt: LLMPrompt) => Promise<void>;
}

/** The prompt being edited: one of the list, or a new one. */
type Editing = { id: string } | { id: null } | null;

interface Drag {
  from: number;
  to: number;
  /** The rows' vertical middles when the drag began. */
  midpoints: number[];
  /** The whole order when the drag began, which the drop rearranges. */
  order: PromptOrderEntry[];
}

export const PromptLibraryList: React.FC<PromptLibraryListProps> = ({
  prompts,
  order,
  onOrderChange,
  onSave,
  onCreate,
  onDelete,
}) => {
  const { t } = useTranslation();
  const [editing, setEditing] = useState<Editing>(null);
  const [draftName, setDraftName] = useState("");
  const [draftText, setDraftText] = useState("");
  const [busy, setBusy] = useState(false);
  const [drag, setDragState] = useState<Drag | null>(null);
  // The window's listeners read the drag here, so a move and a release in
  // the same frame never see a stale one.
  const dragRef = useRef<Drag | null>(null);
  const setDrag = useCallback((next: Drag | null) => {
    dragRef.current = next;
    setDragState(next);
  }, []);
  const rowRefs = useRef<(HTMLLIElement | null)[]>([]);
  const dragging = drag !== null;

  // While a prompt is dragged, the window hears the pointer and Escape. The
  // rows show the new order meanwhile, and dragged down, the dragged row's
  // own node moves, which drops the handle's pointer capture, so a handler
  // on the handle never got the release (qa 2026-11-22_0233, point 10).
  // Escape ends the drag and leaves the order as it was: the pressed handle
  // has no keyboard focus in WebKit.
  useEffect(() => {
    if (!dragging) return;
    const stopEscape = cancelOnEscape(window, () => setDrag(null));
    const stopPointer = followPointer(window, {
      onMove: (clientY) => {
        const current = dragRef.current;
        if (!current) return;
        const to = dropIndex(clientY, current.midpoints, current.from);
        if (to !== current.to) setDrag({ ...current, to });
      },
      onEnd: () => {
        const current = dragRef.current;
        setDrag(null);
        if (current && current.from !== current.to) {
          onOrderChange(moved(current.order, current.from, current.to));
        }
      },
      onCancel: () => setDrag(null),
    });
    return () => {
      stopEscape();
      stopPointer();
    };
  }, [dragging, setDrag, onOrderChange]);

  const fullOrder = normalizedOrder(prompts, order);
  const shownOrder = drag ? moved(fullOrder, drag.from, drag.to) : fullOrder;
  const rows = promptRows(prompts, shownOrder);
  const draggedId = drag ? fullOrder[drag.from]?.id : null;
  const anyActive = rows.some((row) => row.active);

  const startEdit = (prompt: LLMPrompt | null) => {
    setEditing(prompt ? { id: prompt.id } : { id: null });
    setDraftName(prompt?.name ?? "");
    setDraftText(prompt?.prompt ?? "");
  };
  const stopEdit = () => setEditing(null);

  const editedPrompt =
    editing?.id != null
      ? (prompts.find((prompt) => prompt.id === editing.id) ?? null)
      : null;
  const nameTaken = isNameTaken(prompts, draftName, editing?.id ?? undefined);
  const canSave =
    !busy &&
    draftName.trim() !== "" &&
    draftText.trim() !== "" &&
    !nameTaken &&
    (editedPrompt === null ||
      draftName.trim() !== editedPrompt.name ||
      draftText.trim() !== editedPrompt.prompt.trim());

  const save = async () => {
    if (!canSave || !editing) return;
    setBusy(true);
    try {
      const saved =
        editing.id === null
          ? await onCreate(draftName.trim(), draftText.trim())
          : await onSave(editing.id, draftName.trim(), draftText.trim());
      if (saved) stopEdit();
    } finally {
      setBusy(false);
    }
  };

  const remove = async (prompt: LLMPrompt) => {
    setBusy(true);
    try {
      await onDelete(prompt);
    } finally {
      setBusy(false);
    }
  };

  // Dragging starts on the handle; the window follows it from there.
  const beginDrag = (
    event: React.PointerEvent<HTMLButtonElement>,
    index: number,
  ) => {
    if (event.button !== 0 || editing) return;
    event.preventDefault();
    // While the capture holds, a release outside the window still comes; it
    // bubbles up to the window's listener like every other.
    event.currentTarget.setPointerCapture(event.pointerId);
    const midpoints = fullOrder.map((_, i) => {
      const rect = rowRefs.current[i]?.getBoundingClientRect();
      return rect ? rect.top + rect.height / 2 : 0;
    });
    setDrag({ from: index, to: index, midpoints, order: fullOrder });
  };

  const moveWithKeys = (
    event: React.KeyboardEvent<HTMLButtonElement>,
    id: string,
  ) => {
    const delta =
      event.key === "ArrowUp" ? -1 : event.key === "ArrowDown" ? 1 : 0;
    if (delta === 0) return;
    event.preventDefault();
    const next = movedBy(fullOrder, id, delta);
    if (next !== fullOrder) onOrderChange(next);
  };

  const editor = (prompt: LLMPrompt | null) => (
    <div className="space-y-3">
      <div className="space-y-2 flex flex-col">
        <label className="text-sm font-semibold">
          {t("settings.postProcessing.prompts.promptLabel")}
        </label>
        <Input
          type="text"
          value={draftName}
          onChange={(e) => setDraftName(e.target.value)}
          placeholder={t(
            "settings.postProcessing.prompts.promptLabelPlaceholder",
          )}
          variant="compact"
        />
        {nameTaken && (
          <Alert variant="warning">
            {t("settings.postProcessing.prompts.library.nameTaken", {
              name: draftName.trim(),
            })}
          </Alert>
        )}
      </div>
      <div className="space-y-2 flex flex-col">
        <label className="text-sm font-semibold">
          {t("settings.postProcessing.prompts.promptInstructions")}
        </label>
        <PromptFrameText />
        <Textarea
          value={draftText}
          onChange={(e) => setDraftText(e.target.value)}
          placeholder={t("settings.postProcessing.prompts.frame.placeholder")}
        />
        <PromptFrameTip prompt={draftText} />
      </div>
      <div className="flex gap-2 pt-1">
        <Button onClick={save} variant="primary" size="md" disabled={!canSave}>
          {prompt
            ? t("settings.postProcessing.prompts.updatePrompt")
            : t("settings.postProcessing.prompts.createPrompt")}
        </Button>
        <Button onClick={stopEdit} variant="secondary" size="md">
          {t("settings.postProcessing.prompts.cancel")}
        </Button>
        {prompt && (
          <Button
            onClick={() => remove(prompt)}
            variant="danger-ghost"
            size="md"
            disabled={busy || prompts.length <= 1}
            className="ms-auto"
          >
            {t("settings.postProcessing.prompts.deletePrompt")}
          </Button>
        )}
      </div>
    </div>
  );

  return (
    <div className="space-y-3">
      {prompts.length === 0 && (
        <p className="text-sm text-ink-3">
          {t("settings.postProcessing.prompts.library.empty")}
        </p>
      )}
      {prompts.length > 0 && !anyActive && (
        <Alert variant="warning">
          {t("settings.postProcessing.prompts.library.noActive")}
        </Alert>
      )}
      <ul className="space-y-2">
        {rows.map((row, index) => {
          const { prompt } = row;
          const isEditing = editing?.id === prompt.id;
          return (
            <li
              key={prompt.id}
              ref={(element) => {
                rowRefs.current[index] = element;
              }}
              className={`rounded-md border border-line p-2 space-y-2 ${
                draggedId === prompt.id ? "opacity-60 bg-wash" : ""
              }`}
            >
              <div className="flex items-center gap-2 min-w-0">
                <button
                  type="button"
                  className="touch-none cursor-grab active:cursor-grabbing rounded p-1 text-ink-3 hover:text-text focus:outline-none focus-visible:ring-1 focus-visible:ring-logo-primary"
                  aria-label={t(
                    "settings.postProcessing.prompts.library.move",
                    {
                      name: prompt.name,
                    },
                  )}
                  title={t("settings.postProcessing.prompts.library.move", {
                    name: prompt.name,
                  })}
                  // Not while dragging, so `index` is the row's place in
                  // `fullOrder`.
                  onPointerDown={(event) => beginDrag(event, index)}
                  onKeyDown={(event) => moveWithKeys(event, prompt.id)}
                >
                  <GripVertical className="h-4 w-4" />
                </button>
                <span
                  className="w-5 shrink-0 text-end text-sm font-semibold tabular-nums"
                  aria-hidden={row.number === null}
                >
                  {row.number ?? ""}
                </span>
                <label className="flex shrink-0 items-center gap-1 text-xs text-ink-3">
                  <input
                    type="checkbox"
                    checked={row.active}
                    onChange={(event) =>
                      onOrderChange(
                        withActive(fullOrder, prompt.id, event.target.checked),
                      )
                    }
                    aria-label={t(
                      "settings.postProcessing.prompts.library.activeFor",
                      { name: prompt.name },
                    )}
                  />
                  {t("settings.postProcessing.prompts.library.active")}
                </label>
                <span className="truncate text-sm font-semibold">
                  {prompt.name}
                </span>
              </div>
              {isEditing ? (
                editor(prompt)
              ) : (
                <button
                  type="button"
                  onClick={() => startEdit(prompt)}
                  disabled={busy}
                  aria-label={t(
                    "settings.postProcessing.prompts.library.edit",
                    {
                      name: prompt.name,
                    },
                  )}
                  className="block w-full max-h-24 overflow-y-auto rounded border border-line bg-wash p-2 text-start text-xs text-ink-3 whitespace-pre-wrap cursor-text"
                >
                  {prompt.prompt}
                </button>
              )}
              {runsAlone(prompts, fullOrder, prompt.id) && (
                <Alert variant="warning">
                  {t("settings.postProcessing.prompts.library.runsAlone")}
                </Alert>
              )}
            </li>
          );
        })}
      </ul>
      {editing?.id === null ? (
        <div className="rounded-md border border-line p-2">{editor(null)}</div>
      ) : (
        <Button
          onClick={() => startEdit(null)}
          variant="primary"
          size="md"
          disabled={busy}
        >
          {t("settings.postProcessing.prompts.createNew")}
        </Button>
      )}
    </div>
  );
};
