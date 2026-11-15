import type { OSType } from "./utils/keyboard";

/**
 * DanyWhere: the text key of the paste method's description for `osType`.
 * macOS offers only the clipboard (Cmd+V) and None, and the clipboard needs
 * Accessibility, so it gets a text of its own; every other system keeps
 * Handy's key, whose text explains the options offered there.
 */
export const pasteMethodDescriptionKey = (osType: OSType): string =>
  osType === "macos"
    ? "settings.advanced.pasteMethod.descriptionMacos"
    : "settings.advanced.pasteMethod.description";
