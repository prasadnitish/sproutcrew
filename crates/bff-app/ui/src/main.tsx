import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./tokens.css";
import "./styles.css";
import "./brand.css";
import "./screens.css";
import { applyPalette, loadPalette } from "./palette";
import { applyTheme, loadTheme } from "./theme";

applyPalette(loadPalette(), false);
applyTheme(loadTheme(), false);

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
