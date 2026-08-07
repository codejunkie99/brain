// Wrapper around an xterm.js instance bound to a Tauri PTY session.

import { useEffect, useRef } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";

import * as bridge from "@/ipc/bridge";
import { subscribePtyData, subscribePtyExit } from "@/ipc/events";
import type { PtySessionInfo } from "@/ipc/types";

interface Props {
  session: PtySessionInfo;
  onExit?: (status: number | null) => void;
}

export function XTerm({ session, onExit }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!ref.current) return;

    const term = new Terminal({
      fontFamily: "ui-monospace, SF Mono, Menlo, monospace",
      fontSize: 12,
      lineHeight: 1.3,
      theme: {
        background: "#0b0d13",
        foreground: "#e7ecf3",
        cursor: "#6d5dfc",
        selectionBackground: "#3a3380",
        black: "#11141c",
        red: "#ef5e6e",
        green: "#3cd28b",
        yellow: "#f5c451",
        blue: "#4ea3ff",
        magenta: "#a780ff",
        cyan: "#5ad1cd",
        white: "#e7ecf3",
        brightBlack: "#272e3e",
        brightRed: "#ef5e6e",
        brightGreen: "#3cd28b",
        brightYellow: "#f5c451",
        brightBlue: "#4ea3ff",
        brightMagenta: "#a780ff",
        brightCyan: "#5ad1cd",
        brightWhite: "#ffffff",
      },
      cursorBlink: true,
      scrollback: 5000,
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon());
    term.open(ref.current);
    fit.fit();

    termRef.current = term;
    fitRef.current = fit;

    // PTY -> terminal
    const unsubData = subscribePtyData(session.id, ({ b64 }) => {
      const bytes = atob(b64);
      term.write(bytes);
    });
    const unsubExit = subscribePtyExit(session.id, ({ status }) => {
      term.write(`\r\n\x1b[2m[exited with ${status ?? "unknown"}]\x1b[0m\r\n`);
      onExit?.(status);
    });

    // Terminal -> PTY
    term.onData((data) => {
      void bridge.ptyWrite(session.id, data);
    });

    // Resize handling — both the terminal needs to recalc its viewport
    // and we need to notify the backend so the PTY's TIOCSWINSZ fires.
    const onResize = () => {
      try {
        fit.fit();
        const { cols, rows } = term;
        void bridge.ptyResize(session.id, cols, rows);
      } catch {
        // ignore resize errors during teardown
      }
    };
    const ro = new ResizeObserver(onResize);
    ro.observe(ref.current);

    let unsubDataFn: (() => void) | null = null;
    let unsubExitFn: (() => void) | null = null;
    void unsubData.then((fn) => (unsubDataFn = fn));
    void unsubExit.then((fn) => (unsubExitFn = fn));

    return () => {
      ro.disconnect();
      unsubDataFn?.();
      unsubExitFn?.();
      term.dispose();
      termRef.current = null;
    };
  }, [session.id, onExit]);

  return (
    <div
      ref={ref}
      style={{
        flex: 1,
        minHeight: 0,
        minWidth: 0,
        background: "#0b0d13",
        borderRadius: 6,
        overflow: "hidden",
      }}
    />
  );
}
