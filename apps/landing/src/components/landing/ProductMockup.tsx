import { assets } from "./landingData";

const appRows = [
  { name: "VS Code", duration: "34m", icon: assets.apps.vscode },
  { name: "ChatGPT", duration: "8m", icon: assets.apps.chatgpt },
  { name: "Terminal", duration: "5m", icon: assets.apps.terminal },
];

export function ProductMockup() {
  return (
    <div className="mockup-shell" role="img" aria-label="DayTrail mockup showing today, a 24-hour timeline, context recovery, app breakdown, and a weekly update draft">
      <div className="mockup-titlebar">
        <span className="traffic" aria-hidden="true">
          <i />
          <i />
          <i />
        </span>
        <strong>DayTrail</strong>
        <span className="mockup-tools" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none">
            <path d="M21 21 16.6 16.6M18 10.5a7.5 7.5 0 1 1-15 0 7.5 7.5 0 0 1 15 0Z" stroke="currentColor" strokeLinecap="round" strokeWidth="2" />
          </svg>
          <svg viewBox="0 0 24 24" fill="none">
            <path d="M5 7h14M5 12h14M5 17h14" stroke="currentColor" strokeLinecap="round" strokeWidth="2" />
          </svg>
        </span>
      </div>

      <div className="mockup-body">
        <div className="mockup-heading">
          <div>
            <span>Today</span>
            <strong>7h 42m captured</strong>
          </div>
          <p>Context ready</p>
        </div>

        <div className="summary-row">
          <span>Context replay</span>
          <span>3 AI sessions</span>
          <span>Weekly update</span>
        </div>

        <div className="timeline-panel">
          <div className="timeline-title">
            <span>Replay today</span>
            <span>9 AM - 10 AM selected</span>
          </div>
          <span className="time-badge">9:41 AM</span>
          <span className="time-beam" />
          <div className="timeline-track">
            <span className="timeline-segment segment-one" />
            <span className="timeline-segment segment-two" />
            <span className="timeline-segment segment-three" />
            <span className="timeline-segment segment-four" />
            <span className="timeline-segment segment-five" />
            <span className="timeline-segment segment-six" />
          </div>
          <div className="time-axis">
            <span>12 AM</span>
            <span>4 AM</span>
            <span>8 AM</span>
            <span>12 PM</span>
            <span>4 PM</span>
            <span>8 PM</span>
            <span>12 AM</span>
          </div>

          <div className="selected-hour">
            <div className="hour-card">
              <h3>9 AM - 10 AM</h3>
              <p>Focus block · resume point</p>
              {appRows.map((app) => (
                <div className="app-row" key={app.name}>
                  <span>
                    <img src={app.icon} alt="" loading="lazy" />
                    {app.name}
                  </span>
                  <span>{app.duration}</span>
                </div>
              ))}
            </div>

            <div className="report-card">
              <strong>Weekly update draft</strong>
              <span>VS Code context recovered</span>
              <span>ChatGPT research linked</span>
              <span>Next steps ready</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
