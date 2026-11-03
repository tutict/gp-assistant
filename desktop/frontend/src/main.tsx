import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { WorkspaceProvider } from "./hooks/useWorkspace";
import { WorkspaceErrorBoundary } from "./components/ui/ErrorBoundary";
import { applyAndroidSafeAreaInsets, type AndroidSystemBarsBridge } from "./lib/androidSafeArea";
import "./styles/global.css";

const androidSystemBars = (window as Window & { AndroidSystemBars?: AndroidSystemBarsBridge }).AndroidSystemBars;
applyAndroidSafeAreaInsets(document.documentElement.style, androidSystemBars);

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
    <WorkspaceProvider>
      <WorkspaceErrorBoundary label="应用">
        <App onMounted={hideBootSplash} />
      </WorkspaceErrorBoundary>
    </WorkspaceProvider>
  </StrictMode>,
);
