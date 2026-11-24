// A click on a figure opens its image as large as the window allows, in a
// dialog that Escape, the close button or a click anywhere closes. Without
// this script the link opens the SVG file itself.

let dialog: HTMLDialogElement | undefined;

function zoomDialog(): HTMLDialogElement {
  if (dialog) return dialog;
  dialog = document.createElement("dialog");
  dialog.className = "dw-zoom";
  dialog.innerHTML =
    '<button type="button" class="dw-zoom-close"><span aria-hidden="true">×</span></button><img alt="">';
  dialog.addEventListener("click", () => dialog?.close());
  dialog.addEventListener("close", () => {
    document.documentElement.classList.remove("dw-zoom-open");
  });
  document.body.append(dialog);
  return dialog;
}

document.addEventListener("click", (event) => {
  const link = (event.target as Element | null)?.closest?.<HTMLAnchorElement>(
    "a[data-dw-zoom]",
  );
  if (
    !link ||
    event.defaultPrevented ||
    event.button !== 0 ||
    event.metaKey ||
    event.ctrlKey ||
    event.shiftKey ||
    event.altKey
  ) {
    return;
  }
  const source = link.querySelector("img");
  const box = zoomDialog();
  if (!source || typeof box.showModal !== "function") return;
  event.preventDefault();

  const image = box.querySelector("img")!;
  image.src = link.href;
  image.alt = source.alt;
  const close = box.querySelector<HTMLButtonElement>(".dw-zoom-close")!;
  close.setAttribute("aria-label", link.dataset.dwCloseLabel ?? "Close");
  box.setAttribute("aria-label", source.alt);
  document.documentElement.classList.add("dw-zoom-open");
  box.showModal();
  close.focus();
});
