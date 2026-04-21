export interface KbdProps {
  combo: string;
}

export default function Kbd({ combo }: KbdProps) {
  const tokens = combo.split("+").map((t) => t.trim()).filter(Boolean);
  if (tokens.length === 0) return null;
  return (
    <span className="lw-kbd-group">
      {tokens.map((token, i) => (
        <span key={`${i}-${token}`} className="lw-kbd-item">
          {i > 0 && <span aria-hidden="true" className="lw-kbd-plus">+</span>}
          <kbd className="lw-kbd">{token}</kbd>
        </span>
      ))}
    </span>
  );
}
