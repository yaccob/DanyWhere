import { listen } from "@tauri-apps/api/event";
import React, { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import "./RecordingOverlay.css";
import { commands, events } from "@/bindings";
import type {
  Notice,
  StreamPhase,
  StreamPhaseEvent,
  StreamTextEvent,
  StreamWorkKind,
} from "@/bindings";
import { ContextGlyphIcon } from "@/components/icons/ContextGlyphIcon";
import { NoticeBadgeIcon } from "@/components/icons/NoticeBadgeIcon";
import { ProfileGlyphIcon } from "@/components/icons/ProfileGlyphIcon";
import i18n, { syncLanguageFromSettings } from "@/i18n";
import { noticeDisplay } from "@/lib/notices";
import {
  contextGlyph,
  profileGlyph,
  profileNoticeLayout,
  recordingPill,
  workingPill,
} from "@/lib/overlayProfile";
import type {
  DictationSegment,
  OverlayProfile,
  ProfileGlyph,
} from "@/lib/overlayProfile";
import type { ModelStateEvent } from "@/lib/types/events";
import { getLanguageDirection } from "@/lib/utils/rtl";
import { ReadyNotice } from "./ReadyNotice";

type OverlayState =
  | "recording"
  | "streaming"
  | "transcribing"
  | "processing"
  | "notice"
  | "live_notice"
  | "clipboard"
  | "ready"
  | "not_ready"
  | "word_list"
  | "profile"
  | "context";

const isNoticeState = (state: OverlayState) =>
  state === "notice" || state === "live_notice";

// Number of reactive bars in the waveform (the simple, smoothed style shared by
// every overlay form). Mic levels arrive as 16 FFT buckets; we take the first N.
const WAVE_BARS = 9;

// Only call out a model load in the Live preview once it has run this long.
// Warm loads finish in well under this (~0.2s on Apple Silicon, ~1.5s on a
// Windows CPU backend), so the common case never flashes a loading notice while
// the user is speaking; cold loads can take 40s+.
const SLOW_MODEL_LOAD_MS = 2000;

// Matches the delay in `hide_recording_overlay`, so a notice can fade out.
const OVERLAY_FADE_MS = 300;

// 13px padding on each side, the 16px badge and its 7px gap.
const NOTICE_PILL_CHROME = 49;

const RecordingOverlay: React.FC = () => {
  const { t } = useTranslation();
  const [isVisible, setIsVisible] = useState(false);
  const [state, setState] = useState<OverlayState>("recording");
  // `Stream::play()` returning does not mean hardware callbacks are flowing.
  // Stay visually in an arming state until the backend processes the first
  // actual microphone sample chunk.
  const [captureReady, setCaptureReady] = useState(false);
  // Recording starts while the model loads in the background. Say so, otherwise
  // a cold start looks like an empty Live preview that ignores speech, then a
  // hung "Transcribing..." spinner. Once recording stops, any in-flight load is
  // the wait, so the working label reflects it immediately; the Live notice
  // waits for the load to be slow so fast loads don't flash it mid-speech.
  const [modelLoading, setModelLoading] = useState(false);
  const [modelLoadSlow, setModelLoadSlow] = useState(false);
  const modelLoadTimerRef = useRef<ReturnType<typeof setTimeout>>();
  // Latched per Live session: once the loading notice has opened the panel, keep
  // it open after the load so it doesn't collapse and reopen when text arrives.
  const [loadNoticeShown, setLoadNoticeShown] = useState(false);
  const [levels, setLevels] = useState<number[]>(Array(WAVE_BARS).fill(0));
  const [streamText, setStreamText] = useState<StreamTextEvent>({
    committed: "",
    tentative: "",
  });
  const [phase, setPhase] = useState<StreamPhase>("listening");
  const [workKind, setWorkKind] = useState<StreamWorkKind>("transcribing");
  const [elapsed, setElapsed] = useState(0);
  // Bumped on each new streaming session so the Live card remounts fresh (replays
  // the pop-in, and never animates in from the previous panel's open size).
  const [session, setSession] = useState(0);
  // Overlay placement (top vs bottom of the screen). The Live panel grows downward
  // from a top overlay (oldest line under the pill) and upward from a bottom one.
  const [position, setPosition] = useState<"top" | "bottom">("bottom");
  // True once live text overflows the cap. A top overlay fades its top edge only
  // while overflowing, so the resting first line stays crisp flush under the pill.
  const [overflowing, setOverflowing] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  // The additional word list the switch shortcut just turned on; null: only
  // the global list.
  const [wordListName, setWordListName] = useState<string | null>(null);
  // DanyWhere: the profile a switch (sidebar, tray, or the shortcut of
  // choosing the profile by application) names, with that switch's state.
  const [noticeProfile, setNoticeProfile] = useState<OverlayProfile | null>(
    null,
  );
  // DanyWhere: the profile this dictation runs with, shown next to its
  // waveform and working label; null: only one profile, nothing to show.
  const [dictationProfile, setDictationProfile] =
    useState<OverlayProfile | null>(null);
  // DanyWhere: "Send Context" (`send_context.rs`): the profile and new state
  // its shortcut or tray item names, and whether this dictation sends
  // context, shown as a speech bubble after its profile.
  const [noticeContext, setNoticeContext] = useState<{
    name: string;
    sends: boolean;
  } | null>(null);
  const [dictationContext, setDictationContext] = useState(false);
  // DanyWhere: measured widths of the profile's name and the working label,
  // since a pill that shows a profile is as wide as its content.
  const [profileNameWidth, setProfileNameWidth] = useState(0);
  const [workLabelWidth, setWorkLabelWidth] = useState(0);
  const profileProbeRef = useRef<HTMLSpanElement>(null);
  const labelProbeRef = useRef<HTMLSpanElement>(null);
  // The "Insert Last Dictation" service's shortcut, e.g. ⌃⇧V; null: none.
  const [insertKeys, setInsertKeys] = useState<string | null>(null);
  const [noticeWidth, setNoticeWidth] = useState<number>();
  const noticeProbeRef = useRef<HTMLSpanElement>(null);
  const fadeTimerRef = useRef<ReturnType<typeof setTimeout>>();

  const smoothedLevelsRef = useRef<number[]>(Array(16).fill(0));
  // Live-text scroll-back: the text region "sticks" to the newest line while the
  // user is at the bottom; if they scroll up to read history, auto-follow pauses
  // until they scroll back down.
  const capRef = useRef<HTMLDivElement>(null);
  const pinnedRef = useRef(true);
  const direction = getLanguageDirection(i18n.language);

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlistenNotice = await listen<Notice>("overlay-notice", (event) => {
        clearTimeout(fadeTimerRef.current);
        setNotice(event.payload);
      });

      const unlistenWordList = await listen<{ name: string | null }>(
        "overlay-word-list",
        (event) => setWordListName(event.payload.name),
      );

      const unlistenProfile = await listen<OverlayProfile>(
        "overlay-profile",
        (event) => setNoticeProfile(event.payload),
      );

      const unlistenDictationProfile = await listen<OverlayProfile | null>(
        "overlay-dictation-profile",
        (event) => setDictationProfile(event.payload),
      );

      const unlistenContext = await listen<{ name: string; sends: boolean }>(
        "overlay-context",
        (event) => setNoticeContext(event.payload),
      );

      const unlistenDictationContext = await listen<boolean>(
        "overlay-dictation-context",
        (event) => setDictationContext(event.payload),
      );

      const unlistenInsertKeys = await listen<{ keys: string | null }>(
        "overlay-ready",
        (event) => setInsertKeys(event.payload.keys),
      );

      const unlistenShow = await listen("show-overlay", async (event) => {
        const overlayState = event.payload as OverlayState;
        clearTimeout(fadeTimerRef.current);
        if (!isNoticeState(overlayState)) setNotice(null);
        // Reset synchronously before settings I/O. A fast microphone can emit
        // recording-ready while the awaits below are in flight; resetting after
        // them would overwrite that event and leave the overlay stuck arming.
        if (overlayState === "recording" || overlayState === "streaming") {
          setCaptureReady(false);
          smoothedLevelsRef.current = Array(16).fill(0);
          setLevels(Array(WAVE_BARS).fill(0));
          setStreamText({ committed: "", tentative: "" });
        }

        await syncLanguageFromSettings();
        // The Live panel flows downward from a top overlay and upward from a
        // bottom one; read the placement so the layout can flip to match.
        try {
          const settings = await commands.getAppSettings();
          if (settings.status === "ok") {
            setPosition(
              settings.data.overlay_position === "top" ? "top" : "bottom",
            );
          }
        } catch {
          // Keep the previous/default placement if settings can't be read.
        }
        setState(overlayState);
        if (overlayState === "streaming") {
          setPhase("listening");
          setWorkKind("transcribing");
          setElapsed(0);
          setLoadNoticeShown(false);
          setSession((s) => s + 1); // remount the card fresh for this session
        }
        setIsVisible(true);
      });

      const unlistenHide = await listen("hide-overlay", () => {
        setIsVisible(false);
        setCaptureReady(false);
        // A notice keeps rendering while it fades. Dropping the transcript here
        // stops a later notice from showing an old session's text.
        clearTimeout(fadeTimerRef.current);
        fadeTimerRef.current = setTimeout(() => {
          setNotice(null);
          setStreamText({ committed: "", tentative: "" });
        }, OVERLAY_FADE_MS);
      });

      const unlistenReady = await listen("recording-ready", () => {
        setElapsed(0);
        setCaptureReady(true);
      });

      const unlistenLevel = await listen<number[]>("mic-level", (event) => {
        const newLevels = event.payload as number[];
        // Exponential smoothing across the 16 buckets, then take the first N
        // bars for the shared waveform.
        const smoothed = smoothedLevelsRef.current.map((prev, i) => {
          const target = newLevels[i] || 0;
          return prev * 0.7 + target * 0.3;
        });
        smoothedLevelsRef.current = smoothed;
        setLevels(smoothed.slice(0, WAVE_BARS));
      });

      const unlistenStream = await events.streamTextEvent.listen((event) => {
        setStreamText(event.payload);
      });

      const unlistenPhase = await events.streamPhaseEvent.listen((event) => {
        const payload: StreamPhaseEvent = event.payload;
        setPhase(payload.phase);
        if (payload.kind) setWorkKind(payload.kind);
      });

      // The backend ends every `loading_started` with exactly one of completed
      // or failed, so only those end a load. Other events (e.g. `unloaded` from
      // the idle watcher) aren't ordered against an in-flight load and must not
      // clear it.
      const unlistenModel = await listen<ModelStateEvent>(
        "model-state-changed",
        (event) => {
          const type = event.payload.event_type;
          if (type === "loading_started") {
            clearTimeout(modelLoadTimerRef.current);
            setModelLoading(true);
            setModelLoadSlow(false);
            modelLoadTimerRef.current = setTimeout(
              () => setModelLoadSlow(true),
              SLOW_MODEL_LOAD_MS,
            );
          } else if (
            type === "loading_completed" ||
            type === "loading_failed"
          ) {
            clearTimeout(modelLoadTimerRef.current);
            setModelLoading(false);
            setModelLoadSlow(false);
          }
        },
      );

      return () => {
        unlistenNotice();
        unlistenWordList();
        unlistenProfile();
        unlistenDictationProfile();
        unlistenContext();
        unlistenDictationContext();
        unlistenInsertKeys();
        unlistenShow();
        unlistenHide();
        unlistenReady();
        unlistenLevel();
        unlistenStream();
        unlistenPhase();
        unlistenModel();
        clearTimeout(modelLoadTimerRef.current);
        clearTimeout(fadeTimerRef.current);
      };
    };

    setupEventListeners();
  }, []);

  // Elapsed capture timer starts only once microphone samples are flowing.
  useEffect(() => {
    if (state !== "streaming" || !isVisible || !captureReady) return;
    const id = setInterval(() => setElapsed((e) => e + 1), 1000);
    return () => clearInterval(id);
  }, [state, isVisible, captureReady]);

  // Stick to the bottom as text streams in — but only while pinned, so a user who
  // has scrolled up to read history isn't yanked back down by the next chunk.
  useLayoutEffect(() => {
    const el = capRef.current;
    if (!el) return;
    // Fade the top edge only once text actually overflows the cap.
    setOverflowing(el.scrollHeight > el.clientHeight + 1);
    if (pinnedRef.current) el.scrollTop = el.scrollHeight;
  }, [streamText]);

  // `session` re-runs this when a new Live session starts mid-load (e.g. a
  // retry during a cold load), since the reset doesn't change the other deps.
  useEffect(() => {
    if (modelLoadSlow && state === "streaming") setLoadNoticeShown(true);
  }, [modelLoadSlow, state, session]);

  // Each fresh streaming session starts pinned to the bottom, fade cleared.
  useEffect(() => {
    pinnedRef.current = true;
    setOverflowing(false);
  }, [session]);

  const shownNotice = notice && isNoticeState(state) ? notice : null;
  const noticeTitle = shownNotice
    ? t(noticeDisplay(shownNotice.code).title)
    : "";

  // CSS can't animate to an intrinsic width, so a notice pill gets a measured one.
  useLayoutEffect(() => {
    const probe = noticeProbeRef.current;
    if (!probe) return;
    setNoticeWidth(
      Math.ceil(probe.getBoundingClientRect().width) + NOTICE_PILL_CHROME,
    );
  }, [noticeTitle, isVisible]);

  const transcribingLabel = modelLoading
    ? t("overlay.loadingModel")
    : t("overlay.transcribing");
  const workLabel = (
    state === "streaming" ? workKind === "polishing" : state === "processing"
  )
    ? t("overlay.processing")
    : transcribingLabel;
  // DanyWhere: the profile name a pill shows: the switch's in its notice
  // (choosing by application, or Send Context), the dictation's in a session.
  const probedName =
    state === "profile"
      ? noticeProfile?.name
      : state === "context"
        ? noticeContext?.name
        : dictationProfile?.name;
  // A dictation's pill shows a segment after its waveform or label for its
  // profile (from two profiles on) or the bubble of Send Context.
  const dictationContextGlyph = contextGlyph(dictationContext, "dictation");
  const segmentShown =
    dictationProfile !== null || dictationContextGlyph !== null;

  // Like the notice: a pill that shows a profile gets a measured width.
  useLayoutEffect(() => {
    const name = profileProbeRef.current;
    if (name)
      setProfileNameWidth(Math.ceil(name.getBoundingClientRect().width));
    const label = labelProbeRef.current;
    if (label)
      setWorkLabelWidth(Math.ceil(label.getBoundingClientRect().width));
  }, [probedName, segmentShown, workLabel, isVisible, state]);

  if (!isVisible && !shownNotice) return null;

  // Re-pin when the user is within ~a line of the bottom; unpin otherwise.
  const handleStreamScroll = () => {
    const el = capRef.current;
    if (!el) return;
    pinnedRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 16;
  };

  const fmtTime = (s: number) =>
    `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;

  // ---- Shared building blocks (one visual language for every overlay form) ----
  const waveform = (
    <div className={`swave ${captureReady ? "ready" : "arming"}`}>
      {levels.map((v, i) => (
        <i
          key={i}
          style={{
            height: `${Math.max(3, Math.min(18, 3 + Math.pow(v, 0.7) * 15))}px`,
          }}
        />
      ))}
    </div>
  );

  const cancelBtn = (
    <button
      className="sx"
      aria-label="cancel"
      onClick={() => commands.cancelOperation()}
    >
      <svg viewBox="0 0 16 16" aria-hidden="true">
        <path
          d="M4 4 L12 12 M12 4 L4 12"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
        />
      </svg>
    </button>
  );

  // DanyWhere: the glyph of choosing the profile by application.
  const glyphElement = (glyph: ProfileGlyph | null) =>
    glyph === "dot" ? (
      <span className="sglyph dot">
        <span className="sdot ready" />
      </span>
    ) : glyph ? (
      <ProfileGlyphIcon glyph={glyph} className={`sglyph ${glyph}`} />
    ) : null;
  const dictationGlyph = dictationProfile
    ? profileGlyph(dictationProfile.mode, "dictation")
    : null;
  // What the pill's segment holds, for its width (`overlayProfile.ts`).
  const segmentParts: DictationSegment = {
    named: dictationProfile !== null,
    glyph: dictationGlyph !== null,
    context: dictationContextGlyph !== null,
  };
  // DanyWhere: separator, glyph and name of the dictation's profile, then
  // the bubble while it sends context, after the waveform or the working
  // label; `nameMax` cuts a long name.
  const profileSegment = (nameMax: number) =>
    segmentShown && (
      <span className="sprofile">
        <span className="ssep" />
        {dictationProfile && glyphElement(dictationGlyph)}
        {dictationProfile && (
          <span className="sprofile-name" style={{ maxWidth: nameMax }}>
            {dictationProfile.name}
          </span>
        )}
        {dictationContextGlyph && (
          <ContextGlyphIcon glyph={dictationContextGlyph} className="sctx" />
        )}
      </span>
    );
  // The full name and working label, measured for the pill's width.
  const profileProbes = (probedName !== undefined || segmentShown) && (
    <>
      {probedName !== undefined && (
        <span ref={profileProbeRef} className="sprobe" aria-hidden="true">
          {probedName}
        </span>
      )}
      <span ref={labelProbeRef} className="sprobe" aria-hidden="true">
        {workLabel}
      </span>
    </>
  );

  // dot (left) | waveform (center) | timer + cancel (right) — same structure for
  // pill & panel, so the Live morph is a pure width change. DanyWhere: from two
  // profiles on, the dictation's profile follows the waveform.
  const listeningRow = (
    showTimer: boolean,
    showCancel: boolean,
    nameMax: number,
  ) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className={`sdot ${captureReady ? "ready" : "arming"}`} />
      </div>
      {segmentShown ? (
        <div className="scenter">
          {waveform}
          {profileSegment(nameMax)}
        </div>
      ) : (
        waveform
      )}
      <div className="sbase-r">
        {showTimer && <span className="stimer">{fmtTime(elapsed)}</span>}
        {showCancel && cancelBtn}
      </div>
    </div>
  );

  // spinner (left) | label (center) | cancel (right) — same 3-zone grid as the
  // listening row, so the label is centered. DanyWhere: from two profiles on,
  // the dictation's profile follows the label.
  const workingRow = (label: string, showCancel: boolean, nameMax: number) => (
    <div className="sbase">
      <div className="sbase-l">
        <span className="sspinner" />
      </div>
      {segmentShown ? (
        <div className="scenter">
          <span className="swork-label">{label}</span>
          {profileSegment(nameMax)}
        </div>
      ) : (
        <span className="swork-label">{label}</span>
      )}
      <div className="sbase-r">{showCancel && cancelBtn}</div>
    </div>
  );

  const noticeRow = (
    <div className="sbase snotice">
      <NoticeBadgeIcon className="nglyph" />
      <span className="nlabel">{noticeTitle}</span>
    </div>
  );
  const noticeProbe = shownNotice && (
    <span ref={noticeProbeRef} className="nlabel nprobe" aria-hidden="true">
      {noticeTitle}
    </span>
  );
  const noticeClass = shownNotice ? `cnotice level-${shownNotice.level}` : "";
  const dismissNotice = shownNotice
    ? () => commands.dismissOverlayNotice()
    : undefined;

  // ---- Live overlay: a pill that sculpts open into a panel ----
  const liveNotice = state === "live_notice" ? shownNotice : null;
  if (state === "streaming" || liveNotice) {
    const hasText =
      streamText.committed.length > 0 || streamText.tentative.length > 0;
    const working = !liveNotice && phase === "working";
    // A notice keeps the transcript; with none, the panel explains the notice.
    const noticeText =
      liveNotice && !hasText
        ? (noticeDisplay(liveNotice.code).explanation?.(t, liveNotice.detail) ??
          liveNotice.detail)
        : null;
    // While listening, a slow model load opens the panel with a notice where the
    // text would be, so speech that isn't transcribing yet doesn't look ignored.
    const loadingNotice = !liveNotice && modelLoadSlow && !working && !hasText;
    // Keep the panel open whenever there's text — even while finalizing — so the
    // transcript stays put under a working spinner instead of collapsing and
    // squishing the text mid-stream. Only fall back to the small working pill
    // when there was no text to preserve.
    const open = liveNotice
      ? hasText || !!noticeText
      : hasText || loadingNotice || (loadNoticeShown && !working);
    const collapsed = working && !hasText;
    // DanyWhere: the pill grows with the dictation's profile; the open panel
    // keeps its width.
    const liveLayout = working
      ? workingPill(workLabelWidth, profileNameWidth, segmentParts, true)
      : recordingPill(profileNameWidth, segmentParts, true);
    const liveStyle =
      liveNotice && !open
        ? { width: noticeWidth }
        : segmentShown && !liveNotice && !open
          ? { width: liveLayout.width }
          : undefined;

    return (
      <div dir={direction} className={`ov-stage ${position}`}>
        {noticeProbe}
        {profileProbes}
        <div
          key={session}
          className={`scard ${open ? "open" : ""} ${collapsed ? "working" : ""} ${
            isVisible ? "" : "leaving"
          } ${noticeClass}`}
          style={liveStyle}
          onClick={dismissNotice}
        >
          <div className="stext">
            <div className="stext-clip">
              <div
                className={`stext-cap ${overflowing ? "overflowing" : ""}`}
                ref={capRef}
                onScroll={handleStreamScroll}
              >
                <p>
                  {loadingNotice && (
                    <span className="sloading">
                      {t("overlay.loadingModel")}
                    </span>
                  )}
                  {noticeText && <span className="ndetail">{noticeText}</span>}
                  <span className="committed">
                    {streamText.committed ? streamText.committed + " " : ""}
                  </span>
                  <span className="tentative">{streamText.tentative}</span>
                  {/* Drop the blinking caret once finalizing — it's no longer
                      capturing, and a static spinner conveys the work. */}
                  {!working && !loadingNotice && !liveNotice && (
                    <span className="scaret" />
                  )}
                </p>
              </div>
            </div>
          </div>
          {liveNotice
            ? noticeRow
            : working
              ? workingRow(workLabel, true, liveLayout.nameMax)
              : listeningRow(open, true, liveLayout.nameMax)}
        </div>
      </div>
    );
  }

  // ---- Clipboard, ready and not-ready notices (`ReadyNotice.tsx`).
  if (state === "clipboard" || state === "ready" || state === "not_ready") {
    const pasteKeys = navigator.platform.toLowerCase().includes("mac")
      ? "⌘V"
      : "Ctrl+V";
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        <ReadyNotice
          state={state}
          pasteKeys={pasteKeys}
          insertKeys={insertKeys}
          t={t}
        />
      </div>
    );
  }

  // ---- Word list notice: the switch shortcut turned another list on.
  if (state === "word_list") {
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        <div className="scard compact">
          <div className="sbase">
            <div className="sbase-l">
              <span className="sdot ready" />
            </div>
            <span className="swork-label">
              {wordListName
                ? t("overlay.wordList", { name: wordListName })
                : t("overlay.wordListGlobalOnly")}
            </span>
            <div className="sbase-r" />
          </div>
        </div>
      </div>
    );
  }

  // ---- DanyWhere: profile notice, after a switch in the sidebar or the tray
  // or the shortcut of choosing the profile by application: the switch's
  // glyph (the dot without assignments) and the profile's name, as wide as
  // they are. After turning Send Context over by its shortcut or the tray,
  // the same notice with the speech bubble (crossed out while off).
  if (state === "profile" || state === "context") {
    const { width, nameMax } = profileNoticeLayout(profileNameWidth);
    const noticeContextGlyph = contextGlyph(
      noticeContext?.sends ?? false,
      "notice",
    );
    return (
      <div
        dir={direction}
        className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
      >
        {profileProbes}
        <div className="scard compact" style={{ width }}>
          <div className="sbase snotice">
            {state === "context" && noticeContextGlyph ? (
              <ContextGlyphIcon
                glyph={noticeContextGlyph}
                className={`sglyph ${noticeContextGlyph}`}
              />
            ) : (
              glyphElement(
                profileGlyph(noticeProfile?.mode ?? "none", "notice"),
              )
            )}
            <span className="nlabel" style={{ maxWidth: nameMax }}>
              {probedName}
            </span>
          </div>
        </div>
      </div>
    );
  }

  // ---- Minimal overlay: exactly one row at a time — waveform (recording), or a
  // spinner + label (transcribing / processing). Never both. The pill animates its
  // width between them; the cancel button is in both rows so it stays put.
  const working = state === "transcribing" || state === "processing";
  // DanyWhere: the pill grows with the dictation's profile and bubble.
  const compactLayout = working
    ? workingPill(workLabelWidth, profileNameWidth, segmentParts)
    : recordingPill(profileNameWidth, segmentParts);

  return (
    <div
      dir={direction}
      className={`ov-stage ${position} ov-fade ${isVisible ? "show" : ""}`}
    >
      {noticeProbe}
      {profileProbes}
      <div
        className={`scard compact ${working && isVisible ? "cworking" : ""} ${noticeClass}`}
        style={
          shownNotice
            ? { width: noticeWidth }
            : segmentShown
              ? { width: compactLayout.width }
              : undefined
        }
        onClick={dismissNotice}
      >
        {shownNotice
          ? noticeRow
          : working
            ? workingRow(workLabel, true, compactLayout.nameMax)
            : listeningRow(false, true, compactLayout.nameMax)}
      </div>
    </div>
  );
};

export default RecordingOverlay;
