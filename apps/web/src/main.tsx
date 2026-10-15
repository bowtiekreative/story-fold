import React from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { Catalyst } from "./Catalyst.tsx";
import "./styles.css";
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {location.pathname.startsWith("/catalyst") ? <Catalyst /> : <App />}
  </React.StrictMode>,
);
