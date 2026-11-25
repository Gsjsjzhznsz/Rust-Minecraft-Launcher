/* 格式化与纯工具 */

export function bytes(n: number): string {
  if (!n) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
}

export function fmtBytes(n: number): string {
  const mb = Math.floor(n / (1024 * 1024));
  if (mb >= 1024) return (mb / 1024).toFixed(1) + "GB";
  return mb + "MB";
}

export function avatarStyle(uuid: string): string {
  let h = 0;
  for (const c of uuid) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const hue = h % 360;
  return `background: linear-gradient(135deg, hsl(${hue} 70% 62%), hsl(${(hue + 50) % 360} 70% 45%))`;
}

export function avatarLetter(name: string): string {
  return (name[0] ?? "?").toUpperCase();
}
