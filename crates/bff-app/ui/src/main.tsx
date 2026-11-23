import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./tokens.css";
import "./styles.css";
import "./brand.css";
import "./screens.css";
import { applyScheme, loadScheme } from "./scheme";
import { applyTheme, loadTheme } from "./theme";

applyScheme(loadScheme(), false);
applyTheme(loadTheme(), false);

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
