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

/* 头像只输出色相变量（--av-h），形状与配色由当前设计语言的 CSS 决定 */
export function avatarStyle(uuid: string): string {
  let h = 0;
  for (const c of uuid) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const hue = h % 360;
  return `--av-h: ${hue}`;
}

export function avatarLetter(name: string): string {
  return (name[0] ?? "?").toUpperCase();
}
