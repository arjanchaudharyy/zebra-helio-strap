import { copyFileSync, existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const scriptDir = dirname(fileURLToPath(import.meta.url));
const root = resolve(scriptDir, '../..');
const source = join(root, 'src-tauri', 'icons', 'icon-master.png');
const output = join(root, 'src-tauri', 'icons');
const publicIcon = join(root, 'public', 'zeppbridge-icon.png');
const tauriCli = join(root, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');

if (!existsSync(tauriCli)) {
  throw new Error(`@tauri-apps/cli is missing at ${tauriCli}; use the repository install.`);
}

const result = spawnSync(process.execPath, [tauriCli, 'icon', source, '-o', output], {
  cwd: root,
  stdio: 'inherit',
  windowsHide: true,
});
if (result.status !== 0) process.exit(result.status ?? 1);

// The product currently ships Windows desktop bundles. Keep the output folder
// deterministic while still letting the official Tauri generator do the work.
for (const generatedPlatform of ['android', 'ios']) {
  rmSync(join(output, generatedPlatform), { recursive: true, force: true });
}

const normaliseIcns = (path) => {
  if (!existsSync(path)) return;
  const data = readFileSync(path);
  if (data.length < 8 || data.toString('latin1', 0, 4) !== 'icns') return;
  const declared = data.readUInt32BE(4);
  const blocks = [];
  let offset = 8;
  while (offset + 8 <= Math.min(declared, data.length)) {
    const length = data.readUInt32BE(offset + 4);
    if (length < 8 || offset + length > data.length) return;
    blocks.push({ type: data.toString('latin1', offset, offset + 4), body: data.subarray(offset, offset + length) });
    offset += length;
  }
  if (offset !== declared || blocks.length === 0) return;

  blocks.sort((left, right) => (left.type < right.type ? -1 : left.type > right.type ? 1 : 0));
  const header = Buffer.alloc(8);
  header.write('icns', 0, 'latin1');
  header.writeUInt32BE(declared, 4);
  const normalised = Buffer.concat([header, ...blocks.map((block) => block.body)]);
  if (normalised.length !== data.length) return;
  if (normalised.equals(data)) return;
  writeFileRetry(path, normalised);
  console.log(`Normalised chunk order in ${path}`);
};

const writeFileRetry = (path, data) => {
  let last;
  for (let attempt = 1; attempt <= 6; attempt += 1) {
    try {
      writeFileSync(path, data);
      return;
    } catch (error) {
      last = error;
      const code = error && error.code;
      if (code !== 'UNKNOWN' && code !== 'EBUSY' && code !== 'EPERM' && code !== 'EACCES') throw error;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 200 * attempt);
    }
  }
  throw last;
};

normaliseIcns(join(output, 'icon.icns'));

copyFileSync(join(output, 'icon.png'), publicIcon);
console.log(`Copied ${join(output, 'icon.png')} -> ${publicIcon}`);

const publicFavicon = join(root, 'public', 'favicon.png');
copyFileSync(join(output, '64x64.png'), publicFavicon);
console.log(`Copied ${join(output, '64x64.png')} -> ${publicFavicon}`);
