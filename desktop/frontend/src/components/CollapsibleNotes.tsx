interface CollapsibleNotesProps {
  notes: string[];
  label?: string;
}

export function CollapsibleNotes({ notes, label = "补充说明" }: CollapsibleNotesProps) {
  const items = notes
    .map((note) => String(note || "").trim())
    .filter(Boolean);

  if (!items.length) return null;

  return (
    <details className="notes-block">
      <summary className="notes-summary">
        <span className="notes-summary-copy">
          <span className="notes-summary-label">{label}</span>
          <span className="notes-summary-preview">{items[0]}</span>
        </span>
        <strong>{items.length}</strong>
      </summary>
      <div className="notes-list">
        {items.map((note, index) => <p key={`${index}-${note}`}>{note}</p>)}
      </div>
    </details>
  );
}
