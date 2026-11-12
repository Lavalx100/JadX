import profile from "../resources/engineering-principles/profile.json";
import "./ideIntelligence.css";

export default function EngineeringPrinciplesProfile({ onClose }: { onClose: () => void }) {
  return <div className="modal-backdrop intelligence-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
    <section className="engineering-principles-profile-dialog" role="dialog" aria-modal="true" aria-labelledby="engineering-principles-profile-title">
      <header><div><h2 id="engineering-principles-profile-title">Engineering with intention</h2></div><button autoFocus onClick={onClose} aria-label="Close knowledge profile">×</button></header>
      <div className="engineering-principles-profile-content">
        <p>{profile.disclaimer}</p>
        <h3>Core principles</h3>
        <div className="engineering-principles-list">{profile.principles.map((principle) => <article key={principle.id}><div><strong>{principle.title}</strong></div><p>{principle.guidance}</p></article>)}</div>
        <h3>Practical guidelines</h3>
        <ul>{profile.practicalGuidelines.map((guideline) => <li key={guideline}>{guideline}</li>)}</ul>
        <h3>Optional editor palette</h3>
        <p>{profile.design.adaptation}</p>
        <div className="engineering-principles-colors">{Object.entries(profile.design.observed).filter(([, value]) => value.startsWith("#")).map(([name, value]) => <span key={name}><i style={{ background: value }} />{name}<code>{value}</code></span>)}</div>
      </div>
      <footer><span>General guidance; use judgment for the task at hand.</span><button onClick={onClose}>Done</button></footer>
    </section>
  </div>;
}
