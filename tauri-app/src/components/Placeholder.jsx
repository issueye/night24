export function Placeholder({ title, detail }) {
  return (
    <div className="placeholder">
      <strong>{title}</strong>
      <span>{detail}</span>
    </div>
  );
}
