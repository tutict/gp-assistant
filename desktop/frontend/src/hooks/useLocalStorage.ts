// LocalStorage-backed state hooks

import { useCallback, useEffect, useState } from "react";

export function useLocalStorage<T>(
  key: string,
  initialValue: T,
  sanitize?: (value: T) => T,
): [T, (value: T | ((prev: T) => T)) => void, boolean] {
  const [stored, setStored] = useState<T>(() => {
    try {
      const item = localStorage.getItem(key);
      const parsed = item ? (JSON.parse(item) as T) : initialValue;
      return sanitize ? sanitize(parsed) : parsed;
    } catch {
      return initialValue;
    }
  });
  // True when the last persistence attempt could not be written (e.g. QuotaExceededError).
  // Surfaced so callers can warn the user instead of silently dropping state on reload.
  const [quotaError, setQuotaError] = useState(false);

  // Persist on every change. Sanitize is applied to the persisted copy only, so the
  // in-memory state can keep rich/ephemeral fields (e.g. agent result payloads) while
  // localStorage stores a lighter transcript. `sanitize` must be a stable reference
  // (module-level function) to avoid re-writing on every render.
  useEffect(() => {
    try {
      const persisted = sanitize ? sanitize(stored) : stored;
      localStorage.setItem(key, JSON.stringify(persisted));
      setQuotaError(false);
    } catch {
      setQuotaError(true);
    }
  }, [key, stored, sanitize]);

  const setValue = useCallback((value: T | ((prev: T) => T)) => {
    setStored(value);
  }, []);

  return [stored, setValue, quotaError];
}

export function useLocalStorageString(key: string, initialValue: string): [string, (value: string) => void] {
  const [stored, setStored] = useState<string>(() => {
    try {
      return localStorage.getItem(key) ?? initialValue;
    } catch {
      return initialValue;
    }
  });

  const setValue = useCallback(
    (value: string) => {
      setStored(value);
      try {
        localStorage.setItem(key, value);
      } catch {
        // ignore
      }
    },
    [key],
  );

  return [stored, setValue];
}
