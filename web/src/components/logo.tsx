import { useId } from "react";

/** Galileo's mark: Jupiter and its moons in a line, as Galileo drew them in 1610. */
export function Logo({ size = 28, tile = false, className }: { size?: number; tile?: boolean; className?: string }) {
  const id = useId();
  return (
    <svg width={size} height={size} viewBox="0 0 64 64" className={className} aria-hidden="true">
      <defs>
        <linearGradient id={`${id}p`} x1="0" y1="0" x2="1" y2="1"><stop offset="0" stopColor="#ff5ccf" /><stop offset="1" stopColor="#8b5cff" /></linearGradient>
        <linearGradient id={`${id}b`} x1="0" y1="0" x2="1" y2="1"><stop offset="0" stopColor="#1c0f38" /><stop offset="1" stopColor="#0d0818" /></linearGradient>
      </defs>
      {tile && <rect width="64" height="64" rx="14" fill={`url(#${id}b)`} />}
      <line x1="4" y1="32" x2="60" y2="32" stroke="#5ee9ff" strokeOpacity="0.5" strokeWidth="2" />
      <circle cx="27" cy="32" r="14" fill={`url(#${id}p)`} />
      <path d="M15 27.5h24M14 36.5h26" stroke="#1c0f38" strokeOpacity="0.3" strokeWidth="2.4" />
      <circle cx="9" cy="32" r="3.5" fill="#ece6ff" />
      <circle cx="48" cy="32" r="4.5" fill="#5ee9ff" />
      <circle cx="57" cy="32" r="2.5" fill="#ece6ff" />
    </svg>
  );
}
