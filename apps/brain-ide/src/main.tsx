import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./App";
import "@xterm/xterm/css/xterm.css";
import "reactflow/dist/style.css";
import "./styles/global.css";

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
