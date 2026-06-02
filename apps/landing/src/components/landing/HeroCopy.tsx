import { links } from "./landingData";

export function HeroCopy() {
  return (
    <div className="hero-copy">
      <p className="eyebrow motion-copy">
        <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M7 11V8a5 5 0 0 1 10 0v3"
            stroke="currentColor"
            strokeLinecap="round"
            strokeWidth="2"
          />
          <rect height="10" rx="3" stroke="currentColor" strokeWidth="2" width="14" x="5" y="11" />
        </svg>
        <span>ADHD-friendly work memory for interruption-heavy days</span>
      </p>

      <h1 className="motion-copy" id="hero-title">
        Replay your day,
        <br />
        <span>recover your context.</span>
      </h1>

      <p className="hero-subhead motion-copy">
        DayTrail builds a local-first timeline from your apps, windows, projects, and AI activity
        so you can get back on task, retrace decisions, and draft weekly updates without managing
        timers.
      </p>

      <div className="hero-actions motion-cta" id="download">
        <a className="button primary" href={links.releases}>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              d="M12 4v10m0 0 4-4m-4 4-4-4M5 20h14"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
            />
          </svg>
          Download for Mac
        </a>
        <a className="button platform" href={links.releases}>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              d="M4 5h16v14H4V5Zm0 7h16M12 5v14"
              stroke="currentColor"
              strokeLinejoin="round"
              strokeWidth="2"
            />
          </svg>
          Download for Windows
        </a>
        <a className="button quiet" href={links.demo}>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="2" />
            <path d="m10 8 6 4-6 4V8Z" fill="currentColor" />
          </svg>
          Watch the trail
        </a>
      </div>

      <div className="trust-pills motion-cta" aria-label="Trust highlights">
        <span>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              d="M12 3 5 6v6c0 4.2 2.7 7 7 9 4.3-2 7-4.8 7-9V6l-7-3Z"
              stroke="currentColor"
              strokeLinejoin="round"
              strokeWidth="2"
            />
          </svg>
          Local-first memory
        </span>
        <span>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              d="m4 4 16 16M9 9a3 3 0 0 1 4 4M7.5 7.5 5 10s2.5 5 7 5c1.2 0 2.3-.3 3.2-.8M11 5h1c4.5 0 7 5 7 5a12 12 0 0 1-1.8 2.4"
              stroke="currentColor"
              strokeLinecap="round"
              strokeWidth="2"
            />
          </svg>
          No screenshots by default
        </span>
        <span>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path d="M12 3v18M5 8h14M7 16h10" stroke="currentColor" strokeLinecap="round" strokeWidth="2" />
          </svg>
          Focus recovery
        </span>
        <span>
          <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <path
              d="M7 10V8a5 5 0 0 1 10 0v2M6 10h12v10H6V10Z"
              stroke="currentColor"
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth="2"
            />
            <path d="M12 14v2" stroke="currentColor" strokeLinecap="round" strokeWidth="2" />
          </svg>
          AI weekly updates
        </span>
      </div>

      <p className="privacy-line motion-cta" id="privacy">
        <svg viewBox="0 0 24 24" fill="none" aria-hidden="true">
          <path
            d="M12 3 4 7v5c0 4.4 3.1 7.4 8 9 4.9-1.6 8-4.6 8-9V7l-8-4Z"
            stroke="currentColor"
            strokeLinejoin="round"
            strokeWidth="2"
          />
        </svg>
        <span>
          Metadata-first. No screenshots. No clipboard capture. Bring your own AI key for optional
          summaries and weekly updates with OpenAI-compatible, Anthropic, Gemini, or local models.
          Keys stay in your OS keychain.
        </span>
      </p>
    </div>
  );
}
