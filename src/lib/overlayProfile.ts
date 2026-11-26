// DanyWhere: how the overlay shows a profile, as a glyph for the state of
// "choose the profile by application" and the profile's name, and how wide
// each pill that shows one is; also how wide a pill with one label is (where
// the text went, which word list is on). Pure, so `overlayProfile.test.ts`
// can check it.

/** The state of the switch, as `app_profiles::ProfileMode` sends it. */
export type ProfileMode = "none" | "auto" | "lock";

/** A profile as the overlay shows it (`app_profiles::OverlayProfile`). */
export interface OverlayProfile {
  name: string;
  mode: ProfileMode;
}

export type ProfileGlyph = "window" | "lock" | "dot";

/** Width of each native overlay window, mirrored from `overlay.rs`. */
export const OVERLAY_WINDOW = {
  session: 320, // OVERLAY_WIDTH
  notice: 340, // OVERLAY_NOTICE_WIDTH
  live: 400, // OVERLAY_STREAM_WIDTH
};

/** Fixed pill widths, mirrored from `RecordingOverlay.css`. */
export const PILL = {
  rest: 172, // --ov-rest-w
  live: 184, // --ov-pill-w
  work: 216, // --ov-work-w
  open: 392, // --ov-open-w
};

// The rest of the geometry, mirrored from `RecordingOverlay.css`.
const BORDER = 2; // .scard's 1px border on each side
const ROW_PAD = 20; // .sbase: padding 0 10px
const SIDES = 44; // .sbase: two side columns of at least 22px
const WAVE = 68; // .swave: 9 bars of 4px, 8 gaps of 3px, 8px padding
const LABEL_PAD = 16; // .swork-label: padding 0 8px
const SEGMENT = 14; // .ssep: 1px and 7px after it; .sprofile-name: 6px after
const GLYPH = 19; // .sglyph: 14px and 5px after it
const NOTICE_CHROME = 47; // .sprofile-notice: padding 0 13px, 14px glyph, 7px gap

/** The recording pill cuts a longer name than this, to stay a pill. */
export const RECORDING_NAME_MAX = 90;

/**
 * The glyph before a profile's name: a window while choosing by application
 * is on, the padlock while it is off. Without assignments the switch does
 * nothing, so a notice keeps the dot and a dictation's pill shows none.
 */
export function profileGlyph(
  mode: ProfileMode,
  where: "notice" | "dictation",
): ProfileGlyph | null {
  if (mode === "auto") return "window";
  if (mode === "lock") return "lock";
  return where === "notice" ? "dot" : null;
}

/** A pill's width (CSS content box) and how wide its name may get. */
export interface PillLayout {
  width: number;
  nameMax: number;
}

/** `chrome` plus the name, which is cut at `nameMax`, at least `min` wide. */
function layout(
  chrome: number,
  nameWidth: number,
  nameMax: number,
  min: number,
): PillLayout {
  const max = Math.max(0, nameMax);
  return {
    width: Math.max(min, chrome + Math.min(nameWidth, max)),
    nameMax: max,
  };
}

const segment = (glyph: boolean) => SEGMENT + (glyph ? GLYPH : 0);

/** The notice after a switch: glyph and name, cut at the window's edge. */
export function profileNoticeLayout(nameWidth: number): PillLayout {
  const content = OVERLAY_WINDOW.notice - BORDER;
  return layout(NOTICE_CHROME, nameWidth, content - NOTICE_CHROME, 0);
}

/**
 * The recording pill: dot, waveform, separator, glyph and name, cancel. The
 * Live pill keeps its own room for the timer and grows by the profile.
 */
export function recordingLayout(
  nameWidth: number,
  glyph: boolean,
  live = false,
): PillLayout {
  return recordingPill(nameWidth, { named: true, glyph, context: false }, live);
}

/**
 * The working pill: spinner, label, separator, glyph and name, cancel; the
 * name is cut only at the window's edge (in Live, the open panel's).
 */
export function workingLayout(
  labelWidth: number,
  nameWidth: number,
  glyph: boolean,
  live = false,
): PillLayout {
  return workingPill(
    labelWidth,
    nameWidth,
    { named: true, glyph, context: false },
    live,
  );
}

// DanyWhere: "Send Context" (`send_context.rs`). A dictation that sends
// earlier dictations as context shows a speech bubble after its profile's
// name, or, with a single profile, alone after the separator.

/** The bubble's state, as a glyph. */
export type ContextGlyph = "context" | "context-off";

/** What a dictation's pill shows after its waveform or working label. */
export interface DictationSegment {
  /** The profile's name (from two profiles on). */
  named: boolean;
  /** The glyph of choosing the profile by application before the name. */
  glyph: boolean;
  /** The speech bubble: this dictation sends context. */
  context: boolean;
}

const SEPARATOR = 8; // .ssep: 1px and 7px after it
const CONTEXT = 20; // .sctx: 14px and 6px after it

/**
 * The bubble: in the notice after turning the switch over its new state
 * (crossed out while off), in a dictation's pill only while it sends context.
 */
export function contextGlyph(
  sends: boolean,
  where: "notice" | "dictation",
): ContextGlyph | null {
  if (sends) return "context";
  return where === "notice" ? "context-off" : null;
}

/** The segment's width without the name, which `layout` adds. */
const segmentChrome = (s: DictationSegment) =>
  (s.named ? segment(s.glyph) : SEPARATOR) + (s.context ? CONTEXT : 0);

/** The recording pill with `segment` after the waveform. */
export function recordingPill(
  nameWidth: number,
  s: DictationSegment,
  live = false,
): PillLayout {
  const min = live ? PILL.live : PILL.rest;
  const chrome = (live ? PILL.live : ROW_PAD + SIDES + WAVE) + segmentChrome(s);
  const content =
    (live ? OVERLAY_WINDOW.live : OVERLAY_WINDOW.session) - BORDER;
  return layout(
    chrome,
    s.named ? nameWidth : 0,
    Math.min(RECORDING_NAME_MAX, content - chrome),
    min,
  );
}

/**
 * A pill with the dot and one label (`LabelPill.tsx`): where the text went,
 * or which word list is on. As wide as the label, at least the resting
 * pill, and cut only at the edge of the notice window it shows in.
 */
export function labelPill(labelWidth: number): number {
  const chrome = ROW_PAD + SIDES + LABEL_PAD;
  const content = OVERLAY_WINDOW.notice - BORDER;
  return Math.min(content, Math.max(PILL.rest, chrome + labelWidth));
}

/** The working pill with `segment` after the label. */
export function workingPill(
  labelWidth: number,
  nameWidth: number,
  s: DictationSegment,
  live = false,
): PillLayout {
  const chrome = ROW_PAD + SIDES + LABEL_PAD + labelWidth + segmentChrome(s);
  const content = live ? PILL.open : OVERLAY_WINDOW.session - BORDER;
  return layout(chrome, s.named ? nameWidth : 0, content - chrome, PILL.work);
}

// DanyWhere: the spoken instruction's pill, mirrored from `RecordingOverlay.css`.
const INSTRUCTION_GLYPH = 19; // .sinstr-glyph: 14px and 5px after it
const INSTRUCTION_LABEL_GAP = 6; // .sinstr-label: 6px after it
const TEXT_READY = 18; // .sready: 4px before it and 14px

/**
 * DanyWhere: the recording pill while it records a spoken instruction
 * (`RecordingOverlay.tsx`): its glyph and "Instruction" before the
 * waveform, the segment after it, and a check mark once the text is
 * transcribed; the Live pill keeps its own room for the timer.
 */
export function instructionPill(
  labelWidth: number,
  nameWidth: number,
  s: DictationSegment,
  live = false,
): PillLayout {
  const min = live ? PILL.live : PILL.rest;
  const segmented = s.named || s.context;
  const chrome =
    (live ? PILL.live : ROW_PAD + SIDES + WAVE) +
    INSTRUCTION_GLYPH +
    labelWidth +
    INSTRUCTION_LABEL_GAP +
    TEXT_READY +
    (segmented ? segmentChrome(s) : 0);
  const content =
    (live ? OVERLAY_WINDOW.live : OVERLAY_WINDOW.session) - BORDER;
  return layout(
    chrome,
    s.named ? nameWidth : 0,
    Math.min(RECORDING_NAME_MAX, content - chrome),
    min,
  );
}
