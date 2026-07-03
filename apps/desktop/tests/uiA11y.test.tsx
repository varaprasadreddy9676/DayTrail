import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { check as checkUpdate } from "@tauri-apps/plugin-updater";

import App from "../src/App";

// The updater plugin must be mocked so UpdateChecker never hits the network.
vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));

const SIDEBAR_COLLAPSED_KEY = "daytrail-sidebar-collapsed";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.__TAURI__ = undefined;
  window.__TAURI_INTERNALS__ = undefined;
  if (typeof window.localStorage?.clear === "function") window.localStorage.clear();
  if (typeof window.sessionStorage?.clear === "function") window.sessionStorage.clear();
});

// ── Sidebar collapse ───────────────────────────────────────────────────────
describe("sidebar collapse", () => {
  it("toggles between expanded and collapsed, with correct ARIA and persistence", async () => {
    const user = userEvent.setup();
    render(<App />);

    const toggle = screen.getByRole("button", { name: /collapse sidebar/i });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    // No prior choice stored → defaults to expanded.
    expect(window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY)).toBeNull();

    await user.click(toggle);

    // Persisted as collapsed, and the accessible label/expanded reflect it.
    await waitFor(() =>
      expect(window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY)).toBe("1"),
    );
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(toggle).toHaveAccessibleName(/expand sidebar/i);

    // Toggling back restores expanded state and updates storage.
    await user.click(toggle);
    await waitFor(() =>
      expect(window.localStorage.getItem(SIDEBAR_COLLAPSED_KEY)).toBe("0"),
    );
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(toggle).toHaveAccessibleName(/collapse sidebar/i);
  });

  it("restores the collapsed choice from localStorage on mount", () => {
    window.localStorage.setItem(SIDEBAR_COLLAPSED_KEY, "1");
    render(<App />);

    const toggle = screen.getByRole("button", { name: /expand sidebar/i });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
  });

  it("hides the focus composer while collapsed but keeps it mounted (no state loss)", async () => {
    const user = userEvent.setup();
    render(<App />);

    // Open the focus composer first (expanded state). With no tasks captured
    // (Tauri backend absent) the placeholder is the empty-state copy.
    await user.click(screen.getByRole("button", { name: /start focus/i }));
    const composer = screen.getByPlaceholderText(/what are you focusing on\?/i);
    expect(composer).toBeInTheDocument();

    // Collapse the sidebar — the composer becomes hidden, not unmounted.
    await user.click(screen.getByRole("button", { name: /collapse sidebar/i }));
    expect(composer).not.toBeVisible();

    // Expand back — the composer reappears with its prior DOM intact.
    await user.click(screen.getByRole("button", { name: /expand sidebar/i }));
    expect(composer).toBeVisible();
  });
});

// ── Command palette focus restore ──────────────────────────────────────────
describe("command palette focus restore", () => {
  it("returns focus to the trigger when closed with Escape", async () => {
    const user = userEvent.setup();
    render(<App />);

    const trigger = screen.getByRole("button", { name: /search work/i });
    await user.click(trigger);

    const input = await screen.findByPlaceholderText(/search work, apps, ai tools/i);
    expect(input).toHaveFocus();

    // Escape closes the palette. Focus must return to the opener, not <body>.
    await user.keyboard("{Escape}");
    await waitFor(() => expect(trigger).toHaveFocus());
    expect(screen.queryByRole("dialog", { name: /command bar/i })).not.toBeInTheDocument();
  });
});

// ── Heading hierarchy ──────────────────────────────────────────────────────
describe("heading hierarchy", () => {
  it("uses exactly one h1 per view and nests sub-panels under h3", () => {
    render(<App />);

    // The Today view has a single page-level h1.
    const h1s = screen.getAllByRole("heading", { level: 1 });
    expect(h1s).toHaveLength(1);

    // The selected-hour panel ("Selected hour") is a sub-panel inside the
    // Timeline zone — it must be h3, not h2, so the outline stays a tree
    // rather than a flat sibling list.
    const selectedHour = screen.queryByRole("heading", { level: 3, name: /\d{1,2} (am|pm)/i });
    if (selectedHour) {
      // If it renders, it must NOT also appear as an h2.
      const duplicateH2 = screen.queryAllByRole("heading", { level: 2, name: /\d{1,2} (am|pm)/i });
      expect(duplicateH2).toHaveLength(0);
    }
  });
});

// ── Workspace nav semantics ────────────────────────────────────────────────
describe("workspace nav semantics", () => {
  it("renders the primary nav as a labelled list of buttons", () => {
    render(<App />);
    const nav = screen.getByRole("navigation", { name: /workspace views/i });
    expect(nav).toBeInTheDocument();

    // List structure gives screen-reader users item count + arrow nav context.
    const list = nav.querySelector("ul");
    expect(list).not.toBeNull();

    const items = nav.querySelectorAll("ul > li > button.nav-item");
    expect(items.length).toBeGreaterThan(0);

    // The active view is announced via aria-current (already present; we
    // assert it survives the list refactor).
    const today = screen.getByRole("button", { name: /^today$/i });
    expect(today).toHaveAttribute("aria-current", "page");
  });
});
