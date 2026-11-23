export interface FacePart {
  n?: string;
  u?: string;
}

const NUMBER = /([-−]?\d[\d.,:]*)/;

export const splitFaceValue = (text: string | null | undefined, unit?: string | null): FacePart[] => {
  const parts: FacePart[] = [];
  for (const piece of (text ?? '').split(NUMBER)) {
    if (!piece) continue;
    if (NUMBER.test(piece) && /^[-−]?\d/.test(piece)) parts.push({ n: piece });
    else {
      const word = piece.trim();
      if (word) parts.push({ u: word });
    }
  }
  const extra = unit?.trim();
  if (extra) parts.push({ u: extra });
  return parts;
};

const wide = (ch: string) => { const code = ch.codePointAt(0) ?? 0; return (code >= 0x2e80 && code <= 0x9fff) || (code >= 0xac00 && code <= 0xd7af) || (code >= 0xff00 && code <= 0xffef); };

export const faceFit = (parts: FacePart[], unitScale = 0.52): number => {
  let width = 0;
  parts.forEach((part, i) => {
    if (i > 0) width += 0.14;
    if (part.n) width += part.n.length * 0.6;
    if (part.u) for (const ch of part.u) width += (wide(ch) ? 1 : 0.56) * unitScale;
  });
  return Math.max(1, Math.round(width * 100) / 100);
};

export const textFit = (text: string): number => {
  let width = 0;
  for (const ch of text) width += wide(ch) ? 1 : /\d/.test(ch) ? 0.6 : 0.56;
  return Math.max(1, Math.round(width * 100) / 100);
};
