// Standalone assert check (no JS unit-test runner in this repo). Run with:
//   bun src/lib/userTheme.test.ts
//
// The theme file's CSS has to come after every built-in sheet, or a rule of
// equal specificity in the built-in CSS would win over the user's. A reload
// replaces the previous sheet instead of stacking another one.
import assert from "node:assert/strict";
import {
  USER_THEME_ID,
  applyUserTheme as applyToDocument,
  keepUserThemeLast as keepLastInDocument,
} from "./userTheme";

let checks = 0;
let failures = 0;
const check = (name: string, run: () => void) => {
  checks += 1;
  try {
    run();
  } catch (error) {
    failures += 1;
    console.error(`FAILED: ${name}\n  ${(error as Error).message}`);
  }
};

/** Just enough of a DOM: a `<head>` whose children keep their order. */
class FakeNode {
  id = "";
  textContent: string | null = null;
  constructor(
    readonly tag: string,
    private readonly owner: FakeDocument,
  ) {}
  remove() {
    const list = this.owner.headChildren;
    const at = list.indexOf(this);
    if (at !== -1) list.splice(at, 1);
  }
}

class FakeDocument {
  headChildren: FakeNode[] = [];
  readonly head: {
    lastElementChild: FakeNode | null;
    appendChild(node: FakeNode): FakeNode;
  };
  constructor() {
    const children = this.headChildren;
    this.head = {
      get lastElementChild() {
        return children[children.length - 1] ?? null;
      },
      appendChild(node: FakeNode) {
        node.remove();
        children.push(node);
        return node;
      },
    };
  }
  getElementById(id: string) {
    return this.headChildren.find((n) => n.id === id) ?? null;
  }
  createElement(tag: string) {
    return new FakeNode(tag, this);
  }
  /** A built-in sheet, as Vite adds one. */
  addBuiltInSheet(name: string) {
    const node = new FakeNode("link", this);
    node.id = name;
    this.head.appendChild(node);
  }
  ids() {
    return this.headChildren.map((n) => n.id);
  }
  last() {
    return this.ids()[this.headChildren.length - 1];
  }
  userSheets() {
    return this.headChildren.filter((n) => n.id === USER_THEME_ID);
  }
}

const applyUserTheme = (doc: FakeDocument, css: string | null) =>
  applyToDocument(doc as unknown as Document, css);
const keepUserThemeLast = (doc: FakeDocument) =>
  keepLastInDocument(doc as unknown as Document);

const withBuiltIns = () => {
  const doc = new FakeDocument();
  doc.addBuiltInSheet("main.css");
  doc.addBuiltInSheet("wordmark.css");
  return doc;
};

check("the theme file's sheet comes after the built-in sheets", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, ":root { --light-color-text: red; }");
  assert.deepEqual(doc.ids(), ["main.css", "wordmark.css", USER_THEME_ID]);
  assert.equal(
    doc.userSheets()[0].textContent,
    ":root { --light-color-text: red; }",
  );
  assert.equal(doc.userSheets()[0].tag, "style");
});

check("a reload replaces the previous sheet instead of adding one", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, "a { color: red; }");
  applyUserTheme(doc, "a { color: blue; }");
  assert.equal(doc.userSheets().length, 1);
  assert.equal(doc.userSheets()[0].textContent, "a { color: blue; }");
  assert.equal(doc.last(), USER_THEME_ID);
});

check("a reload moves the sheet behind a sheet added since", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, "a { color: red; }");
  doc.addBuiltInSheet("lazy-chunk.css");
  applyUserTheme(doc, "a { color: blue; }");
  assert.deepEqual(doc.ids(), [
    "main.css",
    "wordmark.css",
    "lazy-chunk.css",
    USER_THEME_ID,
  ]);
});

check("keeping it last moves it behind a sheet added later", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, "a { color: red; }");
  doc.addBuiltInSheet("lazy-chunk.css");
  assert.equal(keepUserThemeLast(doc), true);
  assert.equal(doc.last(), USER_THEME_ID);
  assert.equal(doc.userSheets().length, 1);
});

check("keeping it last leaves it alone when it already is", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, "a { color: red; }");
  assert.equal(keepUserThemeLast(doc), false);
});

check("no file leaves the built-in sheets alone", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, "a { color: red; }");
  applyUserTheme(doc, null);
  assert.deepEqual(doc.ids(), ["main.css", "wordmark.css"]);
  assert.equal(keepUserThemeLast(doc), false);
});

check("no file at the start adds nothing", () => {
  const doc = withBuiltIns();
  applyUserTheme(doc, null);
  assert.deepEqual(doc.ids(), ["main.css", "wordmark.css"]);
});

if (failures > 0) {
  console.error(`userTheme: ${failures} of ${checks} checks failed`);
  process.exit(1);
}
console.log(`userTheme: all ${checks} checks passed`);
