interface KbdProps {
  combo: string;
  className?: string;
}

export default function Kbd({ combo, className }: KbdProps) {
  const tokens = combo.split("+").map((t) => t.trim()).filter(Boolean);
  if (tokens.length === 0) return null;

  return (
    <span className={`kbd-group${className ? ` ${className}` : ""}`}>
      {tokens.map((token, i) => (
        <span className="kbd-group-item" key={`${i}-${token}`}>
          {i > 0 && <span className="kbd-plus" aria-hidden="true">+</span>}
          <kbd className="kbd">{token}</kbd>
        </span>
      ))}
    </span>
  );
}
