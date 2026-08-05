import { useCallback, useEffect, useState } from "react";

export type Density = "comfortable" | "compact";

const DENSITY_KEY = "stock-optimizer-density";

function readDensity(): Density {
  if (typeof window === "undefined") return "comfortable";
  const requested = new URLSearchParams(window.location.search).get("density");
  if (requested === "compact" || requested === "comfortable") return requested;
  return localStorage.getItem(DENSITY_KEY) === "compact" ? "compact" : "comfortable";
}

export function useDensity(): {
  density: Density;
  setDensity: (density: Density) => void;
  toggleDensity: () => void;
} {
  const [density, setDensityState] = useState<Density>(readDensity);

  const applyDensity = useCallback((next: Density) => {
    document.documentElement.dataset.density = next;
    localStorage.setItem(DENSITY_KEY, next);
  }, []);

  useEffect(() => {
    applyDensity(density);
  }, [applyDensity, density]);

  const setDensity = useCallback((next: Density) => setDensityState(next), []);
  const toggleDensity = useCallback(() => {
    setDensityState((previous) => (previous === "comfortable" ? "compact" : "comfortable"));
  }, []);

  return { density, setDensity, toggleDensity };
}
