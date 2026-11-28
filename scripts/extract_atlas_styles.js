// 从 style-atlas 的 JS 包中提取 30 种风格定义（palette / tokens / category）
const fs = require("fs");
const src = fs.readFileSync("/tmp/atlas/atlas.js", "utf8");

function extract(anchor) {
  const start = src.indexOf(anchor);
  if (start < 0) throw new Error("anchor not found: " + anchor);
  const arrStart = src.indexOf("[", start);
  let i = arrStart, depth = 0, inStr = null, esc = false;
  for (; i < src.length; i++) {
    const ch = src[i];
    if (inStr) {
      if (esc) { esc = false; continue; }
      if (ch === "\\") { esc = true; continue; }
      if (ch === inStr) inStr = null;
      continue;
    }
    if (ch === "`" || ch === '"' || ch === "'") { inStr = ch; continue; }
    if (ch === "[" || ch === "(" || ch === "{") depth++;
    else if (ch === "]" || ch === ")" || ch === "}") {
      depth--;
      if (depth === 0) { i++; break; }
    }
  }
  return src.slice(arrStart, i);
}

// 原始片段即合法 JS（含模板字面量），直接求值，不做引号替换
const styles = [
  ...new Function("return " + extract(",d=[{id:"))(),
  ...new Function("return " + extract(",f=[{id:"))(),
  ...new Function("return " + extract(",p=[{id:"))(),
];
fs.writeFileSync("/tmp/atlas/styles.json", JSON.stringify(styles, null, 2));
console.log("TOTAL", styles.length);
console.log(styles.map((s) => s.id).join(","));
