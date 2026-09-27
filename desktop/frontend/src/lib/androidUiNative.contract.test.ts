import { describe, expect, it } from "vitest";
const nodeFs = "node:fs";
const { readFileSync } = await import(nodeFs);

const prepareScript = readFileSync(
  new URL("../../../../scripts/prepare-tauri-android-assets.ps1", import.meta.url),
  "utf8",
);

describe("Android native window contract", () => {
  it("keeps edge-to-edge insets owned by CSS rather than padding the WebView twice", () => {
    expect(prepareScript).toContain("WindowCompat.setDecorFitsSystemWindows(window, false)");
    expect(prepareScript).toContain("WindowInsetsControllerCompat");
    expect(prepareScript).not.toContain("view.setPadding(view.paddingLeft, topInset");
    expect(prepareScript).toContain("window.statusBarColor = Color.TRANSPARENT");
    expect(prepareScript).toContain("window.navigationBarColor = Color.TRANSPARENT");
    expect(prepareScript).toContain("webView.addJavascriptInterface(AndroidSystemBarsBridge(this), \"AndroidSystemBars\")");
    expect(prepareScript).toContain("fun setTheme(theme: String)");
    expect(prepareScript).toContain("isAppearanceLightStatusBars = light");
  });

  it("generates Android 12 splash and cutout-safe theme attributes", () => {
    expect(prepareScript).toContain("android:windowLayoutInDisplayCutoutMode");
    expect(prepareScript).toContain("android:windowSplashScreenBackground");
    expect(prepareScript).toContain("android:windowSplashScreenAnimatedIcon");
  });
});




