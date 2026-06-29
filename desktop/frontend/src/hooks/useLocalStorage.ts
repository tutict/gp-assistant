// LocalStorage-backed state hooks

import { useCallback, useEffect, useState } from "react";

export function useLocalStorage<T>(
  key: string,
  initialValue: T,
  sanitize?: (value: T) => T,
): [T, (value: T | ((prev: T) => T)) => void] {
  const [stored, setStored] = useState<T>(() => {
    try {
      const item = localStorage.getItem(key);
      const parsed = item ? (JSON.parse(item) as T) : initialValue;
      const sanitized = sanitize ? sanitize(parsed) : parsed;
      if (sanitize && item !== JSON.stringify(sanitized)) {
        localStorage.setItem(key, JSON.stringify(sanitized));
      }
      return sanitized;
    } catch {
      return initialValue;
    }
  });

  const setValue = useCallback(
    (value: T | ((prev: T) => T)) => {
      setStored((prev) => {
        const next = typeof value === "function" ? (value as (prev: T) => T)(prev) : value;
        const persisted = sanitize ? sanitize(next) : next;
        try {
          localStorage.setItem(key, JSON.stringify(persisted));
        } catch {
          // ignore
        }
        return next;
      });
    },
    [key, sanitize],
  );

  return [stored, setValue];
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
