import { mkdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { grammars, injections, type GrammarInfo } from "tm-grammars";
import { themes } from "tm-themes";

const root = join(import.meta.dir, "..");
const langsOut = join(root, "shiki-langs", "generated");
const themesOut = join(root, "shiki-themes", "generated");

type JsonObject = Record<string, unknown>;

function object(value: unknown): JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonObject)
    : {};
}

function rustString(value: string): string {
  let output = '"';
  for (const char of value) {
    const code = char.codePointAt(0)!;
    switch (char) {
      case '"':
        output += '\\"';
        break;
      case "\\":
        output += "\\\\";
        break;
      case "\n":
        output += "\\n";
        break;
      case "\r":
        output += "\\r";
        break;
      case "\t":
        output += "\\t";
        break;
      default:
        output +=
          code < 0x20 || code === 0x7f || /\p{Cf}/u.test(char) ? `\\u{${code.toString(16)}}` : char;
    }
  }
  return `${output}"`;
}

function ident(value: string): string {
  const normalized = value.replaceAll(/[^a-zA-Z0-9_]/g, "_");
  return (/^[0-9]/.test(normalized) ? `_${normalized}` : normalized).toUpperCase();
}

function moduleIdent(prefix: string, value: string): string {
  const normalized = value.replaceAll(/[^a-zA-Z0-9_]/g, "_").toLowerCase();
  return `${prefix}_${/^[0-9]/.test(normalized) ? `_${normalized}` : normalized}`;
}

function macroIdent(value: string): string | undefined {
  if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(value)) return undefined;
  return value;
}

function staticString(value: unknown): string {
  return typeof value === "string" ? `Some(${rustString(value)})` : "None";
}

function staticList(values: unknown, render: (value: unknown) => string): string {
  const items = Array.isArray(values) ? values : [];
  return `&[${items.map(render).join(",")}]`;
}

function staticMap(values: JsonObject, render: (value: unknown) => string): string {
  const entries = Object.entries(values).sort(([left], [right]) => left.localeCompare(right));
  return `&[${entries
    .map(([key, value]) => `StaticRawMapEntry::new(${rustString(key)},${render(value)})`)
    .join(",")}]`;
}

function captures(value: unknown): string {
  const entries: [string, unknown][] = Array.isArray(value)
    ? value.map((item, index) => [String(index), item])
    : Object.entries(object(value));
  const normalized = Object.fromEntries(entries.filter(([key]) => /^\d+$/.test(key)));
  return staticMap(normalized, (capture) => {
    if (typeof capture === "string")
      return `StaticRawRule{name:Some(${rustString(capture)}),..StaticRawRule::EMPTY}`;
    return rule(capture);
  });
}

function repository(value: unknown): string {
  return staticMap(object(value), (entry) => {
    if (Array.isArray(entry))
      return `StaticRawRule{patterns:${staticList(entry, rule)},..StaticRawRule::EMPTY}`;
    return rule(entry);
  });
}

function boolish(value: unknown): boolean {
  return (
    value === true ||
    value === 1 ||
    value === "1" ||
    (typeof value === "string" && value.toLowerCase() === "true")
  );
}

function rule(value: unknown): string {
  const raw = object(value);
  const fields: string[] = [];
  const strings: [string, string][] = [
    ["include", "include"],
    ["name", "name"],
    ["contentName", "content_name"],
    ["match", "match_pattern"],
    ["begin", "begin"],
    ["end", "end"],
    ["while", "while_pattern"],
  ];
  for (const [jsonName, rustName] of strings) {
    if (typeof raw[jsonName] === "string")
      fields.push(`${rustName}:${staticString(raw[jsonName])}`);
  }
  const maps: [string, string, (value: unknown) => string][] = [
    ["captures", "captures", captures],
    ["beginCaptures", "begin_captures", captures],
    ["endCaptures", "end_captures", captures],
    ["whileCaptures", "while_captures", captures],
    ["repository", "repository", repository],
  ];
  for (const [jsonName, rustName, render] of maps) {
    if (raw[jsonName] !== undefined) fields.push(`${rustName}:${render(raw[jsonName])}`);
  }
  if (Array.isArray(raw.patterns)) fields.push(`patterns:${staticList(raw.patterns, rule)}`);
  if (boolish(raw.applyEndPatternLast)) fields.push("apply_end_pattern_last:true");
  return fields.length === 0
    ? "StaticRawRule::EMPTY"
    : fields.length === 14
      ? `StaticRawRule{${fields.join(",")}}`
      : `StaticRawRule{${fields.join(",")},..StaticRawRule::EMPTY}`;
}

function grammar(value: unknown): string {
  const raw = object(value);
  return `StaticRawGrammar{
    name:${staticString(raw.name)},
    scope_name:${rustString(String(raw.scopeName ?? ""))},
    patterns:${staticList(raw.patterns, rule)},
    repository:${repository(raw.repository)},
    injections:${staticMap(object(raw.injections), rule)},
    injection_selector:${staticString(raw.injectionSelector)},
}`;
}

function themeScope(value: unknown): string {
  if (typeof value === "string") return `StaticRawThemeScope::String(${rustString(value)})`;
  if (Array.isArray(value))
    return `StaticRawThemeScope::Array(${staticList(value, (item) => rustString(String(item)))})`;
  return "StaticRawThemeScope::Missing";
}

function themeSettings(value: unknown): string {
  const raw = object(value);
  const fields: string[] = [];
  if (typeof raw.foreground === "string") fields.push(`foreground:${staticString(raw.foreground)}`);
  if (typeof raw.background === "string") fields.push(`background:${staticString(raw.background)}`);
  if (typeof raw.fontStyle === "string") fields.push(`font_style:${staticString(raw.fontStyle)}`);
  return fields.length === 0
    ? "StaticRawThemeSettings::EMPTY"
    : fields.length === 3
      ? `StaticRawThemeSettings{${fields.join(",")}}`
      : `StaticRawThemeSettings{${fields.join(",")},..StaticRawThemeSettings::EMPTY}`;
}

function themeRule(value: unknown): string {
  const raw = object(value);
  const fields: string[] = [];
  if (raw.scope !== undefined) fields.push(`scope:${themeScope(raw.scope)}`);
  if (raw.settings !== undefined) fields.push(`settings:${themeSettings(raw.settings)}`);
  return fields.length === 0
    ? "StaticRawThemeRule::EMPTY"
    : fields.length === 2
      ? `StaticRawThemeRule{${fields.join(",")}}`
      : `StaticRawThemeRule{${fields.join(",")},..StaticRawThemeRule::EMPTY}`;
}

function rawTheme(value: unknown): string {
  const raw = object(value);
  const sourceColors = object(raw.colors);
  const colors = Object.fromEntries(
    ["editor.foreground", "editor.background"]
      .filter((key) => typeof sourceColors[key] === "string")
      .map((key) => [key, sourceColors[key]]),
  );
  return `StaticRawTheme{
    name:${staticString(raw.name)},
    fg:${staticString(raw.fg)},
    bg:${staticString(raw.bg)},
    colors:${staticMap(colors, (color) => rustString(String(color)))},
    settings:${staticList(raw.settings, themeRule)},
    token_colors:${staticList(raw.tokenColors, themeRule)},
}`;
}

function sourcePath(info: GrammarInfo): string {
  return join(root, "node_modules", "tm-grammars", "grammars", `${info.name}.json`);
}

async function generateLanguages(): Promise<void> {
  await rm(langsOut, { recursive: true, force: true });
  await mkdir(langsOut, { recursive: true });

  const all = [...grammars, ...injections].sort((a, b) => a.name.localeCompare(b.name));
  const byName = new Map(all.map((info) => [info.name, info]));
  const modules: string[] = [];
  const exports: string[] = [];
  const groups: string[] = [];
  const macroArms: string[] = [];

  for (const info of all) {
    const symbol = ident(info.name);
    const moduleName = moduleIdent("lang", info.name);
    const deps = info.embedded ?? [];
    const aliases = info.aliases ?? [];
    const injectTo = (info as GrammarInfo & { injectTo?: string[] }).injectTo ?? [];
    const raw = await Bun.file(sourcePath(info)).json();

    await writeFile(
      join(langsOut, `${info.name}.rs`),
      `// Generated by script/main.ts. Do not edit.
#[allow(unused_imports)]
use shiki::{LanguageDefinition, StaticRawGrammar, StaticRawMapEntry, StaticRawRule};

pub static GRAMMAR: StaticRawGrammar = ${grammar(raw)};

pub static ${symbol}: LanguageDefinition = LanguageDefinition::new(
    ${rustString(info.name)},
    ${rustString(info.displayName || info.name)},
    ${rustString(info.scopeName)},
    &[${aliases.map(rustString).join(",")}],
    &[${deps.map(rustString).join(",")}],
    &[${injectTo.map(rustString).join(",")}],
    &GRAMMAR,
);
`,
    );
    modules.push(`#[path = "../generated/${info.name}.rs"] pub mod ${moduleName};`);
    exports.push(`pub use ${moduleName}::${symbol};`);

    const closure: GrammarInfo[] = [];
    const seen = new Set<string>();
    const visit = (name: string) => {
      if (seen.has(name)) return;
      seen.add(name);
      const dependency = byName.get(name);
      if (!dependency) return;
      for (const child of dependency.embedded ?? []) visit(child);
      closure.push(dependency);
    };
    visit(info.name);
    let addedInjection = true;
    while (addedInjection) {
      addedInjection = false;
      for (const injection of injections) {
        if (seen.has(injection.name)) continue;
        if (injection.embeddedIn?.some((parent) => seen.has(parent))) {
          visit(injection.name);
          addedInjection = true;
        }
      }
    }
    groups.push(
      `pub static ${symbol}_GROUP: LanguageGroup = &[${closure.map((item) => `&${ident(item.name)}`).join(",")}];`,
    );

    const token = macroIdent(info.name);
    if (token) macroArms.push(`    (${token}) => { $crate::generated::${symbol}_GROUP };`);
    for (const alias of aliases) {
      const aliasToken = macroIdent(alias);
      if (aliasToken)
        macroArms.push(`    (${aliasToken}) => { $crate::generated::${symbol}_GROUP };`);
    }
  }

  await writeFile(
    join(root, "shiki-langs", "src", "generated.rs"),
    `// Generated by script/main.ts. Do not edit.
use shiki::LanguageGroup;

${modules.join("\n")}
${exports.join("\n")}

${groups.join("\n")}

pub static ALL_LANGUAGES: LanguageGroup = &[${all.map((item) => `&${ident(item.name)}`).join(",")}];
`,
  );

  await writeFile(
    join(root, "shiki-langs", "src", "macros.rs"),
    `// Generated by script/main.ts. Do not edit.
#[doc(hidden)]
#[macro_export]
macro_rules! __language_group {
${macroArms.join("\n")}
}

#[macro_export]
macro_rules! languages {
    ($($language:ident),* $(,)?) => {
        $crate::LanguageBundle::from_groups(&[
            $($crate::__language_group!($language)),*
        ])
    };
}
`,
  );
}

async function generateThemes(): Promise<void> {
  await rm(themesOut, { recursive: true, force: true });
  await mkdir(themesOut, { recursive: true });

  const sorted = [...themes].sort((a, b) => a.name.localeCompare(b.name));
  const modules: string[] = [];
  const exports: string[] = [];
  const macroArms: string[] = [];
  for (const info of sorted) {
    const fileName = `${info.name}.json`;
    const source = join(root, "node_modules", "tm-themes", "themes", fileName);
    const raw = await Bun.file(source).json();
    const symbol = ident(info.name);
    const moduleName = moduleIdent("theme", info.name);

    await writeFile(
      join(themesOut, `${info.name}.rs`),
      `// Generated by script/main.ts. Do not edit.
use shiki::{StaticRawMapEntry, StaticRawTheme, StaticRawThemeRule, StaticRawThemeScope, StaticRawThemeSettings, ThemeDefinition};

pub static THEME: StaticRawTheme = ${rawTheme(raw)};

pub static ${symbol}: ThemeDefinition = ThemeDefinition::new(
    ${rustString(info.name)},
    ${rustString(info.displayName || info.name)},
    &THEME,
);
`,
    );
    modules.push(`#[path = "../generated/${info.name}.rs"] pub mod ${moduleName};`);
    exports.push(`pub use ${moduleName}::${symbol};`);
    const token = macroIdent(info.name);
    if (token) macroArms.push(`    (${token}) => { &$crate::generated::${symbol} };`);
  }

  await writeFile(
    join(root, "shiki-themes", "src", "generated.rs"),
    `// Generated by script/main.ts. Do not edit.
use shiki::ThemeDefinition;

${modules.join("\n")}
${exports.join("\n")}

pub static ALL_THEMES: &[&ThemeDefinition] = &[${sorted.map((item) => `&${ident(item.name)}`).join(",")}];
`,
  );
  await writeFile(
    join(root, "shiki-themes", "src", "macros.rs"),
    `// Generated by script/main.ts. Do not edit.
#[doc(hidden)]
#[macro_export]
macro_rules! __theme {
${macroArms.join("\n")}
}

#[macro_export]
macro_rules! themes {
    ($($theme:ident),* $(,)?) => {
        $crate::ThemeBundle::new(&[
            $($crate::__theme!($theme)),*
        ])
    };
}
`,
  );
}

await Promise.all([generateLanguages(), generateThemes()]);
const formatter = Bun.spawn(["cargo", "fmt", "--all"], {
  cwd: root,
  stdout: "inherit",
  stderr: "inherit",
});
if ((await formatter.exited) !== 0) throw new Error("cargo fmt failed");
console.log(
  `Generated ${grammars.length + injections.length} languages and ${themes.length} themes.`,
);
