import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles/global.css";

// Hide boot splash after React mounts
function hideBootSplash() {
  const splash = document.getElementById("bootSplash");
  if (splash) {
    splash.classList.add("hidden");
    setTimeout(() => splash.remove(), 500);
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App onMounted={hideBootSplash} />
  </StrictMode>,
);
