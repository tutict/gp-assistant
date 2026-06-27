// LocalStorage-backed state hooks

import { useCallback, useEffect, useState } from "react";

export function useLocalStorage<T>(key: string, initialValue: T): [T, (value: T | ((prev: T) => T)) => void] {
  const [stored, setStored] = useState<T>(() => {
    try {
      const item = localStorage.getItem(key);
      return item ? (JSON.parse(item) as T) : initialValue;
    } catch {
      return initialValue;
    }
  });

  const setValue = useCallback(
    (value: T | ((prev: T) => T)) => {
      setStored((prev) => {
        const next = typeof value === "function" ? (value as (prev: T) => T)(prev) : value;
        try {
          localStorage.setItem(key, JSON.stringify(next));
        } catch {
          // ignore
        }
        return next;
      });
    },
    [key],
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
