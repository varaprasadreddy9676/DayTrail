const proofItems = [
  {
    title: "Recover context fast",
    body: "Resume after meetings, messages, or tab switches with the last app, project, prompt, and file in view.",
  },
  {
    title: "Replay the day",
    body: "Scan a local-first timeline by hour, app, project, and AI session without starting timers.",
  },
  {
    title: "Draft weekly updates",
    body: "Turn captured sessions into AI-assisted weekly notes you can review before sharing.",
  },
];

export function ProofBand() {
  return (
    <section className="proof-band" id="proof" aria-labelledby="proof-title">
      <div>
        <p className="section-kicker">Built for interrupted work</p>
        <h2 id="proof-title">A clean work memory for getting back on task.</h2>
      </div>
      <div className="proof-items">
        {proofItems.map((item) => (
          <article key={item.title}>
            <strong>{item.title}</strong>
            <p>{item.body}</p>
          </article>
        ))}
      </div>
    </section>
  );
}
