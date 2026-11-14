// DanyWhere: the fixed frame around every post-processing prompt. The rule
// is the one in `src-tauri/src/prompt_frame.rs` (`has_old_parts`): a prompt
// with `${output}` or `<transcript>` tags still carries what the frame now
// says. The word "transcript" alone is fine.
export const hasOldParts = (prompt: string): boolean => {
  const lower = prompt.toLowerCase();
  return (
    prompt.includes("${output}") ||
    lower.includes("<transcript>") ||
    lower.includes("</transcript>")
  );
};
