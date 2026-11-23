// DanyWhere: the DOM of the component tests (happy-dom), registered before
// anything else of the preload (`src/test/setup.ts`) is imported, since
// react-dom and testing-library look for `document` when they load.
import { GlobalRegistrator } from "@happy-dom/global-registrator";

if (typeof document === "undefined") {
  GlobalRegistrator.register({
    url: "http://localhost/",
    width: 1024,
    height: 768,
  });
}
