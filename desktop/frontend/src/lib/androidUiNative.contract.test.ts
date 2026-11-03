import { describe, expect, it, vi } from "vitest";
const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const prepareScript = readFileSync(
  new URL("../../../../scripts/prepare-tauri-android-assets.ps1", import.meta.url),
  "utf8",
);
const mainScript = readFileSync(new URL("../main.tsx", import.meta.url), "utf8");
const indexHtml = readFileSync(new URL("../../index.html", import.meta.url), "utf8");

describe("Android native window contract", () => {
  it("keeps edge-to-edge insets owned by CSS rather than padding the WebView twice", () => {
    expect(prepareScript).toContain("WindowCompat.setDecorFitsSystemWindows(window, false)");
    expect(prepareScript).toContain("WindowInsetsControllerCompat");
    expect(prepareScript).not.toContain("view.setPadding(view.paddingLeft, topInset");
    expect(prepareScript).toContain("window.statusBarColor = Color.TRANSPARENT");
    expect(prepareScript).toContain("window.navigationBarColor = Color.TRANSPARENT");
    expect(prepareScript).toContain("webView.addJavascriptInterface(systemBarsBridge, \"AndroidSystemBars\")");
    expect(prepareScript).toContain("fun setTheme(theme: String)");
    expect(prepareScript).toContain("isAppearanceLightStatusBars = light");
  });

  it("targets the generated Android application namespace when preparing its activity", () => {
    expect(prepareScript).toContain("namespaceMatch = [regex]::Match(");
    expect(prepareScript).toContain("$androidPackageNamespace.Replace('.',");
    expect(prepareScript).toContain('(\"package $androidPackageNamespace\")');
  });

  it("primes Android safe-area values before React renders the first frame", () => {
    const applyInsetsAt = mainScript.indexOf("applyAndroidSafeAreaInsets(document.documentElement.style");
    const createRootAt = mainScript.indexOf("createRoot(");

    expect(applyInsetsAt).toBeGreaterThanOrEqual(0);
    expect(applyInsetsAt).toBeLessThan(createRootAt);
    expect(prepareScript).toContain("ViewCompat.getRootWindowInsets(webView)");
    expect(prepareScript).toContain("fun getSafeAreaInsets(): String");
  });

  it("applies valid native safe-area dimensions and ignores malformed payloads", async () => {
    const safeArea = await import("./androidSafeArea").catch(() => null);
    expect(safeArea).not.toBeNull();
    if (!safeArea) return;

    const setProperty = vi.fn();
    const style = { setProperty } as Pick<CSSStyleDeclaration, "setProperty">;
    const bridge = { getSafeAreaInsets: () => "{\"top\":28,\"right\":0,\"bottom\":24,\"left\":0}" };

    expect(safeArea.applyAndroidSafeAreaInsets(style, bridge)).toBe(true);
    expect(setProperty).toHaveBeenNthCalledWith(1, "--safe-top", "28px");
    expect(setProperty).toHaveBeenNthCalledWith(2, "--safe-right", "0px");
    expect(setProperty).toHaveBeenNthCalledWith(3, "--safe-bottom", "24px");
    expect(setProperty).toHaveBeenNthCalledWith(4, "--safe-left", "0px");

    setProperty.mockClear();
    expect(safeArea.applyAndroidSafeAreaInsets(style, { getSafeAreaInsets: () => "not-json" })).toBe(false);
    expect(setProperty).not.toHaveBeenCalled();
  });

  it("keeps early inset delivery safe before the document root exists", () => {
    expect(prepareScript).toContain("if(!root||!root.style)return");
    expect(indexHtml).toContain("boot-splash-content");
    expect(indexHtml).not.toMatch(/<div\s+style=/i);
  });

  it("generates Android 12 splash and cutout-safe theme attributes", () => {
    expect(prepareScript).toContain("android:windowLayoutInDisplayCutoutMode");
    expect(prepareScript).toContain("android:windowSplashScreenBackground");
    expect(prepareScript).toContain("android:windowSplashScreenAnimatedIcon");
  });
});




