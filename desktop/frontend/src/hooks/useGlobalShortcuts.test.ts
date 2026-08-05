import { describe, expect, it } from "vitest";
import { resolveGlobalShortcut } from "./useGlobalShortcuts";

const event = (overrides: Partial<Parameters<typeof resolveGlobalShortcut>[0]> = {}) => ({
  key: "",
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  target: null,
  ...overrides,
});

describe("global shortcuts", () => {
  it("maps Ctrl+K and slash to search without hijacking editable fields", () => {
    expect(resolveGlobalShortcut(event({ key: "k", ctrlKey: true }))).toEqual({ type: "focus-search" });
    expect(resolveGlobalShortcut(event({ key: "/" }))).toEqual({ type: "focus-search" });
    expect(resolveGlobalShortcut(event({ key: "/", target: { tagName: "INPUT" } }))).toBeNull();
  });

  it("maps number keys to views and ignores editable targets", () => {
    expect(resolveGlobalShortcut(event({ key: "1" }))).toEqual({ type: "navigate", view: "screen" });
    expect(resolveGlobalShortcut(event({ key: "5" }))).toEqual({ type: "navigate", view: "agent" });
    expect(resolveGlobalShortcut(event({ key: "1", target: { tagName: "TEXTAREA" } }))).toBeNull();
  });

  it("opens help on question mark and closes overlays on Escape", () => {
    expect(resolveGlobalShortcut(event({ key: "?" }))).toEqual({ type: "toggle-help" });
    expect(resolveGlobalShortcut(event({ key: "Escape" }))).toEqual({ type: "escape" });
  });
});
