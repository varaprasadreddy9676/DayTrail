import { ReactNode } from "react";

export type IconName =
  | "apps"
  | "archive"
  | "arrow"
  | "bell"
  | "chat"
  | "check"
  | "copy"
  | "layout"
  | "panelLeft"
  | "plus"
  | "return"
  | "ritual"
  | "save"
  | "search"
  | "sliders"
  | "sync"
  | "warning"
  | "x"
  | "zap";

export function Icon({ name }: { name: IconName }) {
  const pathByName: Record<IconName, ReactNode> = {
    bell: (
      <>
        <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
        <path d="M13.73 21a2 2 0 0 1-3.46 0" />
      </>
    ),
    chat: (
      <>
        <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      </>
    ),
    apps: (
      <>
        <rect height="6" rx="1.5" width="6" x="4" y="4" />
        <rect height="6" rx="1.5" width="6" x="14" y="4" />
        <rect height="6" rx="1.5" width="6" x="4" y="14" />
        <rect height="6" rx="1.5" width="6" x="14" y="14" />
      </>
    ),
    archive: (
      <>
        <path d="M4 7h16v13H4z" />
        <path d="M3 4h18v3H3zM9 11h6" />
      </>
    ),
    arrow: <path d="M5 12h13M13 6l6 6-6 6" />,
    check: <path d="m5 12 4 4L19 6" />,
    copy: (
      <>
        <rect height="12" rx="2" width="12" x="8" y="8" />
        <path d="M5 15H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v1" />
      </>
    ),
    layout: (
      <>
        <rect height="14" rx="2" width="16" x="4" y="5" />
        <path d="M9 5v14M4 10h16" />
      </>
    ),
    panelLeft: (
      <>
        <rect height="16" rx="2" width="16" x="3" y="4" />
        <path d="M9 4v16" />
      </>
    ),
    plus: <path d="M12 5v14M5 12h14" />,
    return: <path d="M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3" />,
    ritual: (
      <>
        <path d="M12 3v5M12 16v5M5.6 5.6l3.5 3.5M14.9 14.9l3.5 3.5M3 12h5M16 12h5M5.6 18.4l3.5-3.5M14.9 9.1l3.5-3.5" />
        <circle cx="12" cy="12" r="3" />
      </>
    ),
    save: (
      <>
        <path d="M5 3h12l2 2v16H5z" />
        <path d="M8 3v6h8V3M8 21v-7h8v7" />
      </>
    ),
    search: (
      <>
        <circle cx="11" cy="11" r="7" />
        <path d="m16.5 16.5 4 4" />
      </>
    ),
    sliders: (
      <>
        <path d="M4 7h16M4 17h16" />
        <circle cx="9" cy="7" r="2" />
        <circle cx="15" cy="17" r="2" />
      </>
    ),
    sync: (
      <>
        <path d="M20 11a8 8 0 0 0-14.8-4M4 5v5h5" />
        <path d="M4 13a8 8 0 0 0 14.8 4M20 19v-5h-5" />
      </>
    ),
    warning: (
      <>
        <path d="m12 3 10 18H2z" />
        <path d="M12 9v5M12 17h.01" />
      </>
    ),
    x: <path d="M6 6l12 12M18 6 6 18" />,
    zap: <path d="M13 2L3 14h9l-1 8 10-12h-9l1-8z" />,
  };

  return (
    <svg
      aria-hidden="true"
      className="icon"
      fill="none"
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth="1.75"
      viewBox="0 0 24 24"
    >
      {pathByName[name]}
    </svg>
  );
}
