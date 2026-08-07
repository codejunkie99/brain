// Boots Monaco against the bundled `monaco-editor` package and points
// its web workers at the Vite-bundled worker entry points. Calling
// this once at module load means @monaco-editor/react resolves
// `monaco` synchronously without ever touching the CDN — which is
// what we want inside a Tauri webview (no internet at runtime).

import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";

// Vite-special imports: each `?worker` URL turns into a bundled
// worker chunk we can instantiate at runtime. Only register the
// workers for languages we actually want first-class IntelliSense
// on; everything else falls through to the generic editor worker.
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

// Monaco looks for `MonacoEnvironment` on the host (window) before
// spawning a worker. Defining it here is enough.
declare global {
  interface Window {
    MonacoEnvironment?: monaco.Environment;
  }
}

window.MonacoEnvironment = {
  getWorker(_workerId, label) {
    switch (label) {
      case "json":
        return new jsonWorker();
      case "css":
      case "scss":
      case "less":
        return new cssWorker();
      case "html":
      case "handlebars":
      case "razor":
        return new htmlWorker();
      case "typescript":
      case "javascript":
        return new tsWorker();
      default:
        return new editorWorker();
    }
  },
};

// Register a dark theme aligned with the rest of the IDE. Monaco's
// built-in "vs-dark" is close but uses different accent hues.
monaco.editor.defineTheme("brain-dark", {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: {
    "editor.background": "#0b0d13",
    "editor.foreground": "#e7ecf3",
    "editorLineNumber.foreground": "#3b4256",
    "editorLineNumber.activeForeground": "#94a0b3",
    "editorCursor.foreground": "#6d5dfc",
    "editor.selectionBackground": "#3a338088",
    "editor.lineHighlightBackground": "#11141c",
    "editorIndentGuide.background1": "#1c2230",
    "editorWidget.background": "#171b25",
    "editorWidget.border": "#272e3e",
    "scrollbarSlider.background": "#3b425688",
    "scrollbarSlider.hoverBackground": "#3b4256bb",
    "scrollbarSlider.activeBackground": "#3b4256ee",
  },
});

loader.config({ monaco });

export const monacoReady = loader.init();
