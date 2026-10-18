// 把 shell/i18n/zh-CN.tsv 生成为 shell/src/i18n/table.rs（按英文键排序，二分查找）。
// 改完 TSV 跑 `node scripts/gen-i18n.mjs`，然后重新构建。
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "shell", "i18n", "zh-CN.tsv");
const out = join(root, "shell", "src", "i18n", "table.rs");

const seen = new Map();
for (const raw of readFileSync(src, "utf8").split(/\r?\n/)) {
  const line = raw.replace(/\s+$/, "");
  if (!line.trim() || line.startsWith("#")) continue;
  const at = line.indexOf("\t");
  if (at < 0) throw new Error(`zh-CN.tsv: 少一个制表符分隔：${raw}`);
  const en = line.slice(0, at);
  const zh = line.slice(at + 1);
  if (!en || !zh) throw new Error(`zh-CN.tsv: 空字段：${raw}`);
  if (seen.has(en) && seen.get(en) !== zh) {
    throw new Error(`zh-CN.tsv: 「${en}」有两条不同的译文`);
  }
  seen.set(en, zh);
}

const rows = [...seen].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
const esc = (s) => s.replace(/\\/g, "\\\\").replace(/"/g, '\\"');
mkdirSync(dirname(out), { recursive: true });
writeFileSync(
  out,
  [
    "//! 由 `node scripts/gen-i18n.mjs` 从 `shell/i18n/zh-CN.tsv` 生成，请勿手改。",
    "",
    `/// ${rows.length} 条界面字符串的英 → 简中对照，按英文键排序（二分查找）。`,
    "pub static ZH_CN: &[(&str, &str)] = &[",
    ...rows.map(([en, zh]) => `    ("${esc(en)}", "${esc(zh)}"),`),
    "];",
    "",
  ].join("\n"),
  "utf8",
);
console.log(`gen-i18n: ${rows.length} entries -> ${out}`);