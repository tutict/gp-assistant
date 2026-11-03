export interface AndroidSystemBarsBridge {
  getSafeAreaInsets?: () => string;
}

const SAFE_AREA_EDGES = ["top", "right", "bottom", "left"] as const;

export function applyAndroidSafeAreaInsets(
  style: Pick<CSSStyleDeclaration, "setProperty">,
  bridge?: AndroidSystemBarsBridge,
): boolean {
  if (typeof bridge?.getSafeAreaInsets !== "function") return false;

  try {
    const parsed: unknown = JSON.parse(bridge.getSafeAreaInsets());
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return false;
    const insets = parsed as Record<(typeof SAFE_AREA_EDGES)[number], unknown>;
    const values = SAFE_AREA_EDGES.map((edge) => insets[edge]);
    if (values.some((value) => typeof value !== "number" || !Number.isFinite(value) || value < 0)) return false;

    SAFE_AREA_EDGES.forEach((edge, index) => {
      style.setProperty(`--safe-${edge}`, `${values[index]}px`);
    });
    return true;
  } catch {
    return false;
  }
}
