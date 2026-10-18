// 把 shell/i18n/zh-CN.tsv 生成为 shell/src/i18n/table.rs（按英文键排序，二分查找）。
// 改完 TSV 跑 `node scripts/gen-i18n.mjs`，然后重新构建。
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "shell", "i18n", "zh-CN.tsv");
const out = join(root, "shell", "src", "i18n", "table.rs");

// 一行两列（英文 <Tab> 译文）或三列（英文 <Tab> 译文 <Tab> 上下文）。
// 第三列给「同一英文串在不同界面含义不同」的情况：`Mix` 在插件参数里是干湿混合量，
// 在菜单/窗口标题/命令面板里是混音台窗口。带上下文的条目在表里的键是
// 「上下文 + U+001F + 英文」，由上游 `i18n::t_ctx(ctx, s)` 先查，查不到再退回普通键。
const CTX_SEP = "\u001f";
const seen = new Map();
for (const raw of readFileSync(src, "utf8").split(/\r?\n/)) {
  const line = raw.replace(/\s+$/, "");
  if (!line.trim() || line.startsWith("#")) continue;
  const parts = line.split("\t");
  if (parts.length < 2 || parts.length > 3) {
    throw new Error(`zh-CN.tsv: 需要 2 或 3 个制表符分隔字段：${raw}`);
  }
  const [en, zh, ctx = ""] = parts;
  if (!en || !zh) throw new Error(`zh-CN.tsv: 空字段：${raw}`);
  const key = ctx ? `${ctx}${CTX_SEP}${en}` : en;
  if (seen.has(key) && seen.get(key) !== zh) {
    throw new Error(`zh-CN.tsv: 「${ctx ? `${ctx} / ` : ""}${en}」有两条不同的译文`);
  }
  seen.set(key, zh);
}

const rows = [...seen].sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
// 控制字符必须转成 \u{..}：表键里可能有 U+001F（上下文分隔符），
// 让它以不可见字节原样落进 .rs 文件太脆。
const esc = (s) =>
  s
    .replace(/\\/g, "\\\\")
    .replace(/"/g, '\\"')
    .replace(/[\u0000-\u001f\u007f]/g, (c) => `\\u{${c.codePointAt(0).toString(16)}}`);
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