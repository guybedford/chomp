import BinWrapper from 'bin-wrapper';
import { readFileSync } from 'fs';
import path from 'path';
import { spawn } from 'child_process';
import { fileURLToPath } from 'url';

const { version } = JSON.parse(readFileSync(new URL('package.json', import.meta.url), 'utf8'));
const base = `https://github.com/guybedford/chomp/releases/download/${version}`

const bin = new BinWrapper()
  .src(`${base}/chomp-macos-${version}.tar.gz`, 'darwin')
  .src(`${base}/chomp-linux-${version}.tar.gz`, 'linux', 'x64')
  .src(`${base}/chomp-windows-${version}.zip`, 'win32', 'x64')
  .dest(path.join('vendor'))
  .use(process.platform === 'win32' ? 'chomp.exe' : 'chomp')
  .version(version);

spawn(fileURLToPath(new URL(bin.path(), import.meta.url)), process.argv.slice(2), { stdio: 'inherit' });
