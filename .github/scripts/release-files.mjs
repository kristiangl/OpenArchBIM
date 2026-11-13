// Gathers what a release publishes from the installers the build made, and writes latest.json, the file the
// program reads to find out whether there is a newer version (Help > Check for Updates).
//
//   node release-files.mjs <folder of build artifacts> <output folder> <version> <owner/repo> <tag>
//
// Every installer is copied to the output folder under a name with no spaces (a release replaces spaces itself,
// and the addresses in latest.json have to match what is uploaded). latest.json lists, for each platform, the
// address of the installer an update uses and its signature (the text of the .sig file made beside it by the
// build). A platform with no signed installer is left out, and the script fails if none has one.
import { readdirSync, statSync, readFileSync, writeFileSync, copyFileSync, mkdirSync } from "node:fs";
import { join, basename } from "node:path";

const [from, out, version, repo, tag] = process.argv.slice(2);
if (!from || !out || !version || !repo || !tag) {
  console.error("usage: release-files.mjs <artifacts> <out> <version> <owner/repo> <tag>");
  process.exit(2);
}
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`"${version}" is not a version like 0.3.0 (the tag is v0.3.0)`);
  process.exit(2);
}

const walk = (dir) => readdirSync(dir).flatMap((n) => { const p = join(dir, n); return statSync(p).isDirectory() ? walk(p) : [p]; });
const files = walk(from);
const clean = (name) => name.replace(/\s+/g, ".");
// the Mac update bundle is named for the app alone; give it the version and chip so it sits with the rest
const publishName = (path) => {
  const n = basename(path);
  const mac = n.match(/^(.*)\.app\.tar\.gz(\.sig)?$/);
  return clean(mac ? `${mac[1]}_${version}_aarch64.app.tar.gz${mac[2] || ""}` : n);
};

mkdirSync(out, { recursive: true });
const published = new Map(); // source path -> published name
for (const f of files) {
  if (!/\.(exe|msi|dmg|deb|rpm|AppImage|app\.tar\.gz|sig)$/.test(f)) continue;
  const name = publishName(f);
  if ([...published.values()].includes(name)) { console.error(`two files would be published as ${name}`); process.exit(1); }
  copyFileSync(f, join(out, name));
  published.set(f, name);
}

const url = (name) => `https://github.com/${repo}/releases/download/${tag}/${encodeURIComponent(name)}`;
// an installer with its signature beside it, by the ending of its name
const signed = (ending) => {
  const f = files.find((p) => p.endsWith(ending) && files.includes(`${p}.sig`));
  return f ? { signature: readFileSync(`${f}.sig`, "utf8").trim(), url: url(published.get(f)) } : null;
};

const platforms = {};
const add = (key, entry) => { if (entry) platforms[key] = entry; };
const nsis = signed("-setup.exe"), msi = signed(".msi"), appimage = signed(".AppImage");
// Windows: the setup program is what an update runs; a program installed from the .msi is offered the .msi
add("windows-x86_64", nsis || msi);
add("windows-x86_64-nsis", nsis);
add("windows-x86_64-msi", msi);
add("darwin-aarch64", signed(".app.tar.gz"));
add("linux-x86_64", appimage);
add("linux-x86_64-appimage", appimage);
add("linux-x86_64-deb", signed(".deb"));
add("linux-x86_64-rpm", signed(".rpm"));

if (!Object.keys(platforms).length) {
  console.error("No signed installer found: was the build run with the signing key (TAURI_SIGNING_PRIVATE_KEY)?");
  process.exit(1);
}
const latest = { version, notes: `OpenArch BIM ${version}. Release notes: https://github.com/${repo}/releases/tag/${tag}`, pub_date: new Date().toISOString(), platforms };
writeFileSync(join(out, "latest.json"), JSON.stringify(latest, null, 2) + "\n");
console.log(`latest.json for ${version}: ${Object.keys(platforms).join(", ")}`);
console.log([...published.values()].sort().join("\n"));
