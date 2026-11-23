// DanyWhere: a failed post-processing's notice names its reason (user,
// 2026-11-23): the dictation's, a file's and a History entry's. The reason
// comes as the notice's detail (src-tauri/src/post_process_failure.rs) and
// shows as a sentence after the explanation, never as the detail itself; a
// notice without one says what it said before, and a raw error still shows.
import { describe, expect, test } from "bun:test";
import { act } from "@testing-library/react";
import type { Notice, NoticeCode, NoticeSource } from "@/bindings";
import { useNoticeStore } from "@/stores/noticeStore";
import { switchTestLanguage } from "@/test/i18n";
import { renderPage } from "@/test/render";
import { rawTextIn, visibleText } from "@/test/visible";
import { NoticePanel } from "./NoticePanel";

const MODEL_NOT_FOUND = JSON.stringify({
  reason: "model-not-found",
  providerId: "custom",
  provider: "Custom",
  model: "gemma4-12b-qat",
});

const notice = (
  code: NoticeCode,
  source: NoticeSource,
  detail: string | null,
): Notice => ({ code, level: "warning", source, detail });

/** The panel with `shown` popped up as a toast. */
const showToast = async (shown: Notice) => {
  await renderPage(<NoticePanel />);
  await act(async () => {
    useNoticeStore.setState({ notices: [shown], toasts: [shown.source] });
  });
};

describe("a failed post-processing's notice", () => {
  test("names why: the provider doesn't have the model", async () => {
    await showToast(notice("PostProcessFailed", "dictation", MODEL_NOT_FOUND));

    const text = visibleText();
    expect(text).toContain("Pasted Without Post-Processing");
    expect(text).toContain(
      "Post Process didn't finish, so the transcript was pasted as is. Custom doesn't have the model “gemma4-12b-qat”.",
    );
    // The detail itself never shows.
    expect(text).not.toContain("model-not-found");
    expect(text).not.toContain("{");
    expect(rawTextIn()).toEqual([]);
  });

  test("of a file and of a History entry names it too", async () => {
    await showToast(notice("FilePostProcessFailed", "file", MODEL_NOT_FOUND));
    expect(visibleText()).toContain(
      "Post Process didn't finish, so the transcript was saved as is. Custom doesn't have the model “gemma4-12b-qat”.",
    );
    expect(visibleText()).not.toContain("model-not-found");

    await act(async () => {
      const shown = notice("HistoryPostProcessFailed", "file", MODEL_NOT_FOUND);
      useNoticeStore.setState({ notices: [shown], toasts: [shown.source] });
    });
    expect(visibleText()).toContain(
      "Post Process didn't finish, so the entry stays as it was. Custom doesn't have the model “gemma4-12b-qat”.",
    );
    expect(visibleText()).not.toContain("model-not-found");
    expect(rawTextIn()).toEqual([]);
  });

  test("says it in German", async () => {
    await switchTestLanguage("de");
    await showToast(notice("PostProcessFailed", "dictation", MODEL_NOT_FOUND));
    expect(visibleText()).toContain(
      "Eigener Anbieter kennt das Modell „gemma4-12b-qat“ nicht.",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("without a reason says what it said before", async () => {
    await showToast(notice("PostProcessFailed", "dictation", null));
    expect(visibleText()).toContain(
      "Post Process didn't finish, so the transcript was pasted as is.",
    );
    expect(rawTextIn()).toEqual([]);
  });

  test("with a reason this app doesn't know shows no code", async () => {
    await showToast(
      notice(
        "PostProcessFailed",
        "dictation",
        JSON.stringify({ reason: "quota-exceeded", provider: "OpenAI" }),
      ),
    );
    const text = visibleText();
    expect(text).toContain(
      "Post Process didn't finish, so the transcript was pasted as is.",
    );
    expect(text).not.toContain("quota-exceeded");
    expect(text).not.toContain("{");
  });

  test("of a History job keeps showing its raw error", async () => {
    await showToast(
      notice(
        "HistoryPostProcessFailed",
        "file",
        "The entry has no transcript to post-process",
      ),
    );
    const text = visibleText();
    expect(text).toContain(
      "Post Process didn't finish, so the entry stays as it was.",
    );
    expect(text).toContain("The entry has no transcript to post-process");
  });
});
